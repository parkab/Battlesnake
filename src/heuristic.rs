use crate::bitboard::Bitboard;
use crate::simulator::{SimState, MAX_SNAKES};

pub fn evaluate(state: &SimState) -> f32 {
    let my_idx = state.my_idx as usize;
    let my = &state.snakes[my_idx];

    if !my.alive {
        return -1.0;
    }

    let alive_enemies: Vec<usize> = (0..state.num_snakes as usize)
        .filter(|&i| i != my_idx && state.snakes[i].alive)
        .collect();

    if alive_enemies.is_empty() {
        return 1.0;
    }

    let max_enemy_len = alive_enemies.iter().map(|&i| state.snakes[i].len).max().unwrap_or(1);
    let is_duel = alive_enemies.len() == 1;

    let mut score = 0.0f32;

    score += (my.health as f32 / 100.0) * 0.06;

    let my_len = my.len as f32;
    let max_enemy = max_enemy_len as f32;
    let denom = my_len.max(max_enemy).max(1.0);
    let length_adv = (my_len - max_enemy) / denom;
    score += length_adv * 0.12;

    let (vor_score, my_territory, enemy_territory) = area_control_v2(state, my_idx, &alive_enemies);
    score += vor_score * 0.22;

    let space_weight = if is_duel { 0.12 } else if alive_enemies.len() >= 3 { 0.16 } else { 0.12 };
    score += space_sufficiency(state, my_idx, my_territory) * space_weight;

    score += forced_kill_score(state, my_idx, &alive_enemies) * 0.08;

    score += food_proximity(state, my_idx, &alive_enemies) * 0.07;

    score += aggression_v2(state, my_idx, &alive_enemies, my_territory, enemy_territory) * 0.08;

    let center_weight = if is_duel {
        0.15
    } else if alive_enemies.len() >= 3 {
        0.04
    } else {
        0.09
    };
    score += center_control(state, my_idx) * center_weight;

    let wall_weight = if is_duel {
        0.10
    } else if alive_enemies.len() >= 3 {
        0.03
    } else {
        0.06
    };
    score += wall_penalty(state, my_idx) * wall_weight;

    let mobility_weight = 0.10;
    score += mobility(state, my_idx) * mobility_weight;

    let exit_weight = if alive_enemies.len() >= 3 { 0.18 } else { 0.15 };
    score += exit_safety(state, my_idx) * exit_weight;

    score.clamp(-1.0, 1.0)
}

fn area_control_v2(state: &SimState, my_idx: usize, enemies: &[usize]) -> (f32, i32, i32) {
    let w = state.width as u32;
    let h = state.height as u32;
    let masks = &state.masks;
    let num = state.num_snakes as usize;

    let mut frontiers = [Bitboard::EMPTY; MAX_SNAKES];
    let mut owned = [Bitboard::EMPTY; MAX_SNAKES];
    let mut contested = Bitboard::EMPTY;
    let mut reached = Bitboard::EMPTY;

    let moving_tails = state.moving_tails_bb();
    let blocked = Bitboard(state.all_bodies & !moving_tails);

    for i in 0..num {
        let s = &state.snakes[i];
        if !s.alive {
            continue;
        }
        let hb = Bitboard::from_idx(s.head() as u32);
        frontiers[i] = hb & !blocked;
        owned[i] = frontiers[i];
        reached |= frontiers[i];
    }

    let board_mask = Bitboard(masks.board_mask);
    for _ in 0..(w + h) {
        let mut any_expanded = false;
        let mut round_reached = Bitboard::EMPTY;
        let mut round_contested = Bitboard::EMPTY;

        for i in 0..num {
            if frontiers[i].is_empty() {
                continue;
            }
            let expansion = frontiers[i].expand(masks) & board_mask & !blocked & !reached;
            if expansion.any() {
                let overlap = expansion & round_reached;
                if overlap.any() {
                    round_contested |= overlap;
                }
                round_reached |= expansion;
                any_expanded = true;
                owned[i] |= expansion;
                frontiers[i] = expansion;
            } else {
                frontiers[i] = Bitboard::EMPTY;
            }
        }

        if round_contested.any() {
            for i in 0..num {
                owned[i] = Bitboard(owned[i].0 & !round_contested.0);
            }
            contested |= round_contested;
        }
        reached |= round_reached;
        if !any_expanded {
            break;
        }
    }

    let my_area = owned[my_idx].popcount() as f32;
    let contested_count = contested.popcount() as f32;

    let my_len = state.snakes[my_idx].len as f32;
    let max_enemy_len = enemies.iter().map(|&i| state.snakes[i].len as f32).fold(0.0f32, f32::max);
    let len_ratio = if max_enemy_len > 0.0 { my_len / (my_len + max_enemy_len) } else { 0.5 };

    let my_effective = my_area + contested_count * len_ratio;

    let max_enemy_area = enemies.iter()
        .map(|&i| owned[i].popcount() as f32)
        .fold(0.0f32, f32::max);
    let enemy_effective = max_enemy_area + contested_count * (1.0 - len_ratio);

    let total = my_effective + enemy_effective;
    if total == 0.0 {
        return (0.0, 0, 0);
    }

    let num_alive = enemies.len() as f32 + 1.0;
    let fair_share = 1.0 / num_alive;
    let norm = ((my_effective / total) - fair_share) * 2.0;

    (norm, my_area as i32, max_enemy_area as i32)
}

