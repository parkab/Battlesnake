#![allow(dead_code)]

use crate::api::ApiGameState;
use crate::bitboard::{Bitboard, BoardMasks};
use crate::types::{Coord, Direction};

const CELL_OPEN: u8 = 0;
const CELL_BODY: u8 = 1;
const CELL_FOOD: u8 = 2;
const CELL_HAZARD: u8 = 3;

pub struct EnemyHead {
    pub pos: Coord,
    pub length: i32,
    pub health: i32,
    pub id: String,
}

pub struct SnakeInfo {
    pub id: String,
    pub head: Coord,
    pub tail: Coord,
    pub length: i32,
    pub health: i32,
}

pub struct FloodFillResult {
    pub count: i32,
    pub has_tail_exit: bool,
    pub food_count: i32,
    pub exit_count: i32,
}

pub struct FoodInfo {
    pub food: Coord,
    pub dist: i32,
    pub penalty: i32,
}

pub struct Board {
    pub width: i32,
    pub height: i32,
    pub turn: i32,
    pub timeout: i32,
    pub hazard_damage: i32,
    pub ruleset: String,

    pub masks: BoardMasks,

    pub all_bodies: Bitboard,
    pub food_bb: Bitboard,
    pub hazard_bb: Bitboard,
    pub tail_freeing_bb: Bitboard,
    pub danger_zone_bb: Bitboard,
    pub enemy_reachable_bb: Bitboard,

    pub my_id: String,
    pub my_head: Coord,
    pub my_tail: Coord,
    pub my_length: i32,
    pub my_health: i32,

    pub enemy_heads: Vec<EnemyHead>,
    pub food: Vec<Coord>,
    pub all_snakes: Vec<SnakeInfo>,

    grid: Vec<u8>,
}

impl Board {
    pub fn new(gs: &ApiGameState) -> Self {
        let w = gs.board.width;
        let h = gs.board.height;
        let timeout = gs.game.timeout;
        let hazard_damage = gs
            .game
            .ruleset
            .settings
            .as_ref()
            .and_then(|s| s.hazard_damage_per_turn)
            .unwrap_or(14);

        let use_bitboards = (w * h) as u128 <= 128;
        let masks = if use_bitboards {
            BoardMasks::new(w as u32, h as u32)
        } else {
            BoardMasks::new(11, 11)
        };

        let mut board = Board {
            width: w,
            height: h,
            turn: gs.turn,
            timeout,
            hazard_damage,
            ruleset: gs.game.ruleset.name.clone(),
            masks,
            all_bodies: Bitboard::EMPTY,
            food_bb: Bitboard::EMPTY,
            hazard_bb: Bitboard::EMPTY,
            tail_freeing_bb: Bitboard::EMPTY,
            danger_zone_bb: Bitboard::EMPTY,
            enemy_reachable_bb: Bitboard::EMPTY,
            my_id: gs.you.id.clone(),
            my_head: gs.you.head_coord(),
            my_tail: gs.you.tail_coord(),
            my_length: gs.you.length,
            my_health: gs.you.health,
            enemy_heads: Vec::new(),
            food: gs.board.food.iter().map(|f| Coord::new(f.x, f.y)).collect(),
            all_snakes: Vec::new(),
            grid: vec![CELL_OPEN; (w * h) as usize],
        };

        board.build_grid(gs);
        board.compute_danger_zones();
        board
    }

