#![allow(dead_code)]

use serde::{Deserialize, Serialize};
use crate::types::Coord;

#[derive(Deserialize, Clone, Debug)]
pub struct ApiGameState {
    pub game: ApiGame,
    pub turn: i32,
    pub board: ApiBoard,
    pub you: ApiBattlesnake,
}

#[derive(Deserialize, Clone, Debug)]
pub struct ApiGame {
    pub id: String,
    pub ruleset: ApiRuleset,
    pub map: Option<String>,
    pub timeout: i32,
    pub source: Option<String>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct ApiRuleset {
    pub name: String,
    pub version: Option<String>,
    pub settings: Option<ApiRulesetSettings>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct ApiRulesetSettings {
    #[serde(rename = "foodSpawnChance")]
    pub food_spawn_chance: Option<i32>,
    #[serde(rename = "minimumFood")]
    pub minimum_food: Option<i32>,
    #[serde(rename = "hazardDamagePerTurn")]
    pub hazard_damage_per_turn: Option<i32>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct ApiBoard {
    pub height: i32,
    pub width: i32,
    pub food: Vec<ApiCoord>,
    pub hazards: Vec<ApiCoord>,
    pub snakes: Vec<ApiBattlesnake>,
}

#[derive(Deserialize, Clone, Debug)]
pub struct ApiBattlesnake {
    pub id: String,
    pub name: String,
    pub health: i32,
    pub body: Vec<ApiCoord>,
    pub latency: Option<serde_json::Value>,
    pub head: ApiCoord,
    pub length: i32,
    pub shout: Option<String>,
    pub customizations: Option<ApiCustomizations>,
}

impl ApiBattlesnake {
    pub fn head_coord(&self) -> Coord {
        Coord::new(self.head.x, self.head.y)
    }

    pub fn tail_coord(&self) -> Coord {
        let t = self.body.last().unwrap_or(&self.head);
        Coord::new(t.x, t.y)
    }
}

#[derive(Deserialize, Clone, Debug)]
pub struct ApiCoord {
    pub x: i32,
    pub y: i32,
}

impl From<&ApiCoord> for Coord {
    fn from(c: &ApiCoord) -> Coord {
        Coord::new(c.x, c.y)
    }
}

#[derive(Deserialize, Clone, Debug)]
pub struct ApiCustomizations {
    pub color: Option<String>,
    pub head: Option<String>,
    pub tail: Option<String>,
}

#[derive(Serialize)]
pub struct InfoResponse {
    pub apiversion: String,
    pub author: String,
    pub color: String,
    pub head: String,
    pub tail: String,
    pub version: String,
}

#[derive(Serialize)]
pub struct MoveResponse {
    #[serde(rename = "move")]
    pub direction: String,
    pub shout: String,
}
