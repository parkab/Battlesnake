use crate::api::ApiGameState;
use crate::bitboard::BoardMasks;
use crate::types::Direction;

pub const MAX_SNAKES: usize = 8;
const BODY_CAP: usize = 128;

#[derive(Clone, Copy)]
pub struct SnakeState {
    pub health: i16,
    pub body: [u8; BODY_CAP],
    pub head_ptr: u8,
    pub len: u8,
    pub alive: bool,
    pub id_hash: u64,
    #[allow(dead_code)]
    pub is_my_snake: bool,
}

impl SnakeState {
    #[inline]
    pub fn head(&self) -> u8 {
        self.body[self.head_ptr as usize]
    }

    #[inline]
    pub fn tail(&self) -> u8 {
        self.body[(self.head_ptr as usize + self.len as usize - 1) & (BODY_CAP - 1)]
    }

    #[inline]
    pub fn penultimate(&self) -> u8 {
        if self.len < 2 {
            return self.tail();
        }
        self.body[(self.head_ptr as usize + self.len as usize - 2) & (BODY_CAP - 1)]
    }

    #[inline]
    pub fn tail_is_stacked(&self) -> bool {
        self.len >= 2 && self.tail() == self.penultimate()
    }

    pub fn current_direction(&self, width: u8) -> Option<Direction> {
        if self.len < 2 {
            return None;
        }
        let head = self.head();
        let neck = self.body[(self.head_ptr as usize + 1) & (BODY_CAP - 1)];
        let hx = (head % width) as i32;
        let hy = (head / width) as i32;
        let nx = (neck % width) as i32;
        let ny = (neck / width) as i32;
        let dx = hx - nx;
        let dy = hy - ny;
        match (dx, dy) {
            (1, 0) => Some(Direction::Right),
            (-1, 0) => Some(Direction::Left),
            (0, 1) => Some(Direction::Up),
            (0, -1) => Some(Direction::Down),
            _ => None,
        }
    }

    #[inline]
    pub fn push_head(&mut self, pos: u8) {
        self.head_ptr = self.head_ptr.wrapping_sub(1) & (BODY_CAP as u8 - 1);
        self.body[self.head_ptr as usize] = pos;
        self.len = self.len.saturating_add(1);
    }

    #[inline]
    pub fn pop_tail(&mut self) -> u8 {
        let old_tail = self.tail();
        self.len -= 1;
        old_tail
    }
}

#[derive(Clone, Copy)]
pub struct SimState {
    pub width: u8,
    pub height: u8,
    pub turn: u16,
    pub food: u128,
    pub hazards: u128,
    pub hazard_damage: i16,
    pub snakes: [SnakeState; MAX_SNAKES],
    pub num_snakes: u8,
    pub my_idx: u8,
    pub all_bodies: u128,
    pub masks: BoardMasks,
}