    fn build_grid(&mut self, gs: &ApiGameState) {
        let w = self.width as u32;

        for snake in &gs.board.snakes {
            let n = snake.body.len();
            for (i, seg) in snake.body.iter().enumerate() {
                let idx = seg.y * self.width + seg.x;
                if idx < 0 || idx >= self.width * self.height {
                    continue;
                }

                if i == n - 1 {
                    let prev = if n >= 2 { &snake.body[n - 2] } else { seg };
                    if prev.x == seg.x && prev.y == seg.y {
                        self.grid[idx as usize] = CELL_BODY;
                        self.all_bodies = self.all_bodies.set_coord(seg.x, seg.y, w);
                    } else {
                        self.grid[idx as usize] = CELL_BODY;
                        self.all_bodies = self.all_bodies.set_coord(seg.x, seg.y, w);
                        self.tail_freeing_bb =
                            self.tail_freeing_bb.set_coord(seg.x, seg.y, w);
                    }
                } else {
                    self.grid[idx as usize] = CELL_BODY;
                    self.all_bodies = self.all_bodies.set_coord(seg.x, seg.y, w);
                }
            }

            if snake.id != self.my_id {
                let h = Coord::new(snake.head.x, snake.head.y);
                let t = Coord::new(
                    snake.body.last().map(|b| b.x).unwrap_or(snake.head.x),
                    snake.body.last().map(|b| b.y).unwrap_or(snake.head.y),
                );
                self.enemy_heads.push(EnemyHead {
                    pos: h,
                    length: snake.length,
                    health: snake.health,
                    id: snake.id.clone(),
                });
                self.all_snakes.push(SnakeInfo {
                    id: snake.id.clone(),
                    head: h,
                    tail: t,
                    length: snake.length,
                    health: snake.health,
                });
            } else {
                let h = Coord::new(snake.head.x, snake.head.y);
                let t = Coord::new(
                    snake.body.last().map(|b| b.x).unwrap_or(snake.head.x),
                    snake.body.last().map(|b| b.y).unwrap_or(snake.head.y),
                );
                self.all_snakes.push(SnakeInfo {
                    id: snake.id.clone(),
                    head: h,
                    tail: t,
                    length: snake.length,
                    health: snake.health,
                });
            }
        }

        let w32 = w;
        for f in &gs.board.food {
            let idx = f.y * self.width + f.x;
            if idx >= 0 && idx < self.width * self.height && self.grid[idx as usize] == CELL_OPEN {
                self.grid[idx as usize] = CELL_FOOD;
            }
            self.food_bb = self.food_bb.set_coord(f.x, f.y, w32);
        }

        for h in &gs.board.hazards {
            let idx = h.y * self.width + h.x;
            if idx >= 0 && idx < self.width * self.height && self.grid[idx as usize] == CELL_OPEN {
                self.grid[idx as usize] = CELL_HAZARD;
            }
            self.hazard_bb = self.hazard_bb.set_coord(h.x, h.y, w32);
        }
    }

    fn compute_danger_zones(&mut self) {
        let w = self.width as u32;
        for eh in &self.enemy_heads {
            let head_bb = Bitboard::from_coord(eh.pos.x, eh.pos.y, w);
            let reachable = head_bb.expand(&self.masks) & Bitboard(self.masks.board_mask);
            self.enemy_reachable_bb |= reachable;
            if eh.length >= self.my_length {
                self.danger_zone_bb |= reachable;
            }
        }
    }

    #[inline]
    pub fn is_in_bounds(&self, x: i32, y: i32) -> bool {
        x >= 0 && x < self.width && y >= 0 && y < self.height
    }

    #[inline]
    pub fn is_safe(&self, x: i32, y: i32) -> bool {
        if !self.is_in_bounds(x, y) {
            return false;
        }
        let w = self.width as u32;
        let blocked = self.all_bodies & !self.tail_freeing_bb;
        !blocked.test_coord(x, y, w)
    }

    #[inline]
    pub fn is_tail_freeing(&self, x: i32, y: i32) -> bool {
        let w = self.width as u32;
        self.tail_freeing_bb.test_coord(x, y, w)
    }

    #[inline]
    pub fn is_food(&self, x: i32, y: i32) -> bool {
        if !self.is_in_bounds(x, y) {
            return false;
        }
        self.grid[(y * self.width + x) as usize] == CELL_FOOD
    }

    #[inline]
    pub fn is_hazard(&self, x: i32, y: i32) -> bool {
        if !self.is_in_bounds(x, y) {
            return false;
        }
        self.grid[(y * self.width + x) as usize] == CELL_HAZARD
    }

    #[inline]
    pub fn is_in_danger_zone(&self, x: i32, y: i32) -> bool {
        let w = self.width as u32;
        self.danger_zone_bb.test_coord(x, y, w)
    }

    #[inline]
    pub fn is_enemy_reachable(&self, x: i32, y: i32) -> bool {
        let w = self.width as u32;
        self.enemy_reachable_bb.test_coord(x, y, w)
    }

    pub fn enemy_threat_count(&self, x: i32, y: i32) -> i32 {
        let w = self.width as u32;
        let target = Bitboard::from_coord(x, y, w);
        let mut count = 0i32;
        for eh in &self.enemy_heads {
            let head_bb = Bitboard::from_coord(eh.pos.x, eh.pos.y, w);
            let reachable = head_bb.expand(&self.masks);
            if (reachable & target).any() {
                count += 1;
            }
        }
        count
    }

