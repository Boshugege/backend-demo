use rust_server::{generate_unique_name, PlayerState};
use std::collections::HashMap;

fn empty_player(id: &str) -> PlayerState {
    PlayerState {
        id: id.to_string(),
        x: None,
        y: None,
        z: None,
        ts: None,
        rx: None,
        ry: None,
        rz: None,
        vx: None,
        vy: None,
        vz: None,
        action: None,
    }
}

#[test]
fn generate_unique_name_empty() {
    let world: HashMap<String, PlayerState> = HashMap::new();
    let name = generate_unique_name(&world, "player");
    assert_eq!(name, "player_1");
}

#[test]
fn generate_unique_name_some_taken() {
    let mut world: HashMap<String, PlayerState> = HashMap::new();
    world.insert("foo_1".to_string(), empty_player("foo_1"));
    world.insert("foo_2".to_string(), empty_player("foo_2"));
    let name = generate_unique_name(&world, "foo");
    assert_eq!(name, "foo_3");
}

#[test]
fn generate_unique_name_fallback() {
    let mut world: HashMap<String, PlayerState> = HashMap::new();
    for i in 1..10000 {
        let key = format!("bar_{}", i);
        world.insert(key.clone(), empty_player(&key));
    }
    let name = generate_unique_name(&world, "bar");
    assert_eq!(name, "bar_fallback");
}
