use std::time::Instant;
use std::collections::HashMap;
use crate::heuristic;
use crate::opponent::OpponentProfile;
use crate::simulator::{SimState, MAX_SNAKES};
use crate::types::Direction;

const TT_SIZE: usize = 1 << 22;

#[derive(Clone, Copy, Default)]
struct TtEntry {
    hash: u64,
    score: f32,
    depth: u8,
    best_move: u8,
    flag: u8,
    generation: u8,
}

pub struct TranspositionTable {
    table: Vec<TtEntry>,
    generation: u8,
}

impl TranspositionTable {
    pub fn new() -> Self {
        TranspositionTable { table: vec![TtEntry::default(); TT_SIZE], generation: 0 }
    }

    pub fn new_search(&mut self) {
        self.generation = self.generation.wrapping_add(1);
    }

    fn probe(&self, hash: u64) -> Option<&TtEntry> {
        let entry = &self.table[hash as usize & (TT_SIZE - 1)];
        if entry.hash == hash && entry.depth > 0 && entry.generation == self.generation {
            Some(entry)
        } else {
            None
        }
    }

    fn store(&mut self, hash: u64, score: f32, depth: u8, best_move: u8, flag: u8) {
        let idx = hash as usize & (TT_SIZE - 1);
        let existing = &self.table[idx];
        if existing.generation != self.generation || existing.hash != hash || depth >= existing.depth {
            self.table[idx] = TtEntry { hash, score, depth, best_move, flag, generation: self.generation };
        }
    }
}

#[inline]
fn hash_state(state: &SimState) -> u64 {
    let mut h: u64 = 14695981039346656037;

    for i in 0..state.num_snakes as usize {
        let s = &state.snakes[i];
        if s.alive {
            let contribution = (s.head() as u64)
                .wrapping_mul(0x9e3779b97f4a7c15u64)
                .wrapping_add((s.len as u64).wrapping_mul(0x6c62272e07bb0142u64))
                .wrapping_mul((i as u64 + 1).wrapping_mul(0x517cc1b727220a95u64));
            h ^= contribution;
        }
    }

    h ^= (state.food as u64) ^ ((state.food >> 64) as u64).wrapping_mul(0xbf58476d1ce4e5b9);
    h ^= (state.all_bodies as u64).wrapping_mul(0x94d049bb133111eb);

    h
}

const MAX_DEPTH: usize = 24;

struct KillerTable {
    moves: [[Option<Direction>; 2]; MAX_DEPTH],
}

impl KillerTable {
    fn new() -> Self {
        KillerTable { moves: [[None; 2]; MAX_DEPTH] }
    }

    fn store(&mut self, depth: usize, mv: Direction) {
        if depth < MAX_DEPTH {
            let slot = &mut self.moves[depth];
            if slot[0] != Some(mv) {
                slot[1] = slot[0];
                slot[0] = Some(mv);
            }
        }
    }

    fn get(&self, depth: usize) -> [Option<Direction>; 2] {
        if depth < MAX_DEPTH { self.moves[depth] } else { [None; 2] }
    }
}

struct HistoryTable {
    scores: [[i32; 4]; 121],
}

impl HistoryTable {
    fn new() -> Self {
        HistoryTable { scores: [[0i32; 4]; 121] }
    }

    fn add(&mut self, from_pos: u8, dir: Direction, depth: i32) {
        let pos = from_pos as usize;
        if pos < 121 {
            self.scores[pos][dir.to_index()] += depth * depth;
        }
    }

    fn get(&self, from_pos: u8, dir: Direction) -> i32 {
        let pos = from_pos as usize;
        if pos < 121 { self.scores[pos][dir.to_index()] } else { 0 }
    }
}

pub struct SearchResult {
    pub score: f32,
    pub depth: i32,
    pub nodes: u64,
    pub move_scores: Vec<(Direction, f32)>,
}

pub struct MinimaxEngine<'a> {
    tt: &'a mut TranspositionTable,
    killers: KillerTable,
    history: HistoryTable,
    start_time: Instant,
    time_budget_ms: u64,
    nodes: u64,
    aborted: bool,
    profiles: HashMap<u64, OpponentProfile>,
}

impl<'a> MinimaxEngine<'a> {
    pub fn new(time_budget_ms: u64, tt: &'a mut TranspositionTable) -> Self {
        tt.new_search();
        MinimaxEngine {
            tt,
            killers: KillerTable::new(),
            history: HistoryTable::new(),
            start_time: Instant::now(),
            time_budget_ms,
            nodes: 0,
            aborted: false,
            profiles: HashMap::new(),
        }
    }

