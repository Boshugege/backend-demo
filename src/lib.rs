use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PlayerState {
    pub id: String,
    // position
    pub x: Option<f64>,
    pub y: Option<f64>,
    pub z: Option<f64>,
    // timestamp provided by client (millis since epoch)
    pub ts: Option<u128>,
    // rotation (Euler)
    pub rx: Option<f64>,
    pub ry: Option<f64>,
    pub rz: Option<f64>,
    // velocity
    pub vx: Option<f64>,
    pub vy: Option<f64>,
    pub vz: Option<f64>,
    // optional action field for future use
    pub action: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct WorldState {
    pub players: HashMap<String, PlayerState>,
}

pub fn generate_unique_name(world: &HashMap<String, PlayerState>, base: &str) -> String {
    for i in 1..10000 {
        let candidate = format!("{}_{}", base, i);
        if !world.contains_key(&candidate) {
            return candidate;
        }
    }
    format!("{}_fallback", base)
}
