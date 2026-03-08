use std::collections::HashMap;
use crate::api::ApiGameState;

#[derive(Clone, Debug)]
#[allow(dead_code)]
pub struct OpponentProfile {
    pub id: String,
    pub name: String,
    pub samples: u32,
    pub food_seeking: f32,
    pub aggression: f32,
    pub risk_aversion: f32,
}

impl OpponentProfile {
    pub fn new(id: &str, name: &str) -> Self {
        OpponentProfile {
            id: id.to_string(),
            name: name.to_string(),
            samples: 0,
            food_seeking: 0.5,
            aggression: 0.3,
            risk_aversion: 0.5,
        }
    }

    fn update(&mut self, moved_toward_food: bool, moved_toward_us: bool, chose_safe_move: bool) {
        let n = self.samples as f32;
        let n1 = (n + 1.0).max(1.0);
        self.food_seeking = (self.food_seeking * n + if moved_toward_food { 1.0 } else { 0.0 }) / n1;
        self.aggression = (self.aggression * n + if moved_toward_us { 1.0 } else { 0.0 }) / n1;
        self.risk_aversion = (self.risk_aversion * n + if chose_safe_move { 1.0 } else { 0.0 }) / n1;
        self.samples += 1;
    }
}

#[derive(Clone, Debug)]
pub struct GameTracker {
    #[allow(dead_code)]
    pub game_id: String,
    pub prev_state: Option<ApiGameState>,
    pub profiles: HashMap<String, OpponentProfile>,
    pub voronoi_history: Vec<i32>,
    pub territory_shrinking_turns: i32,
}

impl GameTracker {
    pub fn new(gs: &ApiGameState) -> Self {
        let mut profiles = HashMap::new();
        for snake in &gs.board.snakes {
            if snake.id != gs.you.id {
                profiles.insert(snake.id.clone(), OpponentProfile::new(&snake.id, &snake.name));
            }
        }
        GameTracker {
            game_id: gs.game.id.clone(),
            prev_state: None,
            profiles,
            voronoi_history: Vec::new(),
            territory_shrinking_turns: 0,
        }
    }

    pub fn record_turn(&mut self, current: &ApiGameState) {
        if let Some(prev) = &self.prev_state.clone() {
            self.update_profiles(prev, current);
        }
        self.prev_state = Some(current.clone());
    }

    fn update_profiles(&mut self, prev: &ApiGameState, curr: &ApiGameState) {
        let board_w = prev.board.width;
        let board_h = prev.board.height;
        let my_head_prev = &prev.you.head;

        for curr_snake in &curr.board.snakes {
            if curr_snake.id == curr.you.id {
                continue;
            }
            let prev_snake = match prev.board.snakes.iter().find(|s| s.id == curr_snake.id) {
                Some(s) => s,
                None => continue,
            };

            let ph = &prev_snake.head;
            let ch = &curr_snake.head;

            let moved_toward_food = if prev.board.food.is_empty() {
                false
            } else {
                let before_min = prev.board.food.iter()
                    .map(|f| (ph.x - f.x).abs() + (ph.y - f.y).abs())
                    .min()
                    .unwrap_or(i32::MAX);
                let after_min = prev.board.food.iter()
                    .map(|f| (ch.x - f.x).abs() + (ch.y - f.y).abs())
                    .min()
                    .unwrap_or(i32::MAX);
                after_min < before_min
            };

            let dist_before = (ph.x - my_head_prev.x).abs() + (ph.y - my_head_prev.y).abs();
            let dist_after = (ch.x - my_head_prev.x).abs() + (ch.y - my_head_prev.y).abs();
            let moved_toward_us = dist_after < dist_before;

            let new_pos_is_in_bounds = ch.x >= 0 && ch.x < board_w && ch.y >= 0 && ch.y < board_h;
            let chose_safe_move = new_pos_is_in_bounds;

            if let Some(profile) = self.profiles.get_mut(&curr_snake.id) {
                profile.update(moved_toward_food, moved_toward_us, chose_safe_move);
            } else {
                let mut p = OpponentProfile::new(&curr_snake.id, &curr_snake.name);
                p.update(moved_toward_food, moved_toward_us, chose_safe_move);
                self.profiles.insert(curr_snake.id.clone(), p);
            }
        }
    }

    #[allow(dead_code)]
    pub fn get_profile(&self, snake_id: &str) -> Option<&OpponentProfile> {
        self.profiles.get(snake_id)
    }

    pub fn record_voronoi(&mut self, area: i32) {
        self.voronoi_history.push(area);
        if self.voronoi_history.len() > 10 {
            self.voronoi_history.remove(0);
        }
        let len = self.voronoi_history.len();
        if len >= 2 && self.voronoi_history[len - 1] < self.voronoi_history[len - 2] {
            self.territory_shrinking_turns += 1;
        } else {
            self.territory_shrinking_turns = 0;
        }
    }

    pub fn is_territory_collapsing(&self) -> bool {
        self.territory_shrinking_turns >= 3
    }
}
