use serde_json::json;
use std::collections::HashMap;
use std::net::{SocketAddr, UdpSocket};
use std::str;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use uuid::Uuid;
use backend_demo::{PlayerState, WorldState, UuidStorage, generate_unique_name};

// `PlayerState`, `WorldState` and `generate_unique_name` are defined
// in `src/lib.rs` and re-used by this binary.

fn broadcast_world(socket: &UdpSocket, clients: &HashMap<Uuid, SocketAddr>, world: &WorldState, online_status: &HashMap<Uuid, bool>) {
    // 只广播在线玩家
    let online_players: HashMap<Uuid, PlayerState> = world.players
        .iter()
        .filter(|(uuid, _)| online_status.get(uuid).copied().unwrap_or(false))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    
    let payload = json!({"players": online_players}).to_string();
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
    // track last seen time per uuid for inactivity timeout
    let last_seen: Arc<Mutex<HashMap<Uuid, Instant>>> = Arc::new(Mutex::new(HashMap::new()));
    // online status: uuid -> bool (true=online, false=offline)
    let online_status: Arc<Mutex<HashMap<Uuid, bool>>> = Arc::new(Mutex::new(HashMap::new()));
    // UUID persistence storage
    let uuid_storage: Arc<Mutex<UuidStorage>> = Arc::new(Mutex::new(
        UuidStorage::load_from_file("uuid_storage.json").unwrap_or_else(|_| UuidStorage {
            uuids: HashMap::new(),
        })
    ));

    // background cleanup: mark players offline if not seen for 60 seconds
    {
        let world_bg = world.clone();
        let clients_bg = clients.clone();
        let last_seen_bg = last_seen.clone();
        let online_status_bg = online_status.clone();
        let uuid_storage_bg = uuid_storage.clone();
        let socket_bg = socket.try_clone()?;
        thread::spawn(move || loop {
            thread::sleep(Duration::from_secs(5));
            let now = Instant::now();
            let mut to_offline: Vec<Uuid> = Vec::new();

            {
                let ls = last_seen_bg.lock().unwrap();
                for (id, &t) in ls.iter() {
                    if now.duration_since(t) > Duration::from_secs(60) {
                        to_offline.push(*id);
                    }
                }
            }

            if !to_offline.is_empty() {
                let world = world_bg.lock().unwrap();
                let clients = clients_bg.lock().unwrap();
                let mut online = online_status_bg.lock().unwrap();
                let mut storage = uuid_storage_bg.lock().unwrap();

                for uuid in to_offline.iter() {
                    if let Some(player) = world.players.get(uuid) {
                        // Mark as offline
                        online.insert(*uuid, false);
                        
                        // Persist UUID to storage
                        storage.add_uuid(*uuid, player.username.clone());
                        let _ = storage.save_to_file("uuid_storage.json");
                        
                        // Notify the player
                        if let Some(addr) = clients.get(uuid) {
                            let notif = json!({
                                "action": "offline",
                                "reason": "inactivity",
                                "uuid": uuid,
                                "message": "No activity for 60 seconds, going offline. Rejoin with same UUID to resume."
                            });
                            let _ = socket_bg.send_to(notif.to_string().as_bytes(), addr);
                        }
                        
                        println!("Marked {} as offline (UUID saved)", player.username);
                    }
                }

                // broadcast updated world (only online players)
                broadcast_world(&socket_bg, &clients, &world, &online);
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
                    let online_status_clone = online_status.clone();
                    let username_map_clone = username_map.clone();
                    let uuid_storage_clone = uuid_storage.clone();
                    let socket_clone = socket.try_clone().expect("failed clone");

                    thread::spawn(move || {
                        // handle message types: register, update
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
                                        let mut online = online_status_clone.lock().unwrap();
                                        let mut world = world_clone.lock().unwrap();
                                        let mut storage = uuid_storage_clone.lock().unwrap();

                                        // Try to resume if provided uuid exists
                                        if let Some(existing_uuid) = requested_uuid {
                                            if world.players.contains_key(&existing_uuid) {
                                                // UUID exists in memory - resume
                                                let player = world.players.get(&existing_uuid).cloned().unwrap();
                                                uname_map.insert(player.username.clone(), existing_uuid);
                                                clients.insert(existing_uuid, src);
                                                ls.insert(existing_uuid, Instant::now());
                                                online.insert(existing_uuid, true);

                                                let resp = json!({
                                                    "action": "registered",
                                                    "uuid": existing_uuid,
                                                    "username": player.username,
                                                    "state": player,
                                                    "resumed": true
                                                });
                                                let _ = socket_clone.send_to(resp.to_string().as_bytes(), src);
                                                broadcast_world(&socket_clone, &clients, &world, &online);
                                                return;
                                            } else if storage.contains_uuid(&existing_uuid) {
                                                // UUID exists in persistent storage - restore
                                                let stored_username = storage.get_username(&existing_uuid).unwrap();
                                                
                                                // Create restored player state
                                                let restored_player = PlayerState {
                                                    uuid: existing_uuid,
                                                    username: stored_username.clone(),
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
                                                
                                                world.players.insert(existing_uuid, restored_player.clone());
                                                uname_map.insert(stored_username.clone(), existing_uuid);
                                                clients.insert(existing_uuid, src);
                                                ls.insert(existing_uuid, Instant::now());
                                                online.insert(existing_uuid, true);

                                                let resp = json!({
                                                    "action": "registered",
                                                    "uuid": existing_uuid,
                                                    "username": stored_username,
                                                    "state": restored_player,
                                                    "resumed": true,
                                                    "from_storage": true
                                                });
                                                let _ = socket_clone.send_to(resp.to_string().as_bytes(), src);
                                                broadcast_world(&socket_clone, &clients, &world, &online);
                                                return;
                                            }
                                        }

                                        // Check for active username conflict (online players only)
                                        if uname_map.contains_key(uname) {
                                            let suggested = generate_unique_name(&world.players, uname);
                                            let resp = json!({"action": "name_conflict", "suggested": suggested});
                                            let _ = socket_clone.send_to(resp.to_string().as_bytes(), src);
                                            return;
                                        }

                                        // allocate new uuid
                                        let mut new_uuid = requested_uuid.unwrap_or_else(Uuid::new_v4);
                                        while world.players.contains_key(&new_uuid) {
                                            new_uuid = Uuid::new_v4();
                                        }
                                        
                                        uname_map.insert(uname.to_string(), new_uuid);
                                        clients.insert(new_uuid, src);
                                        ls.insert(new_uuid, Instant::now());
                                        online.insert(new_uuid, true);
                                        storage.add_uuid(new_uuid, uname.to_string());
                                        let _ = storage.save_to_file("uuid_storage.json");

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
                                        broadcast_world(&socket_clone, &clients, &world, &online);
                                    }
                                }
                                "update" => {
                                    // expect uuid and state fields
                                    if let Some(uuid_s) = val.get("uuid").and_then(|x| x.as_str()) {
                                        if let Ok(uuid) = Uuid::parse_str(uuid_s) {
                                            let mut world = world_clone.lock().unwrap();
                                            let mut clients = clients_clone.lock().unwrap();
                                            let mut ls = last_seen_clone.lock().unwrap();
                                            let mut online = online_status_clone.lock().unwrap();

                                            if let Some(existing) = world.players.get(&uuid).cloned() {
                                                // update last seen and mark as online
                                                ls.insert(uuid, Instant::now());
                                                online.insert(uuid, true);

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

                                                // broadcast world (only online players)
                                                broadcast_world(&socket_clone, &clients, &world, &online);
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