    pub fn set_profiles(&mut self, profiles: &HashMap<String, OpponentProfile>, state: &SimState) {
        for (id_str, profile) in profiles {
            let hash = crate::simulator::fnv_hash(id_str);
            for i in 0..state.num_snakes as usize {
                if state.snakes[i].alive && state.snakes[i].id_hash == hash {
                    self.profiles.insert(hash, profile.clone());
                    break;
                }
            }
        }
    }

    pub fn search(&mut self, state: &SimState) -> SearchResult {
        self.start_time = Instant::now();
        self.nodes = 0;
        self.aborted = false;

        let my_idx = state.my_idx as usize;
        let valid = state.get_valid_moves(my_idx);
        let my_moves: Vec<Direction> = valid.iter().filter_map(|&m| m).collect();

        if my_moves.is_empty() {
            return SearchResult {
                score: -1.0,
                depth: 0,
                nodes: 0,
                move_scores: Vec::new(),
            };
        }

        if my_moves.len() == 1 {
            return SearchResult {
                score: 0.0,
                depth: 0,
                nodes: 1,
                move_scores: vec![(my_moves[0], 0.0)],
            };
        }

        let num_enemies = state.alive_enemy_count();
        let max_depth = match num_enemies {
            0 | 1 => 24,
            2 => 14,
            3 => 10,
            _ => 8,
        };

        let board_size = (state.width as i32 * state.height as i32) as f32;
        let total_body: f32 = (0..state.num_snakes as usize)
            .filter(|&i| state.snakes[i].alive)
            .map(|i| state.snakes[i].len as f32)
            .sum();
        let occupancy = total_body / board_size;
        let is_endgame = num_enemies <= 1 && occupancy > 0.4;
        let max_depth = if is_endgame { max_depth.max(26) } else { max_depth };

        let mut best = SearchResult {
            score: -2.0,
            depth: 0,
            nodes: 0,
            move_scores: Vec::new(),
        };

        for depth in 1..=max_depth {
            self.aborted = false;
            let scores = self.search_at_depth(state, &my_moves, depth);
            if !self.aborted {
                let (_best_mv, best_sc) = scores.iter()
                    .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
                    .map(|&(d, s)| (d, s))
                    .unwrap_or((my_moves[0], -2.0));
                best = SearchResult {
                    score: best_sc,
                    depth,
                    nodes: self.nodes,
                    move_scores: scores,
                };
            } else {
                break;
            }

            if best.score >= 0.95 {
                break;
            }

            if self.elapsed_ms() > self.time_budget_ms * 50 / 100 {
                break;
            }
        }

        best.nodes = self.nodes;
        best
    }

    fn search_at_depth(
        &mut self,
        state: &SimState,
        my_moves: &[Direction],
        max_depth: i32,
    ) -> Vec<(Direction, f32)> {
        let mut all_scores = Vec::new();
        let mut alpha = f32::NEG_INFINITY;

        let tt_move = self
            .tt
            .probe(hash_state(state))
            .map(|e| Direction::from_index(e.best_move as usize));

        let ordered = self.order_my_moves(state, my_moves, tt_move, 0);

        for mv in ordered {
            if self.timed_out() {
                self.aborted = true;
                break;
            }

            let mut moves = [None; MAX_SNAKES];
            moves[state.my_idx as usize] = Some(mv);

            let score = self.paranoid_expand(state, &moves, 0, max_depth - 1, alpha, f32::INFINITY);

            all_scores.push((mv, score));
            if score > alpha {
                alpha = score;
            }
        }

        all_scores
    }

