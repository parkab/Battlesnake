use std::time::Instant;
use rand::Rng;
use crate::api::{ApiGameState, MoveResponse};
use crate::board::Board;
use crate::minimax::{MinimaxEngine, TranspositionTable};
use crate::opponent::GameTracker;
use crate::simulator::SimState;
use crate::types::Direction;

struct CandidateMove {
    dir: Direction,
    x: i32,
    y: i32,
    score: f32,
    minimax_score: Option<f32>,
    minimax_depth: i32,
    space: i32,
    exit_count: i32,
}

#[derive(Debug, PartialEq, Clone, Copy)]
enum AggressionMode {
    Survival,
    Pressure,
    Kill,
    CutOff,
}

pub fn decide_move(gs: &ApiGameState, tracker: Option<&GameTracker>, tt: &mut TranspositionTable) -> MoveResponse {
    let start = Instant::now();
    let timeout = gs.game.timeout as u64;
    let compute_budget_ms = timeout.saturating_sub(120).max(80);

    let board = Board::new(gs);
    let head = board.my_head;

    let mut candidates: Vec<CandidateMove> = Direction::ALL
        .iter()
        .filter_map(|&dir| {
            let nx = head.x + dir.dx();
            let ny = head.y + dir.dy();
            if board.is_in_bounds(nx, ny) && board.is_safe(nx, ny) {
                Some(CandidateMove { dir, x: nx, y: ny, score: 0.0, minimax_score: None, minimax_depth: 0, space: 0, exit_count: 0 })
            } else {
                None
            }
        })
        .collect();

    if candidates.is_empty() {
        candidates = Direction::ALL
            .iter()
            .filter_map(|&dir| {
                let nx = head.x + dir.dx();
                let ny = head.y + dir.dy();
                if !board.is_in_bounds(nx, ny) {
                    return None;
                }
                let score = if board.is_tail_freeing(nx, ny) { -2.0 } else { -10.0 };
                Some(CandidateMove { dir, x: nx, y: ny, score, minimax_score: None, minimax_depth: 0, space: 0, exit_count: 0 })
            })
            .collect();
        candidates.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        if candidates.is_empty() {
            return MoveResponse { direction: "up".to_string(), shout: "GG".to_string() };
        }
        candidates.truncate(1);
    }

    if candidates.len() == 1 {
        return MoveResponse {
            direction: candidates[0].dir.to_str().to_string(),
            shout: "Only one way!".to_string(),
        };
    }

    let my_len = board.my_length;
    let my_health = board.my_health;
    let turn = board.turn;
    let enemies = &board.enemy_heads;
    let max_enemy_len = enemies.iter().map(|e| e.length).max().unwrap_or(0);

    let (my_voronoi, total_voronoi) = board.voronoi_area();
    let enemy_voronoi = total_voronoi - my_voronoi;

    let trap_status = board.enemy_trap_status();
    let any_enemy_trapped = trap_status.iter().any(|(_, trapped, _)| *trapped);

    let aggression_mode = determine_aggression_mode(
        my_len, max_enemy_len, my_health, my_voronoi, enemy_voronoi,
        any_enemy_trapped, enemies.len(), tracker,
    );

    for cand in &mut candidates {
        let ff = board.flood_fill_enhanced(cand.x, cand.y);
        cand.space = ff.count;
        cand.exit_count = ff.exit_count;

        let (my_vor, total_vor) = board.voronoi_area_projected(cand.x, cand.y, 0);

        if ff.count > my_len {
            let contested_ratio = my_vor as f32 / total_vor.max(1) as f32;
            if contested_ratio < 0.35 {
                cand.score -= (1.0 - contested_ratio) * 10.0;
            } else if contested_ratio < 0.5 {
                cand.score -= (0.5 - contested_ratio) * 8.0;
            }
        }

        if my_voronoi > 0 {
            let territory_shrink = (my_voronoi - my_vor) as f32 / my_voronoi.max(1) as f32;
            if territory_shrink > 0.5 {
                cand.score -= territory_shrink * 8.0;
            } else if territory_shrink > 0.3 {
                cand.score -= territory_shrink * 4.0;
            }
        }
        if my_vor < my_len && ff.count > my_len * 2 {
            cand.score -= 5.0;
        }

        let space_ratio = ff.count as f32 / my_len.max(1) as f32;
        let space_mult = if enemies.len() >= 3 { 1.5 } else { 1.0 };
        if space_ratio < 1.1 {
            cand.score -= 10.0 * space_mult;
        } else if space_ratio < 1.5 {
            cand.score -= 7.0 * space_mult;
        } else if space_ratio < 2.0 {
            cand.score -= 3.0 * space_mult;
        } else if space_ratio < 3.0 {
            cand.score -= 0.5;
        } else {
            let board_size = (board.width * board.height) as f32;
            let space_bonus = if enemies.len() >= 3 { 3.5 } else { 2.0 };
            cand.score += (ff.count as f32 / board_size) * space_bonus;
        }

        let exit_mult = if enemies.len() >= 3 { 1.5 } else { 1.0 };
        if ff.exit_count == 0 {
            cand.score -= 5.0 * exit_mult;
        } else if ff.exit_count <= 1 {
            cand.score -= 3.0 * exit_mult;
        } else if ff.exit_count <= 2 {
            cand.score -= 0.5 * exit_mult;
        }
        if enemies.len() >= 3 && ff.exit_count >= 4 {
            cand.score += 1.0;
        }

        if ff.has_tail_exit {
            cand.score += 1.5;
        }

        if board.is_in_danger_zone(cand.x, cand.y) {
            cand.score -= 8.0;
        }

        for eh in enemies {
            let dist = board.manhattan(cand.x, cand.y, eh.pos.x, eh.pos.y);
            if dist <= 1 {
                if eh.length > my_len {
                    cand.score -= 50.0;
                } else if eh.length == my_len {
                    cand.score -= 20.0;
                } else {
                    let safe_to_engage = enemies.iter().all(|other| {
                        other.id == eh.id
                            || other.length < my_len
                            || board.manhattan(cand.x, cand.y, other.pos.x, other.pos.y) > 2
                    });
                    if safe_to_engage {
                        let kill_bonus = match aggression_mode {
                            AggressionMode::Kill => 8.0,
                            AggressionMode::Pressure => 5.0,
                            _ => 3.0,
                        };
                        cand.score += kill_bonus;
                    } else {
                        cand.score -= 5.0;
                    }
                }
            } else if dist == 2 {
                if eh.length > my_len {
                    cand.score -= 5.0;
                } else if eh.length == my_len {
                    cand.score -= 1.5;
                } else if aggression_mode == AggressionMode::Kill || aggression_mode == AggressionMode::Pressure {
                    cand.score += 1.0;
                }
            }
        }

        {
            let mut adjacent_body_cells = 0;
            for &dir in &Direction::ALL {
                let ax = cand.x + dir.dx();
                let ay = cand.y + dir.dy();
                if board.is_in_bounds(ax, ay) && !board.is_safe(ax, ay)
                    && !board.is_tail_freeing(ax, ay)
                    && !(ax == head.x && ay == head.y)
                {
                    adjacent_body_cells += 1;
                }
            }
            if adjacent_body_cells >= 3 {
                cand.score -= 4.0;
            } else if adjacent_body_cells >= 2 {
                cand.score -= 1.5;
            }
        }

        if board.is_enemy_reachable(cand.x, cand.y) && !board.is_in_danger_zone(cand.x, cand.y) {
            if enemies.len() >= 2 {
                cand.score -= 1.0;
            }
        }

        if board.is_corridor_entrance(cand.x, cand.y) {
            cand.score -= 3.0;
        }

        if board.is_food(cand.x, cand.y) {
            if board.is_food_trapped(cand.x, cand.y) {
                cand.score += if my_health < 10 { 1.0 } else { -2.0 };
            } else {
                cand.score += food_urgency_score(my_health, my_len, max_enemy_len, &aggression_mode);
            }
        }

        if board.is_hazard(cand.x, cand.y) {
            cand.score -= 2.0;
        }

        if ff.food_count > 0 && my_health < 70 {
            let food_in_region_bonus = (ff.food_count as f32).min(3.0) * 0.5;
            cand.score += food_in_region_bonus;
        }
        if let Some(safe_food) = board.nearest_safe_food(cand.x, cand.y) {
            let base_weight = if my_health < 20 {
                3.0
            } else if my_health < 40 {
                1.5
            } else if my_health < 70 {
                0.5
            } else {
                0.2
            };
            let len_diff = my_len - max_enemy_len;
            let len_factor = if my_health < 30 {
                1.0
            } else if len_diff > 6 {
                0.05
            } else if len_diff > 3 {
                0.2
            } else {
                1.0
            };
            let food_weight = base_weight * len_factor
                * if safe_food.penalty > 0 { 0.3 } else { 1.0 };

            let max_d = (board.width + board.height) as f32;
            cand.score += (1.0 - safe_food.dist as f32 / max_d) * food_weight;
        }

        let tail = board.my_tail;
        let tail_dist = board.manhattan(cand.x, cand.y, tail.x, tail.y);
        let max_d = (board.width + board.height) as f32;
        let mut tail_bonus = (1.0 - tail_dist as f32 / max_d) * 0.5;

        for f in &board.food {
            let d_to_food = board.manhattan(cand.x, cand.y, f.x, f.y);
            let d_food_to_tail = board.manhattan(f.x, f.y, tail.x, tail.y);
            if d_to_food + d_food_to_tail <= tail_dist + 2 {
                tail_bonus *= 0.3;
                break;
            }
        }
        if aggression_mode == AggressionMode::Survival {
            tail_bonus *= 1.5;
        }
        cand.score += tail_bonus;

        {
            let cx = (board.width - 1) as f32 / 2.0;
            let cy = (board.height - 1) as f32 / 2.0;
            let dist = (cand.x as f32 - cx).abs() + (cand.y as f32 - cy).abs();
            let max_center_dist = cx + cy;
            let center_proximity = 1.0 - dist / max_center_dist;

            let center_weight = if enemies.len() <= 1 {
                if turn < 15 { 2.5 } else { 2.0 }
            } else if enemies.len() == 2 {
                if turn < 15 { 1.2 } else { 0.6 }
            } else {
                if turn < 15 { 0.4 } else { 0.15 }
            };
            let center_weight = if board.ruleset == "royale" { center_weight * 1.5 } else { center_weight };

            let mirror_boost = enemies.len() <= 2 && enemies.iter().any(|eh| {
                board.is_enemy_mirroring(eh.pos.x, eh.pos.y)
                    && board.manhattan(eh.pos.x, eh.pos.y, cx as i32, cy as i32)
                        < board.manhattan(head.x, head.y, cx as i32, cy as i32)
            });
            let center_weight = if mirror_boost { center_weight * 1.3 } else { center_weight };

            cand.score += center_proximity * center_weight;
        }

        if enemies.len() >= 2 {
            let mut nearby_enemies = 0;
            let mut nearby_bigger = 0;
            let mut direction_sectors = [false; 4];
            for eh in enemies {
                let dist = board.manhattan(cand.x, cand.y, eh.pos.x, eh.pos.y);
                if dist <= 4 {
                    nearby_enemies += 1;
                    if eh.length >= my_len {
                        nearby_bigger += 1;
                    }
                    let dx = eh.pos.x - cand.x;
                    let dy = eh.pos.y - cand.y;
                    if dx >= 0 && dy >= 0 { direction_sectors[0] = true; }
                    if dx < 0 && dy >= 0 { direction_sectors[1] = true; }
                    if dx >= 0 && dy < 0 { direction_sectors[2] = true; }
                    if dx < 0 && dy < 0 { direction_sectors[3] = true; }
                }
            }
            let occupied_sectors = direction_sectors.iter().filter(|&&s| s).count();

            if occupied_sectors >= 3 {
                cand.score -= 4.0;
                if nearby_bigger >= 2 {
                    cand.score -= 3.0;
                }
            } else if occupied_sectors >= 2 && nearby_enemies >= 3 {
                cand.score -= 2.5;
            } else if nearby_enemies >= 2 && nearby_bigger >= 1 {
                cand.score -= 1.0;
            }

            if nearby_enemies >= 2 {
                let mut avg_ex = 0.0f32;
                let mut avg_ey = 0.0f32;
                let mut count = 0.0f32;
                for eh in enemies {
                    let dist = board.manhattan(cand.x, cand.y, eh.pos.x, eh.pos.y);
                    if dist <= 5 {
                        avg_ex += eh.pos.x as f32;
                        avg_ey += eh.pos.y as f32;
                        count += 1.0;
                    }
                }
                if count > 0.0 {
                    avg_ex /= count;
                    avg_ey /= count;
                    let dist_to_cluster = (cand.x as f32 - avg_ex).abs() + (cand.y as f32 - avg_ey).abs();
                    let dist_from_head_to_cluster = (head.x as f32 - avg_ex).abs() + (head.y as f32 - avg_ey).abs();
                    if dist_to_cluster > dist_from_head_to_cluster {
                        cand.score += 1.5;
                    } else if dist_to_cluster < dist_from_head_to_cluster {
                        cand.score -= 0.5;
                    }
                }
            }
        }

        {
            let edge_x = cand.x.min(board.width - 1 - cand.x);
            let edge_y = cand.y.min(board.height - 1 - cand.y);
            let min_edge = edge_x.min(edge_y);
            let wall_mult = if enemies.len() <= 1 { 2.0 } else if enemies.len() >= 3 { 0.5 } else { 1.0 };
            if min_edge == 0 {
                cand.score -= 1.5 * wall_mult;
            } else if min_edge == 1 {
                cand.score -= 0.8 * wall_mult;
            }
        }

        for eh in enemies {
            if board.is_enemy_mirroring(eh.pos.x, eh.pos.y) {
                let dx = (cand.x - head.x).signum();
                let dy = (cand.y - head.y).signum();
                let to_enemy_x = (eh.pos.x - head.x).signum();
                let to_enemy_y = (eh.pos.y - head.y).signum();
                if (dx != 0 && to_enemy_y != 0 && dx == to_enemy_y.signum())
                    || (dy != 0 && to_enemy_x != 0 && dy == to_enemy_x.signum())
                {
                    cand.score += 0.8;
                }
            }
        }

        if aggression_mode == AggressionMode::Kill
            || aggression_mode == AggressionMode::CutOff
            || aggression_mode == AggressionMode::Pressure
        {
            let cx = (board.width - 1) as f32 / 2.0;
            let cy = (board.height - 1) as f32 / 2.0;
            for eh in enemies {
                if eh.length < my_len {
                    let our_center_dist = (cand.x as f32 - cx).abs() + (cand.y as f32 - cy).abs();
                    let enemy_center_dist = (eh.pos.x as f32 - cx).abs() + (eh.pos.y as f32 - cy).abs();
                    if our_center_dist < enemy_center_dist {
                        let cut_bonus = match aggression_mode {
                            AggressionMode::Kill => 2.0,
                            AggressionMode::CutOff => 1.5,
                            AggressionMode::Pressure => 0.8,
                            _ => 0.0,
                        };
                        cand.score += cut_bonus;
                    }
                }
            }
        }

        if total_vor > 0 {
            let territory_ratio = my_vor as f32 / total_vor as f32;
            let fair_share = 1.0 / (enemies.len() as f32 + 1.0);
            let territory_bonus = (territory_ratio - fair_share) * 5.0;
            cand.score += territory_bonus;
        }

        for (ref eid, trapped, region_size) in &trap_status {
            if *trapped {
                if let Some(eh) = enemies.iter().find(|e| &e.id == eid) {
                    let dist = board.manhattan(cand.x, cand.y, eh.pos.x, eh.pos.y);
                    if dist <= 3 {
                        cand.score += 3.0;
                    }
                }
            } else if *region_size < 15 {
                if let Some(eh) = enemies.iter().find(|e| &e.id == eid) {
                    if eh.length < my_len {
                        let dist = board.manhattan(cand.x, cand.y, eh.pos.x, eh.pos.y);
                        if dist <= 2 {
                            cand.score += 1.5;
                        }
                    }
                }
            }
        }

        if tracker.map_or(false, |t| t.is_territory_collapsing()) {
            let (my_area, total_area) = board.voronoi_area_from(cand.x, cand.y);
            if total_area > 0 {
                cand.score += (my_area as f32 / total_area as f32) * 3.0;
            }
        }
    }

    let heuristic_elapsed = start.elapsed().as_millis() as u64;
    let minimax_budget = compute_budget_ms.saturating_sub(heuristic_elapsed).saturating_sub(20);

    if minimax_budget > 40 {
        let sim_state = SimState::from_api(gs);
        let mut engine = MinimaxEngine::new(minimax_budget, tt);
        if let Some(trk) = tracker {
            engine.set_profiles(&trk.profiles, &sim_state);
        }
        let result = engine.search(&sim_state);

        let depth_confidence = (result.depth as f32 / 8.0).min(1.0);
        let minimax_weight = if enemies.len() <= 1 { 5.0 } else { 3.0 } * depth_confidence;

        for cand in &mut candidates {
            if let Some(&(_, mm_score)) = result.move_scores.iter().find(|(d, _)| *d == cand.dir) {
                cand.minimax_score = Some(mm_score);
                cand.minimax_depth = result.depth;
                cand.score += mm_score * minimax_weight;
            }
        }
    }

    let phase = compute_phase_weights(turn, my_len, my_health, enemies.len());

    for cand in &mut candidates {
        let mut phase_score = 0.0f32;

        if phase.opening > 0.0 {
            phase_score += score_opening(cand.x, cand.y, &board) * phase.opening;
        }
        if phase.midgame > 0.0 {
            phase_score += score_midgame(cand.x, cand.y, &board, my_len, &aggression_mode) * phase.midgame;
        }
        if phase.endgame > 0.0 {
            phase_score += score_endgame(cand.x, cand.y, &board, my_len) * phase.endgame;
        }
        if phase.duel > 0.0 {
            phase_score += score_duel(cand.x, cand.y, &board, my_len, my_health, &aggression_mode) * phase.duel;
        }

        cand.score += phase_score;
    }

    {
        let mut rng = rand::thread_rng();
        for cand in &mut candidates {
            cand.score += rng.gen_range(-0.01..0.01);
        }
    }

    candidates.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    let best = &candidates[0];

    let remaining_ms = compute_budget_ms.saturating_sub(start.elapsed().as_millis() as u64);
    let direction = if candidates.len() >= 2 {
        let second = &candidates[1];
        if (best.score - second.score).abs() < 0.05 && remaining_ms > 30 {
            let sim_state = SimState::from_api(gs);
            let rollout_budget = remaining_ms.saturating_sub(10);
            let rollout_start = Instant::now();
            let num_rollouts = 50;
            let mut wins_a = 0u32;
            let mut wins_b = 0u32;
            let dir_a = candidates[0].dir;
            let dir_b = candidates[1].dir;

            for i in 0..num_rollouts * 2 {
                if rollout_start.elapsed().as_millis() as u64 >= rollout_budget {
                    break;
                }
                let test_dir = if i % 2 == 0 { dir_a } else { dir_b };
                if rollout_survives(&sim_state, test_dir, 30) {
                    if i % 2 == 0 { wins_a += 1; } else { wins_b += 1; }
                }
            }

            if wins_a == wins_b {
                if candidates[0].exit_count >= candidates[1].exit_count {
                    dir_a
                } else {
                    dir_b
                }
            } else if wins_a >= wins_b { dir_a } else { dir_b }
        } else {
            best.dir
        }
    } else {
        best.dir
    };

    let shout = make_shout(&phase, best, my_health, my_len, &aggression_mode);

    MoveResponse { direction: direction.to_str().to_string(), shout }
}