impl SimState {
    pub fn from_api(gs: &ApiGameState) -> Self {
        let w = gs.board.width as u8;
        let h = gs.board.height as u8;
        let masks = if (w as u32 * h as u32) <= 128 {
            BoardMasks::new(w as u32, h as u32)
        } else {
            BoardMasks::new(11, 11)
        };

        let mut state = SimState {
            width: w,
            height: h,
            turn: gs.turn as u16,
            food: 0,
            hazards: 0,
            hazard_damage: gs
                .game
                .ruleset
                .settings
                .as_ref()
                .and_then(|s| s.hazard_damage_per_turn)
                .unwrap_or(14) as i16,
            snakes: [SnakeState {
                health: 0,
                body: [0u8; BODY_CAP],
                head_ptr: 0,
                len: 0,
                alive: false,
                id_hash: 0,
                is_my_snake: false,
            }; MAX_SNAKES],
            num_snakes: 0,
            my_idx: 0,
            all_bodies: 0,
            masks,
        };

        let mut idx = 0usize;
        for api_snake in &gs.board.snakes {
            if idx >= MAX_SNAKES {
                break;
            }
            let is_me = api_snake.id == gs.you.id;
            let mut snake = SnakeState {
                health: api_snake.health as i16,
                body: [0u8; BODY_CAP],
                head_ptr: 0,
                len: 0,
                alive: true,
                id_hash: fnv_hash(&api_snake.id),
                is_my_snake: is_me,
            };

            let blen = api_snake.body.len().min(BODY_CAP);
            for (i, seg) in api_snake.body.iter().take(blen).enumerate() {
                let pos = (seg.y as u8) * w + (seg.x as u8);
                snake.body[i] = pos;
                let is_tail = i == blen - 1;
                let stacked = blen >= 2
                    && api_snake.body[blen - 1].x == api_snake.body[blen - 2].x
                    && api_snake.body[blen - 1].y == api_snake.body[blen - 2].y;
                if !is_tail || stacked {
                    state.all_bodies |= 1u128 << pos;
                }
            }
            snake.head_ptr = 0;
            snake.len = blen as u8;

            if is_me {
                state.my_idx = idx as u8;
            }
            state.snakes[idx] = snake;
            idx += 1;
        }
        state.num_snakes = idx as u8;

        for f in &gs.board.food {
            let pos = (f.y as u8) * w + (f.x as u8);
            state.food |= 1u128 << pos;
        }

        for h in &gs.board.hazards {
            let pos = (h.y as u8) * w + (h.x as u8);
            state.hazards |= 1u128 << pos;
        }

        state
    }

    #[inline]
    #[allow(dead_code)]
    pub fn my_snake(&self) -> &SnakeState {
        &self.snakes[self.my_idx as usize]
    }

    pub fn moving_tails_bb(&self) -> u128 {
        let mut bb = 0u128;
        for i in 0..self.num_snakes as usize {
            let s = &self.snakes[i];
            if s.alive && !s.tail_is_stacked() {
                bb |= 1u128 << s.tail();
            }
        }
        bb
    }

    pub fn get_valid_moves(&self, snake_idx: usize) -> [Option<Direction>; 4] {
        let s = &self.snakes[snake_idx];
        if !s.alive {
            return [None; 4];
        }
        let head = s.head();
        let hx = (head % self.width) as i32;
        let hy = (head / self.width) as i32;
        let w = self.width as i32;
        let h = self.height as i32;
        let moving_tails = self.moving_tails_bb();
        let safe = !self.all_bodies | moving_tails;

        let mut result = [None; 4];
        for (i, &dir) in Direction::ALL.iter().enumerate() {
            let nx = hx + dir.dx();
            let ny = hy + dir.dy();
            if nx < 0 || nx >= w || ny < 0 || ny >= h {
                continue;
            }
            let npos = (ny as u8) * self.width + (nx as u8);
            if safe & (1u128 << npos) != 0 {
                result[i] = Some(dir);
            }
        }
        result
    }

