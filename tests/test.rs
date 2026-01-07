use backend_demo::{generate_unique_name, validate_movement, PlayerState, WorldState};
use std::collections::HashMap;
use uuid::Uuid;
use std::fs;
use std::net::UdpSocket;
use std::time::{Duration, Instant};
use serde_json::{json, Value};

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

// ============================================================================
// 用户名生成测试
// ============================================================================

#[test]
fn test_generate_unique_name_empty_world() {
    let world: HashMap<Uuid, PlayerState> = HashMap::new();
    let name = generate_unique_name(&world, "player");
    assert_eq!(name, "player_1");
}

#[test]
fn test_generate_unique_name_some_taken() {
    let mut world: HashMap<Uuid, PlayerState> = HashMap::new();
    world.insert(Uuid::new_v4(), empty_player("foo_1"));
    world.insert(Uuid::new_v4(), empty_player("foo_2"));
    let name = generate_unique_name(&world, "foo");
    assert_eq!(name, "foo_3");
}

#[test]
fn test_generate_unique_name_gap_in_sequence() {
    let mut world: HashMap<Uuid, PlayerState> = HashMap::new();
    world.insert(Uuid::new_v4(), empty_player("bar_1"));
    world.insert(Uuid::new_v4(), empty_player("bar_3"));
    world.insert(Uuid::new_v4(), empty_player("bar_5"));
    let name = generate_unique_name(&world, "bar");
    assert_eq!(name, "bar_2"); // 应该找到第一个空缺
}

#[test]
fn test_generate_unique_name_fallback() {
    let mut world: HashMap<Uuid, PlayerState> = HashMap::new();
    for i in 1..10000 {
        let key = format!("bar_{}", i);
        world.insert(Uuid::new_v4(), empty_player(&key));
    }
    let name = generate_unique_name(&world, "bar");
    assert_eq!(name, "bar_fallback");
}

#[test]
fn test_generate_unique_name_different_prefixes() {
    let mut world: HashMap<Uuid, PlayerState> = HashMap::new();
    world.insert(Uuid::new_v4(), empty_player("alpha_1"));
    world.insert(Uuid::new_v4(), empty_player("beta_1"));
    let name_alpha = generate_unique_name(&world, "alpha");
    let name_beta = generate_unique_name(&world, "beta");
    assert_eq!(name_alpha, "alpha_2");
    assert_eq!(name_beta, "beta_2");
}

#[test]
fn test_generate_unique_name_special_characters() {
    let mut world: HashMap<Uuid, PlayerState> = HashMap::new();
    world.insert(Uuid::new_v4(), empty_player("player@_1"));
    let name = generate_unique_name(&world, "player@");
    assert_eq!(name, "player@_2");
}

#[test]
fn test_generate_unique_name_empty_prefix() {
    let world: HashMap<Uuid, PlayerState> = HashMap::new();
    let name = generate_unique_name(&world, "");
    assert_eq!(name, "_1");
}

// ============================================================================
// 位置验证测试（反作弊）
// ============================================================================

#[test]
fn test_validate_movement_valid_linear_motion() {
    // 从 (0,0,0) 移动到 (10,0,0)，速度 10 m/s，时间 1 秒
    let result = validate_movement(
        0.0, 0.0, 0.0, // 前一位置
        1000,           // 前一时间戳（毫秒）
        10.0, 0.0, 0.0, // 新位置
        2000,           // 新时间戳（毫秒）
        10.0, 0.0, 0.0, // 速度（m/s）
    );
    assert!(result.is_valid);
    assert!(result.corrected_x.is_none());
}

#[test]
fn test_validate_movement_stationary() {
    // 玩家静止不动，位置不变
    let result = validate_movement(
        100.0, 200.0, 300.0, // 前一位置
        5000,                 // 前一时间戳
        100.0, 200.0, 300.0, // 新位置（相同）
        6000,                 // 新时间戳
        0.0, 0.0, 0.0,        // 速度为 0
    );
    assert!(result.is_valid);
}