fn space_sufficiency(state: &SimState, my_idx: usize, voronoi_area: i32) -> f32 {
    let my = &state.snakes[my_idx];
    let raw_area = flood_fill_area(state, my.head()) as f32;
    let len = my.len as f32;
    let effective_area = raw_area.min(voronoi_area.max(my.len as i32) as f32);
    let ratio = effective_area / len.max(1.0);
    if ratio < 1.0 {
        -1.0
    } else if ratio < 1.5 {
        -0.8 + (ratio - 1.0) * 1.0
    } else if ratio < 3.0 {
        -0.3 + (ratio - 1.5) * 0.2
    } else if ratio < 6.0 {
        (ratio - 3.0) / 3.0 * 0.4
    } else {
        0.4
    }
}

fn forced_kill_score(state: &SimState, my_idx: usize, enemies: &[usize]) -> f32 {
    let mut score = 0.0f32;

    for &ei in enemies {
        let e = &state.snakes[ei];
        if !e.alive { continue; }
        let enemy_space = flood_fill_area(state, e.head());
        let enemy_len = e.len as i32;

        if enemy_space < enemy_len {
            score += 1.0;
        } else if enemy_space < enemy_len + 3 {
            score += 0.5;
        } else if enemy_space < enemy_len * 2 {
            score += 0.15;
        }
    }

    let my = &state.snakes[my_idx];
    let my_space = flood_fill_area(state, my.head());
    let my_len = my.len as i32;
    if my_space < my_len {
        score -= 1.5;
    } else if my_space < my_len + 3 {
        score -= 0.6;
    }

    score.clamp(-1.0, 1.0)
}

fn exit_safety(state: &SimState, my_idx: usize) -> f32 {
    let my = &state.snakes[my_idx];
    let head = my.head();
    let hx = (head % state.width) as i32;
    let hy = (head / state.width) as i32;

    let mut total_exits = 0;
    let w = state.width as i32;
    let h = state.height as i32;
    let moving_tails = state.moving_tails_bb();
    let safe = !state.all_bodies | moving_tails;

    for &dir in &crate::types::Direction::ALL {
        let nx = hx + dir.dx();
        let ny = hy + dir.dy();
        if nx >= 0 && nx < w && ny >= 0 && ny < h {
            let npos = (ny as u8) * state.width + (nx as u8);
            if (safe & (1u128 << npos)) != 0 {
                let mut exits = 0;
                for &d2 in &crate::types::Direction::ALL {
                    let nnx = nx + d2.dx();
                    let nny = ny + d2.dy();
                    if nnx >= 0 && nnx < w && nny >= 0 && nny < h {
                        let nnpos = (nny as u8) * state.width + (nnx as u8);
                        if nnpos != head && (safe & (1u128 << nnpos)) != 0 {
                            exits += 1;
                        }
                    }
                }
                total_exits += exits;
            }
        }
    }

    if total_exits == 0 {
        -1.0
    } else if total_exits <= 2 {
        -0.5
    } else if total_exits <= 4 {
        -0.1
    } else {
        0.2
    }
}

fn food_proximity(state: &SimState, my_idx: usize, enemies: &[usize]) -> f32 {
    if state.food == 0 {
        return 0.0;
    }
    let my = &state.snakes[my_idx];
    let head = my.head();
    let hx = (head % state.width) as i32;
    let hy = (head / state.width) as i32;

    let mut min_dist = i32::MAX;
    let mut food_bb = state.food;
    while food_bb != 0 {
        let bit = food_bb.trailing_zeros();
        let fx = (bit % state.width as u32) as i32;
        let fy = (bit / state.width as u32) as i32;
        let d = (hx - fx).abs() + (hy - fy).abs();
        if d < min_dist {
            min_dist = d;
        }
        food_bb &= food_bb - 1;
    }

    let max_dist = state.width as f32 + state.height as f32;
    let proximity = 1.0 - (min_dist as f32 / max_dist);

    let urgency = if my.health < 15 {
        3.0
    } else if my.health < 30 {
        2.0
    } else if my.health < 60 {
        1.2
    } else {
        0.6
    };

    let max_enemy_len = enemies.iter().map(|&i| state.snakes[i].len).max().unwrap_or(0);
    let len_factor = if my.health < 30 {
        1.0
    } else if my.len > max_enemy_len + 5 {
        0.1
    } else if my.len > max_enemy_len + 2 {
        0.35
    } else {
        1.0
    };

    proximity * urgency * 0.5 * len_factor
}