    pub fn get_weighted_enemy_moves_with_profile(
        &self,
        snake_idx: usize,
        profile: Option<&crate::opponent::OpponentProfile>,
    ) -> Vec<Direction> {
        let s = &self.snakes[snake_idx];
        if !s.alive {
            return vec![];
        }
        let my = &self.snakes[self.my_idx as usize];
        let my_head = my.head();
        let my_hx = (my_head % self.width) as i32;
        let my_hy = (my_head / self.width) as i32;

        let valid = self.get_valid_moves(snake_idx);
        let head = s.head();
        let hx = (head % self.width) as i32;
        let hy = (head / self.width) as i32;

        let food_bias = profile.map_or(0.5, |p| p.food_seeking);
        let aggression_bias = profile.map_or(0.3, |p| p.aggression);
        let risk_aversion = profile.map_or(0.5, |p| p.risk_aversion);

        let mut scored: Vec<(Direction, f32)> = valid
            .iter()
            .filter_map(|&opt| opt)
            .map(|dir| {
                let nx = hx + dir.dx();
                let ny = hy + dir.dy();
                let npos = (ny as u8) * self.width + (nx as u8);
                let mut weight = 1.0f32;

                if s.health < 50 || food_bias > 0.6 {
                    let mut min_food_dist = i32::MAX;
                    let mut food_bb = self.food;
                    while food_bb != 0 {
                        let bit = food_bb.trailing_zeros();
                        let fx = (bit % self.width as u32) as i32;
                        let fy = (bit / self.width as u32) as i32;
                        let d = (nx - fx).abs() + (ny - fy).abs();
                        if d < min_food_dist {
                            min_food_dist = d;
                        }
                        food_bb &= food_bb - 1;
                    }
                    if min_food_dist != i32::MAX {
                        let max_d = self.width as f32 + self.height as f32;
                        weight += (1.0 - min_food_dist as f32 / max_d) * 2.0 * food_bias;
                    }
                }

                if self.food & (1u128 << npos) != 0 {
                    weight += 3.0 * food_bias;
                }

                let dist_to_us = (nx - my_hx).abs() + (ny - my_hy).abs();
                if s.len > my.len && dist_to_us <= 2 {
                    weight += aggression_bias * 2.0;
                } else if dist_to_us <= 1 && my.len > s.len {
                    weight *= 0.2 * (1.0 + (1.0 - aggression_bias));
                }

                if risk_aversion > 0.4 {
                    let moving_tails = self.moving_tails_bb();
                    let safe = !self.all_bodies | moving_tails;
                    let mut open_neighbors = 0;
                    for &d2 in &crate::types::Direction::ALL {
                        let nnx = nx + d2.dx();
                        let nny = ny + d2.dy();
                        if nnx >= 0 && nnx < self.width as i32 && nny >= 0 && nny < self.height as i32 {
                            let nnpos = (nny as u8) * self.width + (nnx as u8);
                            if safe & (1u128 << nnpos) != 0 {
                                open_neighbors += 1;
                            }
                        }
                    }
                    weight += open_neighbors as f32 * 0.3 * risk_aversion;
                }

                let cx = (self.width as f32 - 1.0) / 2.0;
                let cy = (self.height as f32 - 1.0) / 2.0;
                let dist_to_center = (nx as f32 - cx).abs() + (ny as f32 - cy).abs();
                let max_center = cx + cy;
                weight += (1.0 - dist_to_center / max_center) * 0.5;

                if let Some(cur_dir) = s.current_direction(self.width) {
                    if dir == cur_dir {
                        weight += 0.8;
                    }
                }

                (dir, weight)
            })
            .collect();

        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.into_iter().map(|(d, _)| d).collect()
    }

