use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use uuid::Uuid;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct PlayerState {
    pub uuid: Uuid,
    pub username: String,
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
    pub players: HashMap<Uuid, PlayerState>,
}

/// UUID 持久化存储结构
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct UuidStorage {
    /// 记录所有见过的 UUID 及其对应的用户名
    pub uuids: HashMap<Uuid, String>,
}

impl UuidStorage {
    /// 从文件加载 UUID 存储
    pub fn load_from_file(path: &str) -> std::io::Result<Self> {
        if Path::new(path).exists() {
            let content = fs::read_to_string(path)?;
            match serde_json::from_str(&content) {
                Ok(storage) => Ok(storage),
                Err(_) => Ok(UuidStorage {
                    uuids: HashMap::new(),
                }),
            }
        } else {
            Ok(UuidStorage {
                uuids: HashMap::new(),
            })
        }
    }

    /// 保存 UUID 存储到文件
    pub fn save_to_file(&self, path: &str) -> std::io::Result<()> {
        let json = serde_json::to_string_pretty(&self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        fs::write(path, json)
    }

    /// 添加或更新 UUID
    pub fn add_uuid(&mut self, uuid: Uuid, username: String) {
        self.uuids.insert(uuid, username);
    }

    /// 检查 UUID 是否存在
    pub fn contains_uuid(&self, uuid: &Uuid) -> bool {
        self.uuids.contains_key(uuid)
    }

    /// 获取 UUID 对应的用户名
    pub fn get_username(&self, uuid: &Uuid) -> Option<String> {
        self.uuids.get(uuid).cloned()
    }
}

/// 生成唯一的用户名（当请求的名字已被占用时）
/// 
/// 算法：依次尝试 "base_1", "base_2", ... "base_9999"，直到找到未被占用的名字
/// 如果全部用尽，使用 "base_fallback" 作为最后的备选
pub fn generate_unique_name(world: &HashMap<Uuid, PlayerState>, base: &str) -> String {
    for i in 1..10000 {
        let candidate = format!("{}_{}", base, i);
        if !world.values().any(|p| p.username == candidate) {
            return candidate;
        }
    }
    format!("{}_fallback", base)
}

/// 位置验证结果
#[derive(Debug, Clone)]
pub struct MovementValidation {
    /// 是否通过验证
    pub is_valid: bool,
    /// 如果违规，纠正后的坐标
    pub corrected_x: Option<f64>,
    pub corrected_y: Option<f64>,
    pub corrected_z: Option<f64>,
}

/// 验证玩家的移动是否合理（反作弊检查）
/// 
/// 规则：
/// - 时间差必须在 (0, 60) 秒之间（否则跳过检查）
/// - 实际位移 <= 期望位移 + 容差(0.5米)
/// - 期望位移 = sqrt(vx² + vy² + vz²) * dt
/// 
/// 参数：
/// - prev_x, prev_y, prev_z: 前一次的位置
/// - prev_ts: 前一次的时间戳（毫秒）
/// - new_x, new_y, new_z: 新位置
/// - new_ts: 新时间戳（毫秒）
/// - vx, vy, vz: 报告的速度（m/s）
/// 
/// 返回：
/// - 若验证通过：is_valid=true，无纠正坐标
/// - 若检测到违规：is_valid=false，包含纠正后的坐标
pub fn validate_movement(
    prev_x: f64,
    prev_y: f64,
    prev_z: f64,
    prev_ts: u128,
    new_x: f64,
    new_y: f64,
    new_z: f64,
    new_ts: u128,
    vx: f64,
    vy: f64,
    vz: f64,
) -> MovementValidation {
    const TOLERANCE: f64 = 0.5; // 米
    const MAX_DT_MS: u128 = 60000; // 60秒

    // 计算时间差
    let dt_ms = if new_ts > prev_ts {
        new_ts - prev_ts
    } else {
        0
    };

    // 时间差必须在合理范围内
    if dt_ms == 0 || dt_ms >= MAX_DT_MS {
        return MovementValidation {
            is_valid: true,
            corrected_x: None,
            corrected_y: None,
            corrected_z: None,
        };
    }

    let dt = (dt_ms as f64) / 1000.0;

    // 期望位移距离
    let expect_dx = vx * dt;
    let expect_dy = vy * dt;
    let expect_dz = vz * dt;
    let expect_dist = (expect_dx * expect_dx + expect_dy * expect_dy + expect_dz * expect_dz).sqrt();

    // 实际位移距离
    let dx = new_x - prev_x;
    let dy = new_y - prev_y;
    let dz = new_z - prev_z;
    let actual_dist = (dx * dx + dy * dy + dz * dz).sqrt();

    // 检查是否违规
    if actual_dist > expect_dist + TOLERANCE {
        // 纠正为期望位置
        let corrected_x = prev_x + expect_dx;
        let corrected_y = prev_y + expect_dy;
        let corrected_z = prev_z + expect_dz;

        MovementValidation {
            is_valid: false,
            corrected_x: Some(corrected_x),
            corrected_y: Some(corrected_y),
            corrected_z: Some(corrected_z),
        }
    } else {
        MovementValidation {
            is_valid: true,
            corrected_x: None,
            corrected_y: None,
            corrected_z: None,
        }
    }
}
