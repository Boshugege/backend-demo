use serde_json::json;
use std::collections::HashMap;
use std::net::{SocketAddr, UdpSocket};
use std::str;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use uuid::Uuid;
use rust_server::{PlayerState, WorldState, generate_unique_name};

// `PlayerState`, `WorldState` and `generate_unique_name` are defined
// in `src/lib.rs` and re-used by this binary.

fn broadcast_world(socket: &UdpSocket, clients: &HashMap<Uuid, SocketAddr>, world: &WorldState) {
    let payload = json!({"players": world.players}).to_string();
    for addr in clients.values() {
        let _ = socket.send_to(payload.as_bytes(), addr);
    }
}

fn main() -> std::io::Result<()> {
    let socket = UdpSocket::bind(("127.0.0.1", 8888))?;
    socket.set_nonblocking(true)?;
    println!("Rust UDP server listening on 8888...");

    let world = Arc::new(Mutex::new(WorldState { players: HashMap::new() }));
    // clients: uuid -> addr
    let clients: Arc<Mutex<HashMap<Uuid, SocketAddr>>> = Arc::new(Mutex::new(HashMap::new()));
    // username -> uuid
    let username_map: Arc<Mutex<HashMap<String, Uuid>>> = Arc::new(Mutex::new(HashMap::new()));
    // track last seen time per uuid for heartbeat timeout
    let last_seen: Arc<Mutex<HashMap<Uuid, Instant>>> = Arc::new(Mutex::new(HashMap::new()));

    // background cleanup: remove players not seen for 3 minutes (180s)
    {
        let world_bg = world.clone();
        let clients_bg = clients.clone();
        let last_seen_bg = last_seen.clone();
        let username_map_bg = username_map.clone();
        let socket_bg = socket.try_clone()?;
        thread::spawn(move || loop {
            thread::sleep(Duration::from_secs(5));
            let now = Instant::now();
            let mut to_remove: Vec<Uuid> = Vec::new();

            {
                let ls = last_seen_bg.lock().unwrap();
                for (id, &t) in ls.iter() {
                    if now.duration_since(t) > Duration::from_secs(180) {
                        to_remove.push(*id);
                    }
                }
            }

            if !to_remove.is_empty() {
                let mut world = world_bg.lock().unwrap();
                let mut clients = clients_bg.lock().unwrap();
                let mut ls = last_seen_bg.lock().unwrap();
                let mut uname_map = username_map_bg.lock().unwrap();

                for uuid in to_remove.iter() {
                    if let Some(addr) = clients.remove(uuid) {
                        // notify removed client
                        let notif = json!({"action": "removed", "reason": "timeout"});
                        let _ = socket_bg.send_to(notif.to_string().as_bytes(), addr);
                    }
                    // remove username mapping and world entry
                    if let Some(player) = world.players.remove(uuid) {
                        uname_map.remove(&player.username);
                        println!("Removed {} due to timeout", player.username);
                    }
                    ls.remove(uuid);
                }

                // broadcast updated world after removals
                broadcast_world(&socket_bg, &clients, &world);
            }
        });
    }

    let mut buf = [0u8; 2048];
    loop {
        match socket.recv_from(&mut buf) {
            Ok((n, src)) => {
                let data = &buf[..n];
                let s = match str::from_utf8(data) {
                    Ok(x) => x.to_string(),
                    Err(_) => {
                        eprintln!("Invalid utf8 from {}", src);
                        continue;
                    }
                };

                // parse generic JSON to inspect message type
                let v: serde_json::Result<serde_json::Value> = serde_json::from_str(&s);
                if let Ok(val) = v {
                    let world_clone = world.clone();
                    let clients_clone = clients.clone();
                    let last_seen_clone = last_seen.clone();
                    let username_map_clone = username_map.clone();
                    let socket_clone = socket.try_clone().expect("failed clone");

                    thread::spawn(move || {
                        // handle message types: register, heartbeat, update
                        if let Some(t) = val.get("type").and_then(|x| x.as_str()) {
                            match t {
                                "register" => {
                                    if let Some(uname) = val.get("username").and_then(|x| x.as_str()) {
                                        let requested_uuid = val
                                            .get("uuid")
                                            .and_then(|x| x.as_str())
                                            .and_then(|s| Uuid::parse_str(s).ok());
                                        let mut uname_map = username_map_clone.lock().unwrap();
                                        let mut clients = clients_clone.lock().unwrap();
                                        let mut ls = last_seen_clone.lock().unwrap();
                                        let mut world = world_clone.lock().unwrap();

                                        // resume if provided uuid exists server-side
                                        if let Some(existing_uuid) = requested_uuid.and_then(|id| world.players.get(&id).map(|_| id)) {
                                            let player = world.players.get(&existing_uuid).cloned().unwrap();
                                            uname_map.insert(player.username.clone(), existing_uuid);
                                            clients.insert(existing_uuid, src);
                                            ls.insert(existing_uuid, Instant::now());

                                            let resp = json!({
                                                "action": "registered",
                                                "uuid": existing_uuid,
                                                "username": player.username,
                                                "state": player,
                                            });
                                            let _ = socket_clone.send_to(resp.to_string().as_bytes(), src);
                                            broadcast_world(&socket_clone, &clients, &world);
                                            return;
                                        }

                                        if uname_map.contains_key(uname) {
                                            // name already taken and active
                                            let suggested = generate_unique_name(&world.players, uname);
                                            let resp = json!({"action": "name_conflict", "suggested": suggested});
                                            let _ = socket_clone.send_to(resp.to_string().as_bytes(), src);
                                            return;
                                        }

                                        // allocate uuid: prefer requested (unused) else new
                                        let mut new_uuid = requested_uuid.unwrap_or_else(Uuid::new_v4);
                                        while world.players.contains_key(&new_uuid) {
                                            new_uuid = Uuid::new_v4();
                                        }
                                        uname_map.insert(uname.to_string(), new_uuid);
                                        clients.insert(new_uuid, src);
                                        ls.insert(new_uuid, Instant::now());

                                        // create empty player entry
                                        let ps = PlayerState {
                                            uuid: new_uuid,
                                            username: uname.to_string(),
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
                                        };
                                        world.players.insert(new_uuid, ps.clone());

                                        let resp = json!({"action": "registered", "uuid": new_uuid, "username": uname});
                                        let _ = socket_clone.send_to(resp.to_string().as_bytes(), src);

                                        // broadcast updated world
                                        broadcast_world(&socket_clone, &clients, &world);
                                    }
                                }
                                "heartbeat" => {
                                    if let Some(uuid_s) = val.get("uuid").and_then(|x| x.as_str()) {
                                        if let Ok(uuid) = Uuid::parse_str(uuid_s) {
                                            let mut ls = last_seen_clone.lock().unwrap();
                                            ls.insert(uuid, Instant::now());
                                        }
                                    }
                                }
                                "update" => {
                                    // expect uuid and state fields
                                    if let Some(uuid_s) = val.get("uuid").and_then(|x| x.as_str()) {
                                        if let Ok(uuid) = Uuid::parse_str(uuid_s) {
                                            let mut world = world_clone.lock().unwrap();
                                            let mut clients = clients_clone.lock().unwrap();
                                            let mut ls = last_seen_clone.lock().unwrap();

                                            if let Some(existing) = world.players.get(&uuid).cloned() {
                                                // update last seen
                                                ls.insert(uuid, Instant::now());

                                                // start from previous state and apply incoming fields
                                                let mut updated = existing.clone();
                                                updated.x = val.get("x").and_then(|x| x.as_f64());
                                                updated.y = val.get("y").and_then(|x| x.as_f64());
                                                updated.z = val.get("z").and_then(|x| x.as_f64());
                                                updated.ts = val.get("ts").and_then(|x| x.as_u64()).map(|v| v as u128);
                                                updated.rx = val.get("rx").and_then(|x| x.as_f64());
                                                updated.ry = val.get("ry").and_then(|x| x.as_f64());
                                                updated.rz = val.get("rz").and_then(|x| x.as_f64());
                                                updated.vx = val.get("vx").and_then(|x| x.as_f64());
                                                updated.vy = val.get("vy").and_then(|x| x.as_f64());
                                                updated.vz = val.get("vz").and_then(|x| x.as_f64());
                                                updated.action = val.get("action").and_then(|x| x.as_str()).map(|s| s.to_string());

                                                // validate movement similar to before using previous state
                                                let mut send_correction: Option<serde_json::Value> = None;
                                                if let (Some(prev_x), Some(prev_y), Some(prev_z), Some(prev_ts), Some(new_ts)) = (
                                                    existing.x,
                                                    existing.y,
                                                    existing.z,
                                                    existing.ts,
                                                    updated.ts,
                                                ) {
                                                    let dt_ms = if new_ts > prev_ts { new_ts - prev_ts } else { 0 };
                                                    let dt = (dt_ms as f64) / 1000.0;
                                                    if dt > 0.0 && dt < 60.0 {
                                                        let svx = updated.vx.unwrap_or(0.0);
                                                        let svy = updated.vy.unwrap_or(0.0);
                                                        let svz = updated.vz.unwrap_or(0.0);
                                                        let expect_dx = svx * dt;
                                                        let expect_dy = svy * dt;
                                                        let expect_dz = svz * dt;
                                                        let expect_dist = (expect_dx * expect_dx + expect_dy * expect_dy + expect_dz * expect_dz).sqrt();

                                                        let dx = updated.x.unwrap_or(prev_x) - prev_x;
                                                        let dy = updated.y.unwrap_or(prev_y) - prev_y;
                                                        let dz = updated.z.unwrap_or(prev_z) - prev_z;
                                                        let actual_dist = (dx * dx + dy * dy + dz * dz).sqrt();

                                                        let tol = 0.5;
                                                        if actual_dist > expect_dist + tol {
                                                            let corrected_x = prev_x + expect_dx;
                                                            let corrected_y = prev_y + expect_dy;
                                                            let corrected_z = prev_z + expect_dz;

                                                            updated.x = Some(corrected_x);
                                                            updated.y = Some(corrected_y);
                                                            updated.z = Some(corrected_z);
                                                            updated.ts = val.get("ts").and_then(|x| x.as_u64()).map(|v| v as u128);

                                                            let corr = json!({
                                                                "action": "correction",
                                                                "reason": "invalid_movement",
                                                                "corrected": {
                                                                    "uuid": uuid,
                                                                    "username": existing.username,
                                                                    "x": corrected_x,
                                                                    "y": corrected_y,
                                                                    "z": corrected_z,
                                                                    "vx": svx,
                                                                    "vy": svy,
                                                                    "vz": svz,
                                                                    "ts": new_ts
                                                                }
                                                            });
                                                            send_correction = Some(corr);
                                                        }
                                                    }
                                                }

                                                // store state and clients
                                                world.players.insert(uuid, updated.clone());
                                                clients.insert(uuid, src);
                                                println!("Received update for {}", updated.username);

                                                if let Some(c) = send_correction {
                                                    let _ = socket_clone.send_to(c.to_string().as_bytes(), src);
                                                }

                                                // broadcast world
                                                broadcast_world(&socket_clone, &clients, &world);
                                            }
                                        }
                                    }
                                }
                                _ => {}
                            }
                        } else {
                            // legacy/default: ignore or log
                            eprintln!("Unknown message without type from {}: {}", src, s);
                        }
                    });
                } else {
                    eprintln!("Invalid json from {}: {}", src, s);
                }
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                // no data; sleep a bit
                thread::sleep(Duration::from_millis(10));
            }
            Err(e) => {
                eprintln!("recv error: {}", e);
            }
        }
    }
}