#[test]
fn test_validate_movement_zero_time_delta() {
    // 时间戳相同（dt=0），应该跳过验证
    let result = validate_movement(
        0.0, 0.0, 0.0, // 前一位置
        1000,           // 前一时间戳
        1000.0, 1000.0, 1000.0, // 新位置（极端移动）
        1000,           // 新时间戳（相同）
        0.0, 0.0, 0.0,  // 速度
    );
    assert!(result.is_valid); // 时间差为 0，应该通过
}

#[test]
fn test_validate_movement_negative_time_delta() {
    // 时间戳倒序（客户端时间不准确），应该跳过验证
    let result = validate_movement(
        0.0, 0.0, 0.0, // 前一位置
        2000,           // 前一时间戳
        1000.0, 0.0, 0.0, // 新位置
        1000,           // 新时间戳（更小）
        0.0, 0.0, 0.0,  // 速度
    );
    assert!(result.is_valid); // dt 被设为 0，应该通过
}

#[test]
fn test_validate_movement_time_delta_too_large() {
    // 时间差超过 60 秒，应该跳过验证
    let result = validate_movement(
        0.0, 0.0, 0.0,   // 前一位置
        0,                // 前一时间戳
        10000.0, 0.0, 0.0, // 新位置（极端移动）
        70000,            // 新时间戳（70秒）
        0.0, 0.0, 0.0,    // 速度
    );
    assert!(result.is_valid); // 超过 60 秒，应该跳过验证
}

#[test]
fn test_validate_movement_cheating_teleport() {
    // 玩家瞬移：从 (0,0,0) 到 (100,0,0)，速度 10 m/s，时间 1 秒（不可能）
    let result = validate_movement(
        0.0, 0.0, 0.0, // 前一位置
        0,              // 前一时间戳
        100.0, 0.0, 0.0, // 新位置（瞬移）
        1000,           // 新时间戳（1秒）
        10.0, 0.0, 0.0, // 速度只有 10 m/s
    );
    assert!(!result.is_valid); // 应该检测到作弊
    assert!(result.corrected_x.is_some());
    // 期望位置：0 + 10 * 1 = 10
    assert_eq!(result.corrected_x.unwrap(), 10.0);
    assert_eq!(result.corrected_y.unwrap(), 0.0);
    assert_eq!(result.corrected_z.unwrap(), 0.0);
}

#[test]
fn test_validate_movement_tolerance_boundary() {
    // 测试容差边界：恰好在容差内
    // 期望移动 10 米，实际移动 10.4 米（容差 0.5 米，通过）
    let result = validate_movement(
        0.0, 0.0, 0.0, // 前一位置
        0,              // 前一时间戳
        10.4, 0.0, 0.0, // 新位置（超过 10 但在容差内）
        1000,           // 新时间戳（1秒）
        10.0, 0.0, 0.0, // 速度 10 m/s
    );
    assert!(result.is_valid); // 10.4 <= 10 + 0.5
}

#[test]
fn test_validate_movement_tolerance_exceeded() {
    // 测试容差边界：超出容差
    // 期望移动 10 米，实际移动 10.6 米（超过容差 0.5 米，失败）
    let result = validate_movement(
        0.0, 0.0, 0.0, // 前一位置
        0,              // 前一时间戳
        10.6, 0.0, 0.0, // 新位置
        1000,           // 新时间戳（1秒）
        10.0, 0.0, 0.0, // 速度 10 m/s
    );
    assert!(!result.is_valid); // 10.6 > 10 + 0.5
}

#[test]
fn test_validate_movement_3d_motion() {
    // 三维运动：沿对角线移动
    // 速度 (10, 10, 10) m/s，时间 1 秒
    // 期望距离 = sqrt(10² + 10² + 10²) = sqrt(300) ≈ 17.32 米
    let result = validate_movement(
        0.0, 0.0, 0.0,    // 前一位置
        0,                 // 前一时间戳
        10.0, 10.0, 10.0, // 新位置
        1000,              // 新时间戳（1秒）
        10.0, 10.0, 10.0,  // 速度
    );
    assert!(result.is_valid); // 应该精确匹配
}