fn determine_aggression_mode(
    my_len: i32,
    max_enemy_len: i32,
    my_health: i32,
    my_voronoi: i32,
    enemy_voronoi: i32,
    any_enemy_trapped: bool,
    num_enemies: usize,
    tracker: Option<&GameTracker>,
) -> AggressionMode {
    if my_health < 20 {
        return AggressionMode::Survival;
    }

    if any_enemy_trapped && my_len >= max_enemy_len {
        return AggressionMode::Kill;
    }

    if num_enemies >= 3 {
        if any_enemy_trapped && my_len > max_enemy_len + 2 {
            return AggressionMode::Kill;
        }
        return AggressionMode::Survival;
    }

    if my_len > max_enemy_len + 2 && my_voronoi > enemy_voronoi {
        return AggressionMode::Kill;
    }

    if tracker.map_or(false, |t| t.is_territory_collapsing()) && my_len >= max_enemy_len {
        return AggressionMode::CutOff;
    }

    if my_len > max_enemy_len && my_voronoi >= enemy_voronoi / 2 {
        return AggressionMode::Pressure;
    }

    if my_len < max_enemy_len {
        return AggressionMode::Survival;
    }

    AggressionMode::Pressure
}

fn random_move(sim: &SimState, snake_idx: usize, rng: &mut impl rand::Rng) -> Direction {
    let options: Vec<Direction> = sim.get_valid_moves(snake_idx)
        .iter()
        .filter_map(|&m| m)
        .collect();
    if options.is_empty() {
        Direction::Up
    } else {
        options[rng.gen_range(0..options.len())]
    }
}