    fn paranoid_expand(
        &mut self,
        state: &SimState,
        partial_moves: &[Option<Direction>; MAX_SNAKES],
        enemy_idx: usize,
        depth: i32,
        alpha: f32,
        beta: f32,
    ) -> f32 {
        if self.timed_out() {
            self.aborted = true;
            return 0.0;
        }

        let next_enemy = (0..state.num_snakes as usize)
            .filter(|&i| {
                i != state.my_idx as usize
                    && state.snakes[i].alive
                    && partial_moves[i].is_none()
            })
            .nth(enemy_idx);

        match next_enemy {
            None => {
                let next = state.advance(partial_moves);
                self.nodes += 1;

                if !next.is_alive(next.my_idx as usize) {
                    return -1.0;
                }
                if next.is_game_over() {
                    return 1.0;
                }
                if depth <= 0 {
                    if self.timed_out() || !self.is_noisy(&next) || depth <= -2 {
                        return heuristic::evaluate(&next);
                    }
                }

                let hash = hash_state(&next);
                if let Some(entry) = self.tt.probe(hash) {
                    if entry.depth >= depth as u8 {
                        match entry.flag {
                            0 => return entry.score,
                            1 => {
                                if entry.score >= beta {
                                    return entry.score;
                                }
                            }
                            2 => {
                                if entry.score <= alpha {
                                    return entry.score;
                                }
                            }
                            _ => {}
                        }
                    }
                }

                let next_valid = next.get_valid_moves(next.my_idx as usize);
                let next_moves: Vec<Direction> =
                    next_valid.iter().filter_map(|&m| m).collect();

                if next_moves.is_empty() {
                    return -1.0;
                }

                let tt_move = self.tt.probe(hash).map(|e| Direction::from_index(e.best_move as usize));
                let ordered = self.order_my_moves(&next, &next_moves, tt_move, depth as usize);

                let mut best_score = f32::NEG_INFINITY;
                let mut best_move_found = ordered[0];
                let mut alpha_inner = alpha;
                let orig_alpha = alpha;

                for mv in ordered {
                    if self.timed_out() {
                        self.aborted = true;
                        return 0.0;
                    }

                    let mut new_moves = [None; MAX_SNAKES];
                    new_moves[next.my_idx as usize] = Some(mv);

                    let score = self.paranoid_expand(&next, &new_moves, 0, depth - 1, alpha_inner, beta);

                    if score > best_score {
                        best_score = score;
                        best_move_found = mv;
                    }
                    if score > alpha_inner {
                        alpha_inner = score;
                        if score >= beta {
                            self.killers.store(depth as usize, mv);
                            self.history.add(next.snakes[next.my_idx as usize].head(), mv, depth);
                            break;
                        }
                    }
                }

                let flag = if best_score <= orig_alpha {
                    2
                } else if best_score >= beta {
                    1
                } else {
                    0
                };
                self.tt.store(hash, best_score, depth as u8, best_move_found.to_index() as u8, flag);

                best_score
            }

            Some(ei) => {
                let profile = self.profiles.get(&state.snakes[ei].id_hash);
                let enemy_moves = state.get_weighted_enemy_moves_with_profile(ei, profile);

                if enemy_moves.is_empty() {
                    let mut m = *partial_moves;
                    m[ei] = Some(Direction::Up);
                    return self.paranoid_expand(state, &m, 0, depth, alpha, beta);
                }

                let mut worst_score = f32::INFINITY;
                let mut beta_inner = beta;

                for em in enemy_moves {
                    if self.timed_out() {
                        self.aborted = true;
                        return 0.0;
                    }

                    let mut m = *partial_moves;
                    m[ei] = Some(em);

                    let score = self.paranoid_expand(state, &m, 0, depth, alpha, beta_inner);

                    if score < worst_score {
                        worst_score = score;
                    }
                    if score < beta_inner {
                        beta_inner = score;
                        if beta_inner <= alpha {
                            break;
                        }
                    }
                }

                worst_score
            }
        }
    }

    fn order_my_moves(
        &self,
        state: &SimState,
        moves: &[Direction],
        tt_move: Option<Direction>,
        depth: usize,
    ) -> Vec<Direction> {
        let my_head = state.snakes[state.my_idx as usize].head();
        let killers = self.killers.get(depth);

        let mut scored: Vec<(Direction, i32)> = moves
            .iter()
            .map(|&mv| {
                let mut priority = self.history.get(my_head, mv);

                if Some(mv) == tt_move {
                    priority += 1_000_000;
                }
                if killers[0] == Some(mv) {
                    priority += 100_000;
                } else if killers[1] == Some(mv) {
                    priority += 50_000;
                }

                (mv, priority)
            })
            .collect();

        scored.sort_by(|a, b| b.1.cmp(&a.1));
        scored.into_iter().map(|(d, _)| d).collect()
    }

    #[inline]
    fn elapsed_ms(&self) -> u64 {
        self.start_time.elapsed().as_millis() as u64
    }

    #[inline]
    fn timed_out(&self) -> bool {
        if self.nodes & 1023 == 0 {
            self.elapsed_ms() >= self.time_budget_ms
        } else {
            false
        }
    }

    fn is_noisy(&self, state: &SimState) -> bool {
        let my_idx = state.my_idx as usize;
        let my = &state.snakes[my_idx];
        if !my.alive {
            return false;
        }
        let my_head = my.head();
        let my_hx = (my_head % state.width) as i32;
        let my_hy = (my_head / state.width) as i32;

        for i in 0..state.num_snakes as usize {
            if i == my_idx || !state.snakes[i].alive {
                continue;
            }
            let eh = state.snakes[i].head();
            let ex = (eh % state.width) as i32;
            let ey = (eh / state.width) as i32;
            let dist = (my_hx - ex).abs() + (my_hy - ey).abs();
            if dist <= 2 {
                return true;
            }
        }

        let head_bb = 1u128 << my_head;
        let expanded = crate::bitboard::Bitboard(head_bb).expand(&state.masks);
        if expanded.0 & state.food != 0 {
            return true;
        }

        let valid = state.get_valid_moves(my_idx);
        let move_count = valid.iter().filter(|m| m.is_some()).count();
        if move_count == 1 {
            return true;
        }

        false
    }
}