#[test]
fn test_validate_movement_small_motion() {
    // 极小的运动
    let result = validate_movement(
        0.0, 0.0, 0.0,       // 前一位置
        0,                    // 前一时间戳
        0.001, 0.0, 0.0,     // 新位置（1mm）
        100,                  // 新时间戳（100ms）
        0.01, 0.0, 0.0,      // 速度（0.01 m/s = 1cm/s）
    );
    assert!(result.is_valid);
}

#[test]
fn test_validate_movement_negative_velocity() {
    // 反向速度（向后移动）
    let result = validate_movement(
        10.0, 0.0, 0.0,  // 前一位置
        0,                // 前一时间戳
        0.0, 0.0, 0.0,   // 新位置（向后移动 10 米）
        1000,             // 新时间戳（1秒）
        -10.0, 0.0, 0.0, // 负速度
    );
    assert!(result.is_valid);
}

#[test]
fn test_validate_movement_mixed_velocity_signs() {
    // 混合正负速度
    let result = validate_movement(
        0.0, 0.0, 0.0,     // 前一位置
        0,                  // 前一时间戳
        10.0, -5.0, 0.0,   // 新位置
        1000,               // 新时间戳（1秒）
        10.0, -5.0, 0.0,   // 速度
    );
    assert!(result.is_valid);
}

#[test]
fn test_validate_movement_very_high_speed() {
    // 非常高的速度（物理上不现实，但在游戏中可能有超能力）
    let result = validate_movement(
        0.0, 0.0, 0.0,       // 前一位置
        0,                    // 前一时间戳
        1000.0, 0.0, 0.0,    // 新位置
        1000,                 // 新时间戳（1秒）
        1000.0, 0.0, 0.0,    // 速度 1000 m/s
    );
    assert!(result.is_valid); // 报告的速度与实际相符
}

#[test]
fn test_validate_movement_fractional_second() {
    // 分数秒的运动（如 0.5 秒）
    let result = validate_movement(
        0.0, 0.0, 0.0,  // 前一位置
        0,               // 前一时间戳
        5.0, 0.0, 0.0,  // 新位置
        500,             // 新时间戳（0.5 秒）
        10.0, 0.0, 0.0, // 速度 10 m/s
    );
    assert!(result.is_valid); // 期望 10 * 0.5 = 5 米
}

#[test]
fn test_validate_movement_floating_point_precision() {
    // 浮点数精度问题
    let result = validate_movement(
        0.0, 0.0, 0.0,                   // 前一位置
        0,                                // 前一时间戳
        0.1 + 0.2, 0.0, 0.0,             // 新位置（0.1 + 0.2 = 0.30000000000000004）
        1000,                             // 新时间戳（1秒）
        0.30000000000000004, 0.0, 0.0,   // 精确速度
    );
    assert!(result.is_valid);
}

#[test]
fn test_validate_movement_large_coordinates() {
    // 非常大的坐标
    let result = validate_movement(
        1e6, 2e6, 3e6,        // 前一位置
        0,                     // 前一时间戳
        1e6 + 10.0, 2e6, 3e6, // 新位置
        1000,                  // 新时间戳（1秒）
        10.0, 0.0, 0.0,       // 速度
    );
    assert!(result.is_valid);
}

#[test]
fn test_validate_movement_negative_coordinates() {
    // 负坐标
    let result = validate_movement(
        -100.0, -200.0, -300.0, // 前一位置
        0,                        // 前一时间戳
        -90.0, -200.0, -300.0,   // 新位置
        1000,                     // 新时间戳
        10.0, 0.0, 0.0,          // 速度
    );
    assert!(result.is_valid);
}

// ============================================================================
// PlayerState 和 WorldState 测试
// ============================================================================