fn rollout_survives(state: &SimState, first_move: Direction, max_turns: i32) -> bool {
    use crate::simulator::MAX_SNAKES;
    let mut rng = rand::thread_rng();
    let mut sim = *state;

    let mut moves = [None; MAX_SNAKES];
    moves[sim.my_idx as usize] = Some(first_move);
    for i in 0..sim.num_snakes as usize {
        if i != sim.my_idx as usize && sim.snakes[i].alive {
            moves[i] = Some(random_move(&sim, i, &mut rng));
        }
    }
    sim = sim.advance(&moves);

    if !sim.is_alive(sim.my_idx as usize) {
        return false;
    }

    for _ in 1..max_turns {
        if sim.is_game_over() {
            return sim.is_alive(sim.my_idx as usize);
        }
        let mut moves = [None; MAX_SNAKES];
        for i in 0..sim.num_snakes as usize {
            if sim.snakes[i].alive {
                moves[i] = Some(random_move(&sim, i, &mut rng));
            }
        }
        sim = sim.advance(&moves);
        if !sim.is_alive(sim.my_idx as usize) {
            return false;
        }
    }
    true
}

fn food_urgency_score(health: i32, my_len: i32, max_enemy_len: i32, mode: &AggressionMode) -> f32 {
    if health < 15 {
        return 5.0;
    }
    if health < 30 {
        return 3.5;
    }
    if health < 50 {
        return 2.0;
    }
    let len_diff = my_len - max_enemy_len;

    if *mode == AggressionMode::Kill && len_diff > 2 {
        return -0.5;
    }

    if len_diff > 6 {
        -0.5
    } else if len_diff > 3 {
        0.0
    } else if my_len <= max_enemy_len {
        2.0
    } else {
        0.3
    }
}

