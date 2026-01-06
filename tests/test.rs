use backend_demo::{generate_unique_name, validate_movement, PlayerState, WorldState, UuidStorage};
use std::collections::HashMap;
use uuid::Uuid;
use std::fs;

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

#[test]
fn test_world_state_serialization() {
    let mut world = WorldState {
        players: HashMap::new(),
    };

    let uuid = Uuid::new_v4();
    world.players.insert(
        uuid,
        PlayerState {
            uuid,
            username: "test".to_string(),
            x: Some(1.0),
            y: Some(2.0),
            z: Some(3.0),
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

    let json = serde_json::to_string(&world).unwrap();
    let deserialized: WorldState = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.players.len(), 1);
    assert!(deserialized.players.contains_key(&uuid));
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
// UUID 持久化存储测试
// ============================================================================

#[test]
fn test_uuid_storage_add_and_retrieve() {
    let mut storage = UuidStorage {
        uuids: HashMap::new(),
    };
    let uuid = Uuid::new_v4();
    storage.add_uuid(uuid, "player_test".to_string());
    
    assert!(storage.contains_uuid(&uuid));
    assert_eq!(storage.get_username(&uuid), Some("player_test".to_string()));
}

#[test]
fn test_uuid_storage_multiple_entries() {
    let mut storage = UuidStorage {
        uuids: HashMap::new(),
    };
    let uuid1 = Uuid::new_v4();
    let uuid2 = Uuid::new_v4();
    
    storage.add_uuid(uuid1, "player_1".to_string());
    storage.add_uuid(uuid2, "player_2".to_string());
    
    assert_eq!(storage.get_username(&uuid1), Some("player_1".to_string()));
    assert_eq!(storage.get_username(&uuid2), Some("player_2".to_string()));
}

#[test]
fn test_uuid_storage_file_persistence() {
    let test_file = "test_uuid_storage.json";
    
    // 创建和保存存储
    {
        let mut storage = UuidStorage {
            uuids: HashMap::new(),
        };
        let uuid = Uuid::new_v4();
        storage.add_uuid(uuid, "offline_player".to_string());
        storage.save_to_file(test_file).expect("Failed to save");
    }
    
    // 从文件加载
    let loaded = UuidStorage::load_from_file(test_file).expect("Failed to load");
    
    // 验证数据完整性
    assert_eq!(loaded.uuids.len(), 1);
    
    // 清理测试文件
    let _ = fs::remove_file(test_file);
}

#[test]
fn test_uuid_storage_load_nonexistent_file() {
    // 加载不存在的文件应该返回空存储
    let storage = UuidStorage::load_from_file("nonexistent_file_xyz.json")
        .expect("Should create empty storage");
    assert_eq!(storage.uuids.len(), 0);
}

#[test]
fn test_uuid_storage_update_existing() {
    let mut storage = UuidStorage {
        uuids: HashMap::new(),
    };
    let uuid = Uuid::new_v4();
    
    storage.add_uuid(uuid, "original_name".to_string());
    // 覆盖相同 UUID 的用户名
    storage.add_uuid(uuid, "updated_name".to_string());
    
    assert_eq!(storage.get_username(&uuid), Some("updated_name".to_string()));
}

// ============================================================================
// 离线状态测试（仅验证数据结构，实际离线通知逻辑在 main.rs）
// ============================================================================

#[test]
fn test_online_status_tracking() {
    let mut online_status: HashMap<Uuid, bool> = HashMap::new();
    let uuid = Uuid::new_v4();
    
    // 玩家上线
    online_status.insert(uuid, true);
    assert_eq!(online_status.get(&uuid).copied(), Some(true));
    
    // 玩家离线
    online_status.insert(uuid, false);
    assert_eq!(online_status.get(&uuid).copied(), Some(false));
}

#[test]
fn test_broadcast_filters_offline_players() {
    let mut world: HashMap<Uuid, PlayerState> = HashMap::new();
    let mut online_status: HashMap<Uuid, bool> = HashMap::new();
    
    let uuid_online = Uuid::new_v4();
    let uuid_offline = Uuid::new_v4();
    
    world.insert(uuid_online, empty_player("online_player"));
    world.insert(uuid_offline, empty_player("offline_player"));
    
    online_status.insert(uuid_online, true);
    online_status.insert(uuid_offline, false);
    
    // 过滤在线玩家（模拟 broadcast_world 的行为）
    let online_players: Vec<Uuid> = world
        .keys()
        .filter(|uuid| online_status.get(uuid).copied().unwrap_or(false))
        .cloned()
        .collect();
    
    assert_eq!(online_players.len(), 1);
    assert!(online_players.contains(&uuid_online));
    assert!(!online_players.contains(&uuid_offline));
}