#[test]
fn test_player_state_serialization() {
    let uuid = Uuid::new_v4();
    let player = PlayerState {
        uuid,
        username: "fighter_alpha".to_string(),
        x: Some(100.5),
        y: Some(200.0),
        z: Some(-50.3),
        ts: Some(1704556800000),
        rx: Some(0.0),
        ry: Some(45.0),
        rz: Some(0.0),
        vx: Some(10.5),
        vy: Some(0.0),
        vz: Some(-5.2),
        action: Some("firing".to_string()),
    };

    let json = serde_json::to_string(&player).unwrap();
    let deserialized: PlayerState = serde_json::from_str(&json).unwrap();

    assert_eq!(player.uuid, deserialized.uuid);
    assert_eq!(player.username, deserialized.username);
    assert_eq!(player.x, deserialized.x);
    assert_eq!(player.ts, deserialized.ts);
    assert_eq!(player.action, deserialized.action);
}

#[test]
fn test_player_state_partial_fields() {
    let uuid = Uuid::new_v4();
    let player = PlayerState {
        uuid,
        username: "partial_player".to_string(),
        x: Some(10.0),
        y: None,
        z: None,
        ts: Some(1000),
        rx: None,
        ry: None,
        rz: None,
        vx: None,
        vy: None,
        vz: None,
        action: None,
    };

    let json = serde_json::to_string(&player).unwrap();
    let deserialized: PlayerState = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.x, Some(10.0));
    assert_eq!(deserialized.y, None);
    assert!(deserialized.action.is_none());
}

#[test]
fn test_world_state_multiple_players() {
    let mut world = WorldState {
        players: HashMap::new(),
    };

    let uuid1 = Uuid::new_v4();
    let uuid2 = Uuid::new_v4();

    world.players.insert(
        uuid1,
        PlayerState {
            uuid: uuid1,
            username: "player1".to_string(),
            x: Some(0.0),
            y: Some(0.0),
            z: Some(0.0),
            ts: Some(1000),
            rx: None,
            ry: None,
            rz: None,
            vx: None,
            vy: None,
            vz: None,
            action: None,
        },
    );

    world.players.insert(
        uuid2,
        PlayerState {
            uuid: uuid2,
            username: "player2".to_string(),
            x: Some(10.0),
            y: Some(10.0),
            z: Some(10.0),
            ts: Some(2000),
            rx: None,
            ry: None,
            rz: None,
            vx: None,
            vy: None,
            vz: None,
            action: None,
        },
    );

    assert_eq!(world.players.len(), 2);
    assert!(world.players.contains_key(&uuid1));
    assert!(world.players.contains_key(&uuid2));
}

// ============================================================================
// 边界情况和极限值测试
// ============================================================================

#[test]
fn test_uuid_uniqueness() {
    let uuid1 = Uuid::new_v4();
    let uuid2 = Uuid::new_v4();
    assert_ne!(uuid1, uuid2);
}

#[test]
fn test_username_max_length() {
    let long_name = "a".repeat(1000);
    let mut world: HashMap<Uuid, PlayerState> = HashMap::new();
    world.insert(Uuid::new_v4(), empty_player(&long_name));

    // 应该能处理非常长的用户名
    assert!(world.values().any(|p| p.username.len() == 1000));
}

#[test]
fn test_generate_unique_name_with_unicode() {
    let mut world: HashMap<Uuid, PlayerState> = HashMap::new();
    world.insert(Uuid::new_v4(), empty_player("玩家_1"));
    let name = generate_unique_name(&world, "玩家");
    assert_eq!(name, "玩家_2");
}

#[test]
fn test_movement_validation_boundary_exactly_at_limit() {
    // dt 恰好 60000 毫秒（60 秒）
    let result = validate_movement(
        0.0, 0.0, 0.0, // 前一位置
        0,              // 前一时间戳
        100.0, 0.0, 0.0, // 新位置
        60000,          // 新时间戳（恰好 60 秒）
        100.0, 0.0, 0.0, // 速度
    );
    // dt == 60000 时，应该跳过验证（因为 dt >= MAX_DT_MS）
    assert!(result.is_valid);
}