    pub fn advance(&self, moves: &[Option<Direction>; MAX_SNAKES]) -> SimState {
        let mut next = *self;
        next.turn += 1;

        let w = self.width as i32;
        let h = self.height as i32;

        let mut new_heads = [0u8; MAX_SNAKES];
        let mut went_oob = [false; MAX_SNAKES];

        for i in 0..self.num_snakes as usize {
            let s = &self.snakes[i];
            if !s.alive {
                continue;
            }
            let dir = match moves[i] {
                Some(d) => d,
                None => continue,
            };
            let head = s.head();
            let hx = (head % self.width) as i32;
            let hy = (head / self.width) as i32;
            let nx = hx + dir.dx();
            let ny = hy + dir.dy();

            if nx < 0 || nx >= w || ny < 0 || ny >= h {
                went_oob[i] = true;
            } else {
                new_heads[i] = (ny as u8) * self.width + (nx as u8);
            }
        }

        let mut eats_food = [false; MAX_SNAKES];
        for i in 0..self.num_snakes as usize {
            if !self.snakes[i].alive || went_oob[i] {
                continue;
            }
            if self.food & (1u128 << new_heads[i]) != 0 {
                eats_food[i] = true;
            }
        }

        let mut new_all_bodies = self.all_bodies;
        let moving_tails = self.moving_tails_bb();
        new_all_bodies &= !moving_tails;

        for i in 0..self.num_snakes as usize {
            let s = &self.snakes[i];
            if !s.alive || went_oob[i] {
                continue;
            }
            new_all_bodies |= 1u128 << new_heads[i];
            if eats_food[i] {
                new_all_bodies |= 1u128 << s.tail();
            }
        }

        let mut food_eaten = 0u128;

        for i in 0..self.num_snakes as usize {
            if !next.snakes[i].alive {
                continue;
            }
            if went_oob[i] {
                continue;
            }
            let ate = eats_food[i];
            let new_head = new_heads[i];

            let s = &mut next.snakes[i];
            s.push_head(new_head);
            s.health -= 1;

            if ate {
                s.health = 100;
                food_eaten |= 1u128 << new_head;
            } else {
                s.pop_tail();
            }
        }

        next.food &= !food_eaten;
        next.all_bodies = new_all_bodies;

        for i in 0..self.num_snakes as usize {
            let s = &mut next.snakes[i];
            if !s.alive {
                continue;
            }
            let head = s.head();
            if self.hazards & (1u128 << head) != 0 {
                s.health -= next.hazard_damage;
            }
        }

        let mut eliminate = [false; MAX_SNAKES];

        for i in 0..next.num_snakes as usize {
            let s = &next.snakes[i];
            if !s.alive {
                continue;
            }
            if went_oob[i] || s.health <= 0 {
                eliminate[i] = true;
                continue;
            }
            let head = s.head();
            let head_bit = 1u128 << head;
            let others_body = self.all_bodies & !moving_tails;
            if others_body & head_bit != 0 {
                let mut in_body = false;
                for j in 0..self.num_snakes as usize {
                    let sj = &self.snakes[j];
                    if !sj.alive {
                        continue;
                    }
                    for k in 0..sj.len as usize {
                        let seg_idx = (sj.head_ptr as usize + k) & (BODY_CAP - 1);
                        let seg = sj.body[seg_idx];
                        if k == sj.len as usize - 1 && !sj.tail_is_stacked() {
                            continue;
                        }
                        if seg == head && !(j == i && k == 0) {
                            in_body = true;
                            break;
                        }
                    }
                    if in_body {
                        break;
                    }
                }
                if in_body {
                    eliminate[i] = true;
                }
            }
        }

        for i in 0..next.num_snakes as usize {
            for j in (i + 1)..next.num_snakes as usize {
                if !next.snakes[i].alive || !next.snakes[j].alive {
                    continue;
                }
                if eliminate[i] || eliminate[j] {
                    continue;
                }
                if next.snakes[i].head() == next.snakes[j].head() {
                    if next.snakes[i].len > next.snakes[j].len {
                        eliminate[j] = true;
                    } else if next.snakes[j].len > next.snakes[i].len {
                        eliminate[i] = true;
                    } else {
                        eliminate[i] = true;
                        eliminate[j] = true;
                    }
                }
            }
        }

        for i in 0..next.num_snakes as usize {
            if eliminate[i] {
                let s = &next.snakes[i];
                for k in 0..s.len as usize {
                    let seg_idx = (s.head_ptr as usize + k) & (BODY_CAP - 1);
                    let seg = s.body[seg_idx];
                    next.all_bodies &= !(1u128 << seg);
                }
                next.snakes[i].alive = false;
            }
        }

        next
    }

    #[inline]
    pub fn is_alive(&self, snake_idx: usize) -> bool {
        snake_idx < self.num_snakes as usize && self.snakes[snake_idx].alive
    }

    #[inline]
    pub fn is_game_over(&self) -> bool {
        let alive = (0..self.num_snakes as usize)
            .filter(|&i| self.snakes[i].alive)
            .count();
        alive <= 1
    }

    #[inline]
    pub fn alive_enemy_count(&self) -> usize {
        (0..self.num_snakes as usize)
            .filter(|&i| i != self.my_idx as usize && self.snakes[i].alive)
            .count()
    }
}

pub fn fnv_hash(s: &str) -> u64 {
    let mut hash: u64 = 14695981039346656037;
    for byte in s.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(1099511628211);
    }
    hash
}