struct PhaseWeights {
    opening: f32,
    midgame: f32,
    endgame: f32,
    duel: f32,
}

fn compute_phase_weights(
    turn: i32,
    _my_len: i32,
    _health: i32,
    num_enemies: usize,
) -> PhaseWeights {
    if num_enemies == 0 {
        return PhaseWeights { opening: 0.0, midgame: 0.0, endgame: 1.0, duel: 0.0 };
    }
    if num_enemies == 1 {
        return PhaseWeights { opening: 0.0, midgame: 0.2, endgame: 0.0, duel: 0.8 };
    }

    if num_enemies >= 3 {
        if turn < 15 {
            return PhaseWeights { opening: 0.8, midgame: 0.2, endgame: 0.0, duel: 0.0 };
        }
        return PhaseWeights { opening: 0.0, midgame: 1.0, endgame: 0.0, duel: 0.0 };
    }

    if num_enemies == 2 {
        if turn < 15 {
            let t = turn as f32 / 15.0;
            return PhaseWeights { opening: 1.0 - t, midgame: t, endgame: 0.0, duel: 0.0 };
        }
        if turn > 60 {
            let t = ((turn - 60) as f32 / 30.0).min(1.0);
            return PhaseWeights { opening: 0.0, midgame: 1.0 - t * 0.5, endgame: t * 0.5, duel: 0.0 };
        }
        return PhaseWeights { opening: 0.0, midgame: 1.0, endgame: 0.0, duel: 0.0 };
    }

    if turn < 15 {
        PhaseWeights { opening: 1.0, midgame: 0.0, endgame: 0.0, duel: 0.0 }
    } else if turn < 30 {
        let t = (turn - 15) as f32 / 15.0;
        PhaseWeights { opening: 1.0 - t, midgame: t, endgame: 0.0, duel: 0.0 }
    } else {
        PhaseWeights { opening: 0.0, midgame: 1.0, endgame: 0.0, duel: 0.0 }
    }
}