fn aggression_v2(state: &SimState, my_idx: usize, enemies: &[usize], my_territory: i32, enemy_territory: i32) -> f32 {
    if enemies.is_empty() {
        return 0.0;
    }

    let my = &state.snakes[my_idx];
    let my_head = my.head();
    let my_hx = (my_head % state.width) as i32;
    let my_hy = (my_head / state.width) as i32;
    let my_len = my.len;

    let territory_advantage = my_territory as f32 - enemy_territory as f32;
    let max_enemy_len = enemies.iter().map(|&i| state.snakes[i].len).max().unwrap_or(0);

    #[derive(PartialEq)]
    enum Mode { Survival, Pressure, Kill }

    let mode = if my.health < 25 {
        Mode::Survival
    } else if enemies.len() >= 3 {
        if (my_len as i32) > max_enemy_len as i32 + 4 && territory_advantage > 15.0 {
            Mode::Pressure
        } else {
            Mode::Survival
        }
    } else if my_len > max_enemy_len + 2 && territory_advantage > 5.0 {
        Mode::Kill
    } else if my_len > max_enemy_len {
        Mode::Pressure
    } else if territory_advantage < -10.0 {
        Mode::Survival
    } else {
        Mode::Pressure
    };

    let max_d = state.width as f32 + state.height as f32;
    let mut score = 0.0f32;

    for &ei in enemies {
        let e = &state.snakes[ei];
        let e_head = e.head();
        let ex = (e_head % state.width) as i32;
        let ey = (e_head / state.width) as i32;
        let dist = ((my_hx - ex).abs() + (my_hy - ey).abs()) as f32;

        match mode {
            Mode::Kill => {
                if my_len > e.len + 1 {
                    score += (1.0 - dist / max_d) * 0.8;
                } else if my_len > e.len {
                    score += (1.0 - dist / max_d) * 0.5;
                }
                let cx = (state.width as i32 - 1) / 2;
                let cy = (state.height as i32 - 1) / 2;
                let my_to_center = (my_hx - cx).abs() + (my_hy - cy).abs();
                let enemy_to_center = (ex - cx).abs() + (ey - cy).abs();
                if my_to_center < enemy_to_center {
                    score += 0.3;
                }
            }
            Mode::Pressure => {
                if my_len > e.len + 1 {
                    score += (1.0 - dist / max_d) * 0.4;
                } else if my_len > e.len {
                    score += (1.0 - dist / max_d) * 0.2;
                } else if my_len < e.len {
                    score -= (1.0 - dist / max_d) * 0.4;
                } else if dist <= 2.0 {
                    score -= 0.2;
                }
            }
            Mode::Survival => {
                if my_len <= e.len && dist < 4.0 {
                    score += (dist / max_d) * 0.5;
                }
            }
        }
    }

    let divisor = enemies.len().max(1) as f32;
    (score / divisor).clamp(-1.0, 1.0)
}

fn center_control(state: &SimState, my_idx: usize) -> f32 {
    let my = &state.snakes[my_idx];
    let head = my.head();
    let hx = (head % state.width) as i32;
    let hy = (head / state.width) as i32;
    let cx = (state.width as i32 - 1) / 2;
    let cy = (state.height as i32 - 1) / 2;
    let dist = (hx - cx).abs() + (hy - cy).abs();
    let max_dist = cx + cy;
    if max_dist == 0 {
        return 0.5;
    }
    (1.0 - dist as f32 / max_dist as f32) * 1.5 - 0.25
}

fn wall_penalty(state: &SimState, my_idx: usize) -> f32 {
    let my = &state.snakes[my_idx];
    let head = my.head();
    let hx = (head % state.width) as i32;
    let hy = (head / state.width) as i32;
    let w = state.width as i32;
    let h = state.height as i32;
    let edge_x = hx.min(w - 1 - hx);
    let edge_y = hy.min(h - 1 - hy);
    let min_edge = edge_x.min(edge_y);
    if min_edge == 0 {
        -1.0
    } else if min_edge == 1 {
        -0.5
    } else {
        0.0
    }
}

fn mobility(state: &SimState, my_idx: usize) -> f32 {
    let valid = state.get_valid_moves(my_idx);
    let moves = valid.iter().filter(|m| m.is_some()).count();
    match moves {
        0 => -1.0,
        1 => -0.3,
        2 => 0.0,
        3 => 0.25,
        _ => 0.5,
    }
}

pub fn flood_fill_area(state: &SimState, start_pos: u8) -> i32 {
    let moving_tails = state.moving_tails_bb();
    let blocked = Bitboard(state.all_bodies & !moving_tails);
    let start = Bitboard::from_idx(start_pos as u32);
    start.flood_fill(blocked, &state.masks).popcount() as i32
}