    pub fn flood_fill_enhanced(&self, x: i32, y: i32) -> FloodFillResult {
        if !self.is_in_bounds(x, y) {
            return FloodFillResult { count: 0, has_tail_exit: false, food_count: 0, exit_count: 0 };
        }

        let w = self.width as u32;
        let blocked = self.all_bodies & !self.tail_freeing_bb;

        let start = Bitboard::from_coord(x, y, w);
        let my_tail_bb = Bitboard::from_coord(self.my_tail.x, self.my_tail.y, w);

        let (reachable, has_tail_exit) =
            start.flood_fill_with_target(blocked, my_tail_bb, &self.masks);

        let count = reachable.popcount() as i32;
        let food_count = (reachable & self.food_bb).popcount() as i32;

        let expanded = reachable.expand(&self.masks) & Bitboard(self.masks.board_mask);
        let border = Bitboard(expanded.0 & !reachable.0 & blocked.0);
        let tail_exits = (border & self.tail_freeing_bb).popcount() as i32;
        let open_adjacent = Bitboard(expanded.0 & !reachable.0 & !blocked.0);
        let exit_count = tail_exits + open_adjacent.popcount() as i32;

        FloodFillResult { count, has_tail_exit, food_count, exit_count }
    }

    pub fn flood_fill_count(&self, x: i32, y: i32) -> i32 {
        if !self.is_in_bounds(x, y) {
            return 0;
        }
        let w = self.width as u32;
        let blocked = self.all_bodies & !self.tail_freeing_bb;
        let start = Bitboard::from_coord(x, y, w);
        start.flood_fill(blocked, &self.masks).popcount() as i32
    }

    pub fn is_corridor_entrance(&self, x: i32, y: i32) -> bool {
        let safe_neighbors = self.count_safe_neighbors(x, y);
        if safe_neighbors > 2 {
            return false;
        }
        let mut visited = std::collections::HashSet::new();
        visited.insert((x, y));
        let depth = self.measure_corridor_depth(x, y, &mut visited, 0, 20);
        depth < self.my_length + 2
    }

    fn count_safe_neighbors(&self, x: i32, y: i32) -> i32 {
        let mut count = 0;
        for dir in Direction::ALL {
            let nx = x + dir.dx();
            let ny = y + dir.dy();
            if self.is_safe(nx, ny) {
                count += 1;
            }
        }
        count
    }

    fn measure_corridor_depth(
        &self,
        x: i32,
        y: i32,
        visited: &mut std::collections::HashSet<(i32, i32)>,
        depth: i32,
        max: i32,
    ) -> i32 {
        if depth >= max {
            return i32::MAX;
        }
        let mut best = 1;
        for dir in Direction::ALL {
            let nx = x + dir.dx();
            let ny = y + dir.dy();
            let key = (nx, ny);
            if !visited.contains(&key) && self.is_safe(nx, ny) {
                visited.insert(key);
                let mut next_open = 0;
                for d2 in Direction::ALL {
                    let nnx = nx + d2.dx();
                    let nny = ny + d2.dy();
                    if self.is_safe(nnx, nny) && !visited.contains(&(nnx, nny)) {
                        next_open += 1;
                    }
                }
                if next_open <= 1 {
                    best = best.max(
                        1 + self.measure_corridor_depth(nx, ny, visited, depth + 1, max),
                    );
                } else {
                    return i32::MAX;
                }
            }
        }
        best
    }

    pub fn is_food_trapped(&self, food_x: i32, food_y: i32) -> bool {
        self.flood_fill_count(food_x, food_y) < self.my_length + 3
    }

    pub fn is_food_contested(&self, food_x: i32, food_y: i32) -> bool {
        let my_dist = self.manhattan(self.my_head.x, self.my_head.y, food_x, food_y);
        for eh in &self.enemy_heads {
            let enemy_dist = self.manhattan(eh.pos.x, eh.pos.y, food_x, food_y);
            if enemy_dist <= my_dist && eh.length >= self.my_length {
                return true;
            }
        }
        false
    }