#[test]
fn test_movement_validation_boundary_just_under_limit() {
    // dt 恰好 59999 毫秒（略小于 60 秒）
    let result = validate_movement(
        0.0, 0.0, 0.0,      // 前一位置
        0,                   // 前一时间戳
        10000.0, 0.0, 0.0,  // 新位置（极端移动）
        59999,               // 新时间戳
        10.0, 0.0, 0.0,     // 实际速度无法达到这个移动
    );
    assert!(!result.is_valid); // 应该进行验证并检测到作弊
}

// ============================================================================
// 世界状态持久化测试（新优化逻辑）
// ============================================================================

#[test]
fn test_world_state_serialization() {
    let mut world = WorldState {
        players: HashMap::new(),
    };
    
    let uuid1 = Uuid::new_v4();
    let uuid2 = Uuid::new_v4();
    
    world.players.insert(uuid1, empty_player("player_1"));
    world.players.insert(uuid2, empty_player("player_2"));
    
    // 序列化
    let json = serde_json::to_string(&world).expect("Failed to serialize");
    
    // 反序列化
    let loaded: WorldState = serde_json::from_str(&json).expect("Failed to deserialize");
    
    assert_eq!(loaded.players.len(), 2);
    assert!(loaded.players.contains_key(&uuid1));
    assert!(loaded.players.contains_key(&uuid2));
}

#[test]
fn test_world_state_file_persistence() {
    let test_file = "test_world_state.json";
    
    // 创建世界状态
    let mut world = WorldState {
        players: HashMap::new(),
    };
    let uuid = Uuid::new_v4();
    world.players.insert(uuid, empty_player("persistent_player"));
    
    // 保存到文件
    let json = serde_json::to_string_pretty(&world).expect("Failed to serialize");
    fs::write(test_file, json).expect("Failed to write file");
    
    // 从文件加载
    let content = fs::read_to_string(test_file).expect("Failed to read file");
    let loaded: WorldState = serde_json::from_str(&content).expect("Failed to deserialize");
    
    // 验证
    assert_eq!(loaded.players.len(), 1);
    assert_eq!(loaded.players.get(&uuid).unwrap().username, "persistent_player");
    
    // 清理
    let _ = fs::remove_file(test_file);
}

// ============================================================================
// 在线状态判断测试（基于 last_seen）
// ============================================================================

#[test]
fn test_online_detection_by_last_seen() {
    let mut last_seen: HashMap<Uuid, Instant> = HashMap::new();
    let uuid_online = Uuid::new_v4();
    let uuid_offline = Uuid::new_v4();
    
    let now = Instant::now();
    
    // 在线玩家：刚刚活跃
    last_seen.insert(uuid_online, now);
    
    // 离线玩家：60秒前活跃
    last_seen.insert(uuid_offline, now - Duration::from_secs(61));
    
    // 判断在线状态
    let is_online = |uuid: &Uuid| {
        last_seen.get(uuid)
            .map(|&t| now.duration_since(t).as_secs() < 60)
            .unwrap_or(false)
    };
    
    assert!(is_online(&uuid_online));
    assert!(!is_online(&uuid_offline));
}

#[test]
fn test_filter_online_players() {
    let mut world: HashMap<Uuid, PlayerState> = HashMap::new();
    let mut last_seen: HashMap<Uuid, Instant> = HashMap::new();
    
    let uuid_online = Uuid::new_v4();
    let uuid_offline = Uuid::new_v4();
    let uuid_never_active = Uuid::new_v4();
    
    world.insert(uuid_online, empty_player("online_player"));
    world.insert(uuid_offline, empty_player("offline_player"));
    world.insert(uuid_never_active, empty_player("never_active"));
    
    let now = Instant::now();
    last_seen.insert(uuid_online, now);
    last_seen.insert(uuid_offline, now - Duration::from_secs(61));
    // uuid_never_active 没有 last_seen 记录
    
    // 过滤在线玩家
    let online_players: Vec<Uuid> = world
        .keys()
        .filter(|uuid| {
            last_seen.get(uuid)
                .map(|&t| now.duration_since(t).as_secs() < 60)
                .unwrap_or(false)
        })
        .cloned()
        .collect();
    
    assert_eq!(online_players.len(), 1);
    assert!(online_players.contains(&uuid_online));
    assert!(!online_players.contains(&uuid_offline));
    assert!(!online_players.contains(&uuid_never_active));
}