fn score_opening(x: i32, y: i32, board: &Board) -> f32 {
    let mut score = 0.0f32;
    if let Some(f) = board.nearest_safe_food(x, y) {
        let max_d = (board.width + board.height) as f32;
        score += (1.0 - f.dist as f32 / max_d) * 1.5;
    }
    for eh in &board.enemy_heads {
        let dist = board.manhattan(x, y, eh.pos.x, eh.pos.y);
        if dist <= 2 {
            score -= 1.5;
        }
    }
    score
}

fn score_midgame(x: i32, y: i32, board: &Board, my_len: i32, mode: &AggressionMode) -> f32 {
    let mut score = 0.0f32;

    let (my_area, total_area) = board.voronoi_area_from(x, y);
    if total_area > 0 {
        let vor_weight = if board.enemy_heads.len() >= 3 { 2.0 } else { 1.5 };
        score += (my_area as f32 / total_area as f32) * vor_weight;
    }

    {
        let cx = (board.width - 1) as f32 / 2.0;
        let cy = (board.height - 1) as f32 / 2.0;
        let dist = (x as f32 - cx).abs() + (y as f32 - cy).abs();
        let max_dist = cx + cy;
        let center_w = if board.enemy_heads.len() >= 3 { 0.2 } else { 1.0 };
        score += (1.0 - dist / max_dist) * center_w;
    }

    for eh in &board.enemy_heads {
        if eh.length < my_len {
            let dist = board.manhattan(x, y, eh.pos.x, eh.pos.y);
            if dist <= 3 {
                let bigger_nearby = board.enemy_heads.iter().any(|other| {
                    other.id != eh.id
                        && other.length >= my_len
                        && board.manhattan(x, y, other.pos.x, other.pos.y) <= 4
                });
                if !bigger_nearby {
                    let pressure_bonus = match mode {
                        AggressionMode::Kill => 1.2,
                        AggressionMode::Pressure | AggressionMode::CutOff => 0.8,
                        AggressionMode::Survival => 0.2,
                    };
                    score += pressure_bonus;
                }
            }
        }
    }

    score
}