    pub fn nearest_safe_food(&self, from_x: i32, from_y: i32) -> Option<FoodInfo> {
        if self.food.is_empty() {
            return None;
        }
        self.food
            .iter()
            .map(|f| {
                let dist = self.manhattan(from_x, from_y, f.x, f.y);
                let mut penalty = 0i32;
                if self.is_food_trapped(f.x, f.y) {
                    penalty += 100;
                }
                if self.is_food_contested(f.x, f.y) {
                    penalty += 50;
                }
                FoodInfo { food: *f, dist, penalty }
            })
            .min_by_key(|fi| fi.dist + fi.penalty)
    }

    pub fn nearest_food(&self, from_x: i32, from_y: i32) -> Option<FoodInfo> {
        self.food.iter().map(|f| {
            let dist = self.manhattan(from_x, from_y, f.x, f.y);
            FoodInfo { food: *f, dist, penalty: 0 }
        }).min_by_key(|fi| fi.dist)
    }

    pub fn is_near_dangerous_head(&self, x: i32, y: i32) -> bool {
        let my_len = self.my_length;
        for eh in &self.enemy_heads {
            let dist = self.manhattan(x, y, eh.pos.x, eh.pos.y);
            if dist <= 1 && eh.length >= my_len {
                return true;
            }
        }
        false
    }

    pub fn can_kill_head_to_head(&self, x: i32, y: i32) -> bool {
        let my_len = self.my_length;
        for eh in &self.enemy_heads {
            let dist = self.manhattan(x, y, eh.pos.x, eh.pos.y);
            if dist <= 1 && eh.length < my_len {
                let safe_to_engage = self.enemy_heads.iter().all(|other| {
                    other.id == eh.id
                        || other.length < my_len
                        || self.manhattan(x, y, other.pos.x, other.pos.y) > 1
                });
                if safe_to_engage {
                    return true;
                }
            }
        }
        false
    }

    pub fn voronoi_area(&self) -> (i32, i32) {
        self.voronoi_area_from(self.my_head.x, self.my_head.y)
    }

    pub fn voronoi_area_from(&self, from_x: i32, from_y: i32) -> (i32, i32) {
        let size = (self.width * self.height) as usize;
        let mut dist = vec![-1i32; size];
        let mut owner = vec![-1i8; size];
        let mut queue = std::collections::VecDeque::new();

        let my_si = self.all_snakes.iter().position(|s| s.id == self.my_id).unwrap_or(0);

        for (si, snake) in self.all_snakes.iter().enumerate() {
            let head = if si == my_si {
                Coord::new(from_x, from_y)
            } else {
                snake.head
            };
            let idx = (head.y * self.width + head.x) as usize;
            if self.is_in_bounds(head.x, head.y) && dist[idx] == -1 {
                dist[idx] = 0;
                owner[idx] = si as i8;
                queue.push_back(idx);
            }
        }

        while let Some(idx) = queue.pop_front() {
            let x = (idx as i32) % self.width;
            let y = (idx as i32) / self.width;
            let d = dist[idx];
            for dir in Direction::ALL {
                let nx = x + dir.dx();
                let ny = y + dir.dy();
                if self.is_in_bounds(nx, ny) {
                    let nidx = (ny * self.width + nx) as usize;
                    if dist[nidx] == -1 && self.grid[nidx] != CELL_BODY {
                        dist[nidx] = d + 1;
                        owner[nidx] = owner[idx];
                        queue.push_back(nidx);
                    }
                }
            }
        }

        let my_idx = my_si as i8;
        let mut my_area = 0i32;
        let mut total = 0i32;
        for o in &owner {
            if *o >= 0 {
                total += 1;
                if *o == my_idx {
                    my_area += 1;
                }
            }
        }
        (my_area, total)
    }