#[test]
fn test_player_resume_from_world() {
    let mut world = WorldState {
        players: HashMap::new(),
    };
    
    let uuid = Uuid::new_v4();
    let mut player = empty_player("resumable_player");
    player.uuid = uuid;
    player.x = Some(100.0);
    player.y = Some(200.0);
    player.z = Some(300.0);
    
    world.players.insert(uuid, player.clone());
    
    // 模拟玩家恢复
    let resumed = world.players.get(&uuid);
    assert!(resumed.is_some());
    
    let resumed_player = resumed.unwrap();
    assert_eq!(resumed_player.username, "resumable_player");
    assert_eq!(resumed_player.x, Some(100.0));
    assert_eq!(resumed_player.y, Some(200.0));
    assert_eq!(resumed_player.z, Some(300.0));
}

// ============================================================================
// 性能测试：在线判断
// ============================================================================

#[test]
fn test_online_check_performance() {
    let mut last_seen: HashMap<Uuid, Instant> = HashMap::new();
    let now = Instant::now();
    
    // 创建 1000 个玩家
    for _ in 0..1000 {
        let uuid = Uuid::new_v4();
        // 随机分配在线/离线状态
        let offset = (uuid.as_u128() % 120) as u64;
        last_seen.insert(uuid, now - Duration::from_secs(offset));
    }
    
    // 测试判断速度
    let start = Instant::now();
    let online_count = last_seen
        .iter()
        .filter(|(_, &t)| now.duration_since(t).as_secs() < 60)
        .count();
    let elapsed = start.elapsed();
    
    println!("在线判断 1000 个玩家耗时: {:?}", elapsed);
    assert!(elapsed < Duration::from_millis(10)); // 应该很快
    assert!(online_count > 0 && online_count < 1000);
}

// ============================================================================
// UUID 恢复逻辑集成测试
// ============================================================================

/// 辅助函数：创建测试用的 UDP socket 并发送消息
fn send_and_receive(message: Value, timeout_secs: u64) -> Result<Value, String> {
    let socket = UdpSocket::bind("127.0.0.1:0").map_err(|e| format!("Bind failed: {}", e))?;
    socket
        .set_read_timeout(Some(Duration::from_secs(timeout_secs)))
        .map_err(|e| format!("Set timeout failed: {}", e))?;

    let server_addr = "127.0.0.1:8888";
    let msg_str = message.to_string();
    socket
        .send_to(msg_str.as_bytes(), server_addr)
        .map_err(|e| format!("Send failed: {}", e))?;

    let mut buf = [0u8; 4096];
    match socket.recv_from(&mut buf) {
        Ok((n, _)) => {
            let response = String::from_utf8_lossy(&buf[..n]);
            serde_json::from_str(&response).map_err(|e| format!("Parse failed: {}", e))
        }
        Err(e) => Err(format!("Receive failed: {}", e)),
    }
}

#[test]
#[ignore] // 需要运行服务器才能测试
fn test_uuid_not_found() {
    // 测试：提供一个不存在的 UUID，不提供用户名
    let fake_uuid = "00000000-0000-0000-0000-000000000001";
    let request = json!({
        "type": "register",
        "uuid": fake_uuid
    });

    match send_and_receive(request, 2) {
        Ok(response) => {
            assert_eq!(
                response.get("action").and_then(|v| v.as_str()),
                Some("uuid_not_found"),
                "服务器应该返回 uuid_not_found"
            );
            assert_eq!(
                response.get("uuid").and_then(|v| v.as_str()),
                Some(fake_uuid),
                "响应应该包含原始 UUID"
            );
        }
        Err(e) => panic!("测试失败: {}", e),
    }
}

