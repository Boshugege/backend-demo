use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;
use std::net::{SocketAddr, UdpSocket};
use std::str;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug, Serialize, Deserialize, Clone)]
struct PlayerState {
    id: String,
    // position
    x: Option<f64>,
    y: Option<f64>,
    z: Option<f64>,
    // timestamp provided by client (millis since epoch)
    ts: Option<u128>,
    // rotation (Euler)
    rx: Option<f64>,
    ry: Option<f64>,
    rz: Option<f64>,
    // velocity
    vx: Option<f64>,
    vy: Option<f64>,
    vz: Option<f64>,
    // optional action field for future use
    action: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct WorldState {
    players: HashMap<String, PlayerState>,
}

fn generate_unique_name(world: &HashMap<String, PlayerState>, base: &str) -> String {
    for i in 1..10000 {
        let candidate = format!("{}_{}", base, i);
        if !world.contains_key(&candidate) {
            return candidate;
        }
    }
    format!("{}_fallback", base)
}

fn main() -> std::io::Result<()> {
    let socket = UdpSocket::bind(("127.0.0.1", 8888))?;
    socket.set_nonblocking(true)?;
    println!("Rust UDP server listening on 8888...");

    let world = Arc::new(Mutex::new(WorldState { players: HashMap::new() }));
    let clients: Arc<Mutex<HashMap<String, SocketAddr>>> = Arc::new(Mutex::new(HashMap::new()));
    // track last seen time per player for timeout-based removal
    let last_seen: Arc<Mutex<HashMap<String, Instant>>> = Arc::new(Mutex::new(HashMap::new()));

    // background cleanup: remove players not seen for 30 seconds
    {
        let world_bg = world.clone();
        let clients_bg = clients.clone();
        let last_seen_bg = last_seen.clone();
        let socket_bg = socket.try_clone()?;
        thread::spawn(move || loop {
            thread::sleep(Duration::from_secs(5));
            let now = Instant::now();
            let mut to_remove: Vec<String> = Vec::new();

            {
                let ls = last_seen_bg.lock().unwrap();
                for (id, &t) in ls.iter() {
                    if now.duration_since(t) > Duration::from_secs(30) {
                        to_remove.push(id.clone());
                    }
                }
            }

            if !to_remove.is_empty() {
                let mut world = world_bg.lock().unwrap();
                let mut clients = clients_bg.lock().unwrap();
                let mut ls = last_seen_bg.lock().unwrap();

                for id in to_remove.iter() {
                    if let Some(addr) = clients.remove(id) {
                        // notify the removed client if we have address
                        let notif = json!({"action": "removed", "reason": "timeout"});
                        let _ = socket_bg.send_to(notif.to_string().as_bytes(), addr);
                    }
                    world.players.remove(id);
                    ls.remove(id);
                    println!("Removed {} due to timeout", id);
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
                    Ok(x) => x,
                    Err(_) => {
                        eprintln!("Invalid utf8 from {}", src);
                        continue;
                    }
                };

                let input: serde_json::Result<PlayerState> = serde_json::from_str(s);
                if let Ok(p) = input {
                    let world_clone = world.clone();
                    let clients_clone = clients.clone();
                    let last_seen_clone = last_seen.clone();
                    let socket_clone = socket.try_clone().expect("failed clone");

                    thread::spawn(move || {
                        let mut world = world_clone.lock().unwrap();
                        let mut clients = clients_clone.lock().unwrap();

                        // Name conflict detection: if id already exists with different addr
                        if let Some(existing_addr) = clients.get(&p.id) {
                            if *existing_addr != src {
                                let suggested = generate_unique_name(&world.players, &p.id);
                                let warning = json!({"action": "name_conflict", "suggested": suggested});
                                let _ = socket_clone.send_to(warning.to_string().as_bytes(), src);
                                return;
                            }
                        }

                        // update last_seen for this id
                        {
                            let mut ls = last_seen_clone.lock().unwrap();
                            ls.insert(p.id.clone(), Instant::now());
                        }

                        // Validate movement based on previous stored state (if available)
                        let mut to_store = p.clone();
                        let mut send_correction: Option<serde_json::Value> = None;

                        if let Some(prev) = world.players.get(&p.id) {
                            if let (Some(prev_x), Some(prev_y), Some(prev_z), Some(prev_ts), Some(new_ts)) = (
                                prev.x,
                                prev.y,
                                prev.z,
                                prev.ts,
                                p.ts,
                            ) {
                                // compute dt in seconds
                                let dt_ms = if new_ts > prev_ts { new_ts - prev_ts } else { 0 };
                                let dt = (dt_ms as f64) / 1000.0;
                                if dt > 0.0 && dt < 60.0 {
                                    // compute reported speed magnitude
                                    let svx = p.vx.unwrap_or(0.0);
                                    let svy = p.vy.unwrap_or(0.0);
                                    let svz = p.vz.unwrap_or(0.0);
                                    let _speed = (svx * svx + svy * svy + svz * svz).sqrt();

                                    // expected displacement (using reported velocity)
                                    let expect_dx = svx * dt;
                                    let expect_dy = svy * dt;
                                    let expect_dz = svz * dt;
                                    let expect_dist = (expect_dx * expect_dx + expect_dy * expect_dy + expect_dz * expect_dz).sqrt();

                                    // actual displacement from previous to reported
                                    let dx = p.x.unwrap_or(prev_x) - prev_x;
                                    let dy = p.y.unwrap_or(prev_y) - prev_y;
                                    let dz = p.z.unwrap_or(prev_z) - prev_z;
                                    let actual_dist = (dx * dx + dy * dy + dz * dz).sqrt();

                                    // tolerance (meters)
                                    let tol = 0.5;

                                    if actual_dist > expect_dist + tol {
                                        // movement invalid — prepare correction using reported velocity applied to prev position
                                        let corrected_x = prev_x + expect_dx;
                                        let corrected_y = prev_y + expect_dy;
                                        let corrected_z = prev_z + expect_dz;

                                        to_store.x = Some(corrected_x);
                                        to_store.y = Some(corrected_y);
                                        to_store.z = Some(corrected_z);
                                        // keep server-side ts as client's ts to maintain timeline
                                        to_store.ts = p.ts;

                                        let corr = json!({
                                            "action": "correction",
                                            "reason": "invalid_movement",
                                            "corrected": {
                                                "id": p.id.clone(),
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

                        // Update world and clients (store corrected or provided state)
                        world.players.insert(p.id.clone(), to_store.clone());
                        clients.insert(p.id.clone(), src);
                        println!("Received {} update", p.id);

                        // send correction to originator if needed
                        if let Some(c) = send_correction {
                            let _ = socket_clone.send_to(c.to_string().as_bytes(), src);
                        }

                        // broadcast world
                        let state = json!({"players": world.players});
                        let s = state.to_string();
                        for addr in clients.values() {
                            let _ = socket_clone.send_to(s.as_bytes(), addr);
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