fn score_endgame(x: i32, y: i32, board: &Board, my_len: i32) -> f32 {
    let mut score = 0.0f32;
    let max_d = (board.width + board.height) as f32;

    if board.enemy_heads.is_empty() {
        let tail = board.my_tail;
        let dist = board.manhattan(x, y, tail.x, tail.y);
        score += (1.0 - dist as f32 / max_d) * 2.0;
    } else {
        let cx = (board.width - 1) as f32 / 2.0;
        let cy = (board.height - 1) as f32 / 2.0;
        let our_dist_to_center = (x as f32 - cx).abs() + (y as f32 - cy).abs();
        let max_center_dist = cx + cy;

        for eh in &board.enemy_heads {
            let dist = board.manhattan(x, y, eh.pos.x, eh.pos.y) as f32;
            let enemy_dist_to_center = (eh.pos.x as f32 - cx).abs() + (eh.pos.y as f32 - cy).abs();
            if my_len > eh.length + 1 {
                score += (1.0 - dist / max_d) * 2.5;
                if our_dist_to_center < enemy_dist_to_center {
                    score += 1.2;
                }
            } else if my_len > eh.length {
                score += (1.0 - dist / max_d) * 1.5;
                if our_dist_to_center < enemy_dist_to_center {
                    score += 0.8;
                }
            } else {
                score += (dist / max_d) * 1.0;
            }
        }
        score += (1.0 - our_dist_to_center / max_center_dist) * 1.5;
    }

    score
}