#[test]
#[ignore] // 需要运行服务器才能测试
fn test_username_required() {
    // 测试：既不提供 UUID 也不提供用户名
    let request = json!({
        "type": "register"
    });

    match send_and_receive(request, 2) {
        Ok(response) => {
            assert_eq!(
                response.get("action").and_then(|v| v.as_str()),
                Some("username_required"),
                "服务器应该返回 username_required"
            );
        }
        Err(e) => panic!("测试失败: {}", e),
    }
}

#[test]
#[ignore] // 需要运行服务器才能测试
fn test_normal_registration() {
    // 测试：正常注册（提供用户名）
    let username = format!("test_user_{}", std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs());
    
    let request = json!({
        "type": "register",
        "username": username
    });

    match send_and_receive(request, 2) {
        Ok(response) => {
            assert_eq!(
                response.get("action").and_then(|v| v.as_str()),
                Some("registered"),
                "服务器应该返回 registered"
            );
            assert!(
                response.get("uuid").is_some(),
                "响应应该包含 UUID"
            );
            assert_eq!(
                response.get("username").and_then(|v| v.as_str()),
                Some(username.as_str()),
                "响应应该包含用户名"
            );
        }
        Err(e) => panic!("测试失败: {}", e),
    }
}

#[test]
#[ignore] // 需要运行服务器才能测试
fn test_valid_uuid_resume() {
    // 测试：先注册，然后使用有效的 UUID 恢复
    let username = format!("resume_test_{}", std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs());
    
    // 第一步：注册
    let register_request = json!({
        "type": "register",
        "username": username
    });

    let uuid = match send_and_receive(register_request, 2) {
        Ok(response) => {
            response.get("uuid")
                .and_then(|v| v.as_str())
                .expect("应该返回 UUID")
                .to_string()
        }
        Err(e) => panic!("注册失败: {}", e),
    };

    // 第二步：使用 UUID 恢复
    let resume_request = json!({
        "type": "register",
        "uuid": uuid
    });

    match send_and_receive(resume_request, 2) {
        Ok(response) => {
            assert_eq!(
                response.get("action").and_then(|v| v.as_str()),
                Some("registered"),
                "服务器应该返回 registered"
            );
            assert_eq!(
                response.get("resumed").and_then(|v| v.as_bool()),
                Some(true),
                "响应应该标记为 resumed"
            );
            assert_eq!(
                response.get("username").and_then(|v| v.as_str()),
                Some(username.as_str()),
                "响应应该包含原始用户名"
            );
        }
        Err(e) => panic!("恢复测试失败: {}", e),
    }
}

#[test]
#[ignore] // 需要运行服务器才能测试
fn test_malformed_uuid() {
    // 测试：提供格式错误的 UUID
    let request = json!({
        "type": "register",
        "uuid": "this-is-not-a-valid-uuid"
    });

    match send_and_receive(request, 2) {
        Ok(response) => {
            // 格式错误的 UUID 会被解析失败，服务器会要求提供用户名
            assert_eq!(
                response.get("action").and_then(|v| v.as_str()),
                Some("username_required"),
                "服务器应该返回 username_required（因为 UUID 解析失败）"
            );
        }
        Err(e) => panic!("测试失败: {}", e),
    }
}

#[test]
#[ignore] // 需要运行服务器才能测试
fn test_uuid_with_username_invalid_uuid() {
    // 测试：同时提供 UUID 和用户名，但 UUID 不存在
    // 服务器应该优先检查 UUID，返回 uuid_not_found
    let fake_uuid = "11111111-1111-1111-1111-111111111111";
    let request = json!({
        "type": "register",
        "uuid": fake_uuid,
        "username": "should_not_be_used"
    });

    match send_and_receive(request, 2) {
        Ok(response) => {
            assert_eq!(
                response.get("action").and_then(|v| v.as_str()),
                Some("uuid_not_found"),
                "服务器应该优先检查 UUID，返回 uuid_not_found"
            );
        }
        Err(e) => panic!("测试失败: {}", e),
    }
}