    pub fn voronoi_area_projected(&self, from_x: i32, from_y: i32, enemy_headstart: i32) -> (i32, i32) {
        let size = (self.width * self.height) as usize;
        let w = self.width as u32;
        let blocked = self.all_bodies & !self.tail_freeing_bb;

        let mut enemy_dist = vec![i32::MAX; size];
        {
            let mut q = std::collections::VecDeque::new();
            for snake in &self.all_snakes {
                if snake.id == self.my_id { continue; }
                if self.is_in_bounds(snake.head.x, snake.head.y)
                    && !blocked.test_coord(snake.head.x, snake.head.y, w)
                {
                    let idx = (snake.head.y * self.width + snake.head.x) as usize;
                    if enemy_dist[idx] == i32::MAX {
                        enemy_dist[idx] = 0;
                        q.push_back(idx);
                    }
                }
            }
            while let Some(idx) = q.pop_front() {
                let x = (idx as i32) % self.width;
                let y = (idx as i32) / self.width;
                let d = enemy_dist[idx];
                for dir in Direction::ALL {
                    let nx = x + dir.dx();
                    let ny = y + dir.dy();
                    if self.is_in_bounds(nx, ny) && !blocked.test_coord(nx, ny, w) {
                        let nidx = (ny * self.width + nx) as usize;
                        if enemy_dist[nidx] == i32::MAX {
                            enemy_dist[nidx] = d + 1;
                            q.push_back(nidx);
                        }
                    }
                }
            }
        }

        let mut my_dist = vec![i32::MAX; size];
        {
            let mut q = std::collections::VecDeque::new();
            if self.is_in_bounds(from_x, from_y) && !blocked.test_coord(from_x, from_y, w) {
                let start_idx = (from_y * self.width + from_x) as usize;
                my_dist[start_idx] = 0;
                q.push_back(start_idx);
            }
            while let Some(idx) = q.pop_front() {
                let x = (idx as i32) % self.width;
                let y = (idx as i32) / self.width;
                let d = my_dist[idx];
                for dir in Direction::ALL {
                    let nx = x + dir.dx();
                    let ny = y + dir.dy();
                    if self.is_in_bounds(nx, ny) && !blocked.test_coord(nx, ny, w) {
                        let nidx = (ny * self.width + nx) as usize;
                        if my_dist[nidx] == i32::MAX {
                            my_dist[nidx] = d + 1;
                            q.push_back(nidx);
                        }
                    }
                }
            }
        }

        let mut my_area = 0i32;
        let mut total = 0i32;
        for i in 0..size {
            let md = my_dist[i];
            let ed = enemy_dist[i];
            if md == i32::MAX && ed == i32::MAX { continue; }
            total += 1;
            let effective_enemy = if ed == i32::MAX {
                i32::MAX
            } else {
                (ed - enemy_headstart).max(0)
            };
            if md < effective_enemy {
                my_area += 1;
            }
        }

        (my_area, total)
    }

    pub fn get_safe_neighbors(&self, x: i32, y: i32) -> Vec<Coord> {
        Direction::ALL
            .iter()
            .filter_map(|&dir| {
                let nx = x + dir.dx();
                let ny = y + dir.dy();
                if self.is_safe(nx, ny) { Some(Coord::new(nx, ny)) } else { None }
            })
            .collect()
    }

    #[inline]
    pub fn manhattan(&self, x1: i32, y1: i32, x2: i32, y2: i32) -> i32 {
        (x1 - x2).abs() + (y1 - y2).abs()
    }

    pub fn enemy_trap_status(&self) -> Vec<(String, bool, i32)> {
        let w = self.width as u32;
        let blocked = self.all_bodies & !self.tail_freeing_bb;

        self.enemy_heads.iter().map(|eh| {
            let start = Bitboard::from_coord(eh.pos.x, eh.pos.y, w);
            let reachable = start.flood_fill(blocked, &self.masks);
            let area = reachable.popcount() as i32;
            (eh.id.clone(), area < eh.length, area)
        }).collect()
    }

    pub fn region_exit_count(&self, x: i32, y: i32) -> i32 {
        let ff = self.flood_fill_enhanced(x, y);
        ff.exit_count
    }

    pub fn is_enemy_mirroring(&self, ex: i32, ey: i32) -> bool {
        let cx = (self.width - 1) as f32 / 2.0;
        let cy = (self.height - 1) as f32 / 2.0;
        let my_dx = (self.my_head.x as f32 - cx).abs();
        let my_dy = (self.my_head.y as f32 - cy).abs();
        let e_dx = (ex as f32 - cx).abs();
        let e_dy = (ey as f32 - cy).abs();
        let row_mirror = (self.my_head.y - ey).abs() <= 1 && (my_dx - e_dx).abs() < 2.0;
        let col_mirror = (self.my_head.x - ex).abs() <= 1 && (my_dy - e_dy).abs() < 2.0;
        row_mirror || col_mirror
    }
}
