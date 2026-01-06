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
                    if let Some((uname, _)) = uname_map.iter().find(|(_, &u)| u == *uuid).map(|(k, &v)| (k.clone(), v)) {
                        world.players.remove(&uname);
                        uname_map.remove(&uname);
                        println!("Removed {} due to timeout", uname);
                    }
                    ls.remove(uuid);
                }

                // broadcast updated world after removals
                let state = json!({"players": world.players});
                let s = state.to_string();
                for addr in clients.values() {
                    let _ = socket_bg.send_to(s.as_bytes(), addr);
                }
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
                                        let mut uname_map = username_map_clone.lock().unwrap();
                                        let mut clients = clients_clone.lock().unwrap();
                                        let mut ls = last_seen_clone.lock().unwrap();
                                        let mut world = world_clone.lock().unwrap();

                                        if let Some(existing_uuid) = uname_map.get(uname) {
                                            // name already taken and active
                                            let suggested = generate_unique_name(&world.players, uname);
                                            let resp = json!({"action": "name_conflict", "suggested": suggested});
                                            let _ = socket_clone.send_to(resp.to_string().as_bytes(), src);
                                            return;
                                        }

                                        let new_uuid = Uuid::new_v4();
                                        uname_map.insert(uname.to_string(), new_uuid);
                                        clients.insert(new_uuid, src);
                                        ls.insert(new_uuid, Instant::now());

                                        // create empty player entry
                                        let ps = PlayerState {
                                            id: uname.to_string(),
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
                                        world.players.insert(uname.to_string(), ps);

                                        let resp = json!({"action": "registered", "uuid": new_uuid.to_string()});
                                        let _ = socket_clone.send_to(resp.to_string().as_bytes(), src);

                                        // broadcast updated world
                                        let state = json!({"players": world.players});
                                        let s = state.to_string();
                                        for addr in clients.values() {
                                            let _ = socket_clone.send_to(s.as_bytes(), addr);
                                        }
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
                                            let mut uname_map = username_map_clone.lock().unwrap();
                                            if let Some(uname) = uname_map.iter().find(|(_, &u)| u == uuid).map(|(k,_)| k.clone()) {
                                                let mut world = world_clone.lock().unwrap();
                                                let mut clients = clients_clone.lock().unwrap();
                                                let mut ls = last_seen_clone.lock().unwrap();

                                                // update last seen
                                                ls.insert(uuid, Instant::now());

                                                // build PlayerState from incoming fields
                                                let mut p = PlayerState {
                                                    id: uname.clone(),
                                                    x: val.get("x").and_then(|x| x.as_f64()),
                                                    y: val.get("y").and_then(|x| x.as_f64()),
                                                    z: val.get("z").and_then(|x| x.as_f64()),
                                                    ts: val.get("ts").and_then(|x| x.as_u64()).map(|v| v as u128),
                                                    rx: val.get("rx").and_then(|x| x.as_f64()),
                                                    ry: val.get("ry").and_then(|x| x.as_f64()),
                                                    rz: val.get("rz").and_then(|x| x.as_f64()),
                                                    vx: val.get("vx").and_then(|x| x.as_f64()),
                                                    vy: val.get("vy").and_then(|x| x.as_f64()),
                                                    vz: val.get("vz").and_then(|x| x.as_f64()),
                                                    action: val.get("action").and_then(|x| x.as_str()).map(|s| s.to_string()),
                                                };

                                                // validate movement similar to before using username as key
                                                let mut send_correction: Option<serde_json::Value> = None;
                                                if let Some(prev) = world.players.get(&uname) {
                                                    if let (Some(prev_x), Some(prev_y), Some(prev_z), Some(prev_ts), Some(new_ts)) = (
                                                        prev.x,
                                                        prev.y,
                                                        prev.z,
                                                        prev.ts,
                                                        p.ts,
                                                    ) {
                                                        let dt_ms = if new_ts > prev_ts { new_ts - prev_ts } else { 0 };
                                                        let dt = (dt_ms as f64) / 1000.0;
                                                        if dt > 0.0 && dt < 60.0 {
                                                            let svx = p.vx.unwrap_or(0.0);
                                                            let svy = p.vy.unwrap_or(0.0);
                                                            let svz = p.vz.unwrap_or(0.0);
                                                            let expect_dx = svx * dt;
                                                            let expect_dy = svy * dt;
                                                            let expect_dz = svz * dt;
                                                            let expect_dist = (expect_dx * expect_dx + expect_dy * expect_dy + expect_dz * expect_dz).sqrt();

                                                            let dx = p.x.unwrap_or(prev_x) - prev_x;
                                                            let dy = p.y.unwrap_or(prev_y) - prev_y;
                                                            let dz = p.z.unwrap_or(prev_z) - prev_z;
                                                            let actual_dist = (dx * dx + dy * dy + dz * dz).sqrt();

                                                            let tol = 0.5;
                                                            if actual_dist > expect_dist + tol {
                                                                let corrected_x = prev_x + expect_dx;
                                                                let corrected_y = prev_y + expect_dy;
                                                                let corrected_z = prev_z + expect_dz;

                                                                p.x = Some(corrected_x);
                                                                p.y = Some(corrected_y);
                                                                p.z = Some(corrected_z);
                                                                p.ts = val.get("ts").and_then(|x| x.as_u64()).map(|v| v as u128);

                                                                let corr = json!({
                                                                    "action": "correction",
                                                                    "reason": "invalid_movement",
                                                                    "corrected": {
                                                                        "username": uname.clone(),
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
                                                }

                                                // store state and clients
                                                world.players.insert(uname.clone(), p.clone());
                                                clients.insert(uuid, src);
                                                println!("Received update for {}", uname);

                                                if let Some(c) = send_correction {
                                                    let _ = socket_clone.send_to(c.to_string().as_bytes(), src);
                                                }

                                                // broadcast world
                                                let state = json!({"players": world.players});
                                                let s = state.to_string();
                                                for addr in clients.values() {
                                                    let _ = socket_clone.send_to(s.as_bytes(), addr);
                                                }
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
