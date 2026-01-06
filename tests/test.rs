use rust_server::{generate_unique_name, PlayerState};
use std::collections::HashMap;
use uuid::Uuid;

fn empty_player(username: &str) -> PlayerState {
    PlayerState {
        uuid: Uuid::new_v4(),
        username: username.to_string(),
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
    let world: HashMap<Uuid, PlayerState> = HashMap::new();
    let name = generate_unique_name(&world, "player");
    assert_eq!(name, "player_1");
}

#[test]
fn generate_unique_name_some_taken() {
    let mut world: HashMap<Uuid, PlayerState> = HashMap::new();
    world.insert(Uuid::new_v4(), empty_player("foo_1"));
    world.insert(Uuid::new_v4(), empty_player("foo_2"));
    let name = generate_unique_name(&world, "foo");
    assert_eq!(name, "foo_3");
}

#[test]
fn generate_unique_name_fallback() {
    let mut world: HashMap<Uuid, PlayerState> = HashMap::new();
    for i in 1..10000 {
        let key = format!("bar_{}", i);
        world.insert(Uuid::new_v4(), empty_player(&key));
    }
    let name = generate_unique_name(&world, "bar");
    assert_eq!(name, "bar_fallback");
}