fn score_duel(x: i32, y: i32, board: &Board, my_len: i32, my_health: i32, mode: &AggressionMode) -> f32 {
    let enemy = match board.enemy_heads.first() {
        Some(e) => e,
        None => return 0.0,
    };

    let dist = board.manhattan(x, y, enemy.pos.x, enemy.pos.y) as f32;
    let max_d = (board.width + board.height) as f32;
    let mut score = 0.0f32;

    let cx = (board.width - 1) as f32 / 2.0;
    let cy = (board.height - 1) as f32 / 2.0;
    let our_dist_to_center = (x as f32 - cx).abs() + (y as f32 - cy).abs();
    let max_center_dist = cx + cy;
    let enemy_dist_to_center = (enemy.pos.x as f32 - cx).abs() + (enemy.pos.y as f32 - cy).abs();

    if my_len > enemy.length + 1 {
        let chase_weight = match mode {
            AggressionMode::Kill => 3.5,
            AggressionMode::Pressure | AggressionMode::CutOff => 2.5,
            AggressionMode::Survival => 1.0,
        };
        score += (1.0 - dist / max_d) * chase_weight;
        let enemy_moves = board.get_safe_neighbors(enemy.pos.x, enemy.pos.y).len();
        if enemy_moves <= 2 {
            score += 1.5;
        }
        if enemy_moves <= 1 {
            score += 2.0;
        }
        if our_dist_to_center < enemy_dist_to_center {
            score += 1.5;
        }
    } else if my_len > enemy.length {
        score += (1.0 - dist / max_d) * 1.5;
        if our_dist_to_center < enemy_dist_to_center {
            score += 1.0;
        }
    } else if my_len == enemy.length {
        if dist <= 2.0 {
            score -= 2.0;
        }
        if let Some(f) = board.nearest_safe_food(x, y) {
            score += (1.0 - f.dist as f32 / max_d) * 2.0;
        }
    } else {
        if let Some(f) = board.nearest_safe_food(x, y) {
            score += (1.0 - f.dist as f32 / max_d) * 2.5;
        }
        if my_health < 40 {
            if let Some(f) = board.nearest_food(x, y) {
                score += (1.0 - f.dist as f32 / max_d) * 1.0;
            }
        }
        score += (dist / max_d) * 1.0;
    }

    score += (1.0 - our_dist_to_center / max_center_dist) * 2.0;

    let edge_x = x.min(board.width - 1 - x);
    let edge_y = y.min(board.height - 1 - y);
    let min_edge = edge_x.min(edge_y);
    if min_edge == 0 {
        score -= 1.5;
    } else if min_edge == 1 {
        score -= 0.5;
    }

    score
}

fn make_shout(phase: &PhaseWeights, best: &CandidateMove, health: i32, _length: i32, mode: &AggressionMode) -> String {
    if health < 15 {
        return "Hungry!".to_string();
    }
    if let Some(mm) = best.minimax_score {
        if mm > 0.8 {
            return "Checkmate!".to_string();
        }
        if mm < -0.5 {
            return "Uh oh...".to_string();
        }
    }

    match mode {
        AggressionMode::Kill => return "Going for the kill!".to_string(),
        AggressionMode::CutOff => return "Cutting you off!".to_string(),
        _ => {}
    }

    let dominant = if phase.duel > 0.5 {
        "duel"
    } else if phase.endgame > 0.5 {
        "endgame"
    } else if phase.opening > 0.5 {
        "opening"
    } else {
        "midgame"
    };

    match dominant {
        "opening" => "Growing...".to_string(),
        "midgame" => "Territory secured.".to_string(),
        "endgame" => "Closing in.".to_string(),
        "duel" => "Just us now.".to_string(),
        _ => "Calculated.".to_string(),
    }
}
