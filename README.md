# Rust UDP Server

Vibe 一个最小化的模拟多人在线空战需求的后端 demo

# API Documentation

## 目录

1. [系统概述](#系统概述)
2. [整体架构](#整体架构)
3. [通信协议](#通信协议)
4. [数据结构](#数据结构)
5. [API 消息类型](#api-消息类型)
6. [服务器状态管理](#服务器状态管理)
7. [完整使用示例](#完整使用示例)
8. [错误处理指南](#错误处理指南)
9. [最佳实践](#最佳实践)
10. [性能调优建议](#性能调优建议)

---

## 系统概述

本服务器是一个高性能的多人联机游戏后端，专为**3D 空战游戏**设计。它采用 **UDP 协议**进行实时通信，支持：

- **玩家注册与会话恢复**：基于 UUID 的唯一身份识别，支持断线重连
- **实时状态同步**：玩家位置、旋转、速度等 3D 变换数据
- **反作弊验证**：检测不合理的移动（速度超限），自动纠正客户端位置
- **离线管理**：1 分钟无活动标记离线（不广播位置），UUID 持久化到外部存储
- **会话恢复**：离线玩家通过 UUID 重连时从持久化存储恢复身份，重新进入游戏
- **广播机制**：每次更新后向所有**在线**客户端广播完整世界状态（仅包含在线玩家）

**适用场景**：

- 多人空战、飞行模拟
- 实时 PvP 竞技游戏
- 需要位置验证的 MMO 快速原型

---

## 整体架构

```
┌─────────────────────────────────────────────────────────────┐
│                   Rust UDP 服务器                           │
│                   (127.0.0.1:8888)                          │
└────────┬──────────────────────────┬──────────────────────────┘
         │                          │
    ┌────▼─────┐         ┌──────────▼───────┐
    │  前端/    │         │  不活动检测线程  │
    │ 客户端    │         │  (5秒一轮)       │
    │ (Python   │         │  1分钟不活动     │
    │  游戏引擎) │         │  → 标记离线      │
    └──────────┘         └──────────────────┘
                               │
                               ▼
                        ┌─────────────┐
                        │ 持久化存储  │
                        │ (文件/Redis)│
                        └─────────────┘

┌────────────────────────────────────────────────────────────┐
│  内存数据结构                                               │
├────────────────────────────────────────────────────────────┤
│ • World: HashMap<Uuid, PlayerState>                        │
│   └─ 所有**在线**玩家的完整 3D 状态                          │
│ • OnlineStatus: HashMap<Uuid, bool>                        │
│   └─ 玩家在线状态 (true=在线, false=离线)                    │
│ • Clients: HashMap<Uuid, SocketAddr>                       │
│   └─ 玩家 UUID 到网络地址的映射                              │
│ • UsernameMap: HashMap<String, Uuid>                       │
│   └─ 用户名到 UUID 的索引（快速冲突检测）                     │
│ • LastSeen: HashMap<Uuid, Instant>                         │
│   └─ 玩家最后活动时间戳（用于不活动检测）                     │
└────────────────────────────────────────────────────────────┘
```

**消息流**：

```
客户端                              服务器
  │
  ├─► [register] ──────────────────► 检查 UUID/用户名
  │                                   • UUID 存在且在线？→ 更新地址
  │                                   • UUID 存在但离线？→ 恢复身份
  │                                   • 用户名冲突？→ 建议新名字
  │                                   • 否则→ 分配新 UUID, 创建 PlayerState
  │
  │◄────── [registered] ───────────── 返回 UUID、用户名、历史状态
  │
  ├─► [update] ──────────────────────► 位置/旋转/速度 + 时间戳
  │                                    • 时间戳校验
  │                                    • 速度检查（反作弊）
  │                                    • 若超限→ 发送纠正
  │                                    • 更新在线状态为 true
  │
  │◄────── [broadcast world] ───────── **仅在线玩家**的完整状态
  │        (每次有人更新都触发)
  │
  │ (1 分钟无更新)
  │
  │◄────── [offline] ──────────────── 标记离线，不再广播此玩家
  │        UUID 已保存到持久化存储
  │
  │ (玩家重连，发送 register + UUID)
  │
  │◄────── [registered] ───────────── 返回已保存的身份信息
```

---

## 通信协议

### 基本信息

| 项目           | 值                    |
| -------------- | --------------------- |
| **协议**       | UDP（无连接、低延迟） |
| **地址**       | `127.0.0.1`           |
| **端口**       | `8888`                |
| **编码**       | UTF-8 JSON            |
| **最大包大小** | 2048 字节             |
| **不活动超时** | 60 秒（1 分钟）       |
| **离线标记**   | UUID 存储到持久化存储 |

### 消息格式

所有消息必须是 **有效的 JSON 对象**，包含 `type` 字段：

```json
{
  "type": "register|heartbeat|update",
  ... // 其他字段
}
```

**验证失败时的行为**：

- 非 UTF-8 数据：忽略，控制台输出 "Invalid utf8"
- 非 JSON：忽略，控制台输出 "Invalid json"
- 缺少 `type` 字段：忽略，控制台输出 "Unknown message without type"

---

## 数据结构

### PlayerState（玩家状态）

```json
{
  "uuid": "550e8400-e29b-41d4-a716-446655440000",
  "username": "player_1",
  "x": 100.5,
  "y": 200.0,
  "z": -50.3,
  "ts": 1704556800000,
  "rx": 0.0,
  "ry": 45.0,
  "rz": 0.0,
  "vx": 10.5,
  "vy": 0.0,
  "vz": -5.2,
  "action": "firing"
}
```

| 字段       | 类型             | 必需 | 说明                                     |
| ---------- | ---------------- | ---- | ---------------------------------------- |
| `uuid`     | string (UUID v4) | ✓    | 玩家唯一标识符                           |
| `username` | string           | ✓    | 玩家昵称（可修改，全局唯一）             |
| `x`        | float64          | ✗    | X 轴位置（米）                           |
| `y`        | float64          | ✗    | Y 轴位置（米）                           |
| `z`        | float64          | ✗    | Z 轴位置（米）                           |
| `ts`       | u128             | ✗    | 时间戳（毫秒，客户端端口时间）           |
| `rx`       | float64          | ✗    | X 轴旋转（欧拉角，度数）                 |
| `ry`       | float64          | ✗    | Y 轴旋转（欧拉角，度数）                 |
| `rz`       | float64          | ✗    | Z 轴旋转（欧拉角，度数）                 |
| `vx`       | float64          | ✗    | X 轴速度（m/s）                          |
| `vy`       | float64          | ✗    | Y 轴速度（m/s）                          |
| `vz`       | float64          | ✗    | Z 轴速度（m/s）                          |
| `action`   | string           | ✗    | 自定义动作标签（如 "firing", "evading"） |

**字段值规范**：

- **坐标系**：右手笛卡尔坐标（X 右，Y 上，Z 后）
- **旋转**：欧拉角，单位度数（-180 ~ 180）
- **速度**：米每秒
- **时间**：毫秒（UTC 时间戳或任意递增值）
- **可选字段**：若不更新可省略，服务器保留旧值

---

## API 消息类型

### 1. Register（注册/恢复）

**功能**：

- 新玩家注册并获得 UUID
- 断线玩家通过 UUID 恢复会话

**请求**：

```json
{
  "type": "register",
  "username": "fighter_alpha",
  "uuid": "550e8400-e29b-41d4-a716-446655440000"
}
```

| 字段       | 类型   | 必需 | 说明                     |
| ---------- | ------ | ---- | ------------------------ |
| `type`     | string | ✓    | 必须为 "register"        |
| `username` | string | ✓    | 玩家昵称，长度 1-64 字符 |
| `uuid`     | string | ✗    | UUID（如果是断线恢复）   |

**响应场景 1：恢复成功**

```json
{
  "action": "registered",
  "uuid": "550e8400-e29b-41d4-a716-446655440000",
  "username": "fighter_alpha",
  "state": {
    "uuid": "550e8400-e29b-41d4-a716-446655440000",
    "username": "fighter_alpha",
    "x": 100.5,
    "y": 200.0,
    "z": -50.3,
    "ts": 1704556800000,
    "rx": 0.0,
    "ry": 45.0,
    "rz": 0.0,
    "vx": 10.5,
    "vy": 0.0,
    "vz": -5.2,
    "action": null
  }
}
```

**响应场景 2：新建成功**

```json
{
  "action": "registered",
  "uuid": "650e8400-e29b-41d4-a716-446655440001",
  "username": "fighter_alpha"
}
```

**响应场景 3：用户名冲突**

```json
{
  "action": "name_conflict",
  "suggested": "fighter_alpha_1"
}
```

| 字段        | 说明                                 |
| ----------- | ------------------------------------ |
| `suggested` | 服务器建议的替代名字（原名 + "\_N"） |

**处理流程**：

```
┌─ 有 UUID 且存在？
│  └─ 是 → 恢复历史状态，返回 "registered" + state
│
├─ 用户名被占用？
│  └─ 是 → 返回 "name_conflict" + suggested
│
└─ 都没问题
   └─ 分配新 UUID，创建空 PlayerState，返回 "registered"
```

---

### 2. Update（状态更新）

**功能**：

- 报告玩家的当前 3D 位置、旋转、速度
- 服务器验证移动合理性（反作弊）
- 广播更新给所有玩家

**请求**：

```json
{
  "type": "update",
  "uuid": "550e8400-e29b-41d4-a716-446655440000",
  "x": 105.5,
  "y": 200.0,
  "z": -48.3,
  "ts": 1704556801000,
  "rx": 0.0,
  "ry": 45.0,
  "rz": 0.0,
  "vx": 10.5,
  "vy": 0.0,
  "vz": -5.2,
  "action": "accelerating"
}
```

**必需字段**：

- `type`: "update"
- `uuid`: 玩家 UUID（字符串）

**可选字段**：位置、旋转、速度、时间戳、动作（任意组合）

**服务器处理逻辑**：

1. **查找玩家**：根据 UUID 查找，不存在则忽略
2. **更新时间戳**：记录此次更新时间（用于超时检测）
3. **应用字段**：将请求中的字段覆盖旧值
4. **反作弊检查**：
   - 若有位置 + 时间戳 + 旧位置记录
     - 计算时间差 `dt = (新ts - 旧ts) / 1000` 秒
     - 期望位移：`expect_dist = sqrt(vx² + vy² + vz²) * dt`
     - 实际位移：`actual_dist = sqrt(dx² + dy² + dz²)`
     - 若 `actual_dist > expect_dist + 0.5`，发送纠正消息
5. **广播**：向所有玩家广播更新后的世界状态

**响应（仅在需要纠正时）**：

```json
{
  "action": "correction",
  "reason": "invalid_movement",
  "corrected": {
    "uuid": "550e8400-e29b-41d4-a716-446655440000",
    "username": "fighter_alpha",
    "x": 103.5,
    "y": 200.0,
    "z": -49.3,
    "vx": 10.5,
    "vy": 0.0,
    "vz": -5.2,
    "ts": 1704556801000
  }
}
```

**广播消息**（发送给所有玩家）：

```json
{
  "players": {
    "550e8400-e29b-41d4-a716-446655440000": {
      "uuid": "550e8400-e29b-41d4-a716-446655440000",
      "username": "fighter_alpha",
      "x": 105.5,
      "y": 200.0,
      "z": -48.3,
      "ts": 1704556801000,
      "rx": 0.0,
      "ry": 45.0,
      "rz": 0.0,
      "vx": 10.5,
      "vy": 0.0,
      "vz": -5.2,
      "action": "accelerating"
    },
    "650e8400-e29b-41d4-a716-446655440001": {
      "uuid": "650e8400-e29b-41d4-a716-446655440001",
      "username": "fighter_beta",
      ...
    }
  }
}
```

---

### 3. Offline Notification（离线通知）

**功能**：

- 服务器主动通知客户端：你已离线，UUID 已保存
- 离线玩家不再出现在广播中

**服务器主动发送**（后台线程每 5 秒检查一次）：

```json
{
  "action": "offline",
  "reason": "inactivity",
  "uuid": "550e8400-e29b-41d4-a716-446655440000",
  "message": "No activity for 60 seconds, going offline. Rejoin with same UUID to resume."
}
```

**触发条件**：

- 玩家 60 秒（1 分钟）无任何 update 活动
- 不需要 heartbeat（已删除）

**客户端应对**：

- 显示"离线"提示，但 UUID 已保存
- 玩家可以重新注册使用相同 UUID 恢复身份
- 离线期间不显示该玩家位置

**服务器行为**：

- 离线玩家的状态保留在内存中
- 但不包含在 broadcast world 消息中
- UUID 被持久化到外部存储（文件/Redis）
- 重连时可从存储中恢复完整身份

---

## 服务器状态管理

### 内存数据结构

#### World（世界状态）

```rust
HashMap<Uuid, PlayerState>
```

- **键**：玩家 UUID（全局唯一）
- **值**：完整的 PlayerState
- **特点**：所有实时数据的真实来源

#### Clients（客户端地址表）

```rust
HashMap<Uuid, SocketAddr>
```

- **键**：玩家 UUID
- **值**：网络地址（IP + 端口）
- **用途**：广播时快速查表所有目标地址

#### UsernameMap（用户名索引）

```rust
HashMap<String, Uuid>
```

- **键**：玩家昵称
- **值**：对应的 UUID
- **用途**：快速检测用户名冲突

#### LastSeen（活动时间戳）

```rust
HashMap<Uuid, Instant>
```

- **键**：玩家 UUID
- **值**：最后更新时间
- **用途**：后台线程检测超时

### 线程模型

```
主线程（socket.recv_from 循环）
  └─ 接收 UDP 包
     └─ 解析 JSON
        └─ 为每个消息生成新线程

后台线程（心跳检测）
  └─ 每 5 秒扫描一次
     └─ 找出超过 180 秒无活动的玩家
        └─ 移除 World/Clients/UsernameMap/LastSeen 中的记录
           └─ 广播更新的世界状态
```

**线程安全**：

- 使用 `Arc<Mutex<T>>` 保护所有共享数据
- 每条消息独立加锁，锁定范围尽可能小
- 无死锁风险（单向依赖：lock → operate → unlock）

---

## 完整使用示例

### 例 1：Python 客户端（自驾飞行器）

```python
import socket
import json
import time
import uuid
import math

class AirfighterClient:
    def __init__(self, server_ip="127.0.0.1", server_port=8888):
        self.server = (server_ip, server_port)
        self.socket = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
        self.socket.settimeout(2.0)
        self.uuid = None
        self.username = None
        self.x = self.y = self.z = 0.0
        self.vx = self.vy = self.vz = 0.0
        self.rx = self.ry = self.rz = 0.0

    def register(self, username, resume_uuid=None):
        """注册或恢复"""
        msg = {
            "type": "register",
            "username": username
        }
        if resume_uuid:
            msg["uuid"] = resume_uuid

        self.socket.sendto(json.dumps(msg).encode('utf-8'), self.server)
        try:
            resp, _ = self.socket.recvfrom(4096)
            r = json.loads(resp.decode('utf-8'))

            if r.get('action') == 'registered':
                self.uuid = r.get('uuid')
                self.username = r.get('username')
                if r.get('state'):
                    state = r['state']
                    self.x = state.get('x', 0.0) or 0.0
                    self.y = state.get('y', 0.0) or 0.0
                    self.z = state.get('z', 0.0) or 0.0
                    self.vx = state.get('vx', 0.0) or 0.0
                    self.vy = state.get('vy', 0.0) or 0.0
                    self.vz = state.get('vz', 0.0) or 0.0
                return True, f"已注册: {self.username} (uuid={self.uuid})"

            elif r.get('action') == 'name_conflict':
                suggested = r.get('suggested')
                return False, f"用户名冲突，建议: {suggested}"
        except socket.timeout:
            return False, "注册超时"

    def send_update(self, action=None):
        """发送位置更新"""
        msg = {
            "type": "update",
            "uuid": self.uuid,
            "x": self.x,
            "y": self.y,
            "z": self.z,
            "rx": self.rx,
            "ry": self.ry,
            "rz": self.rz,
            "vx": self.vx,
            "vy": self.vy,
            "vz": self.vz,
            "ts": int(time.time() * 1000),
        }
        if action:
            msg["action"] = action

        self.socket.sendto(json.dumps(msg).encode('utf-8'), self.server)

    def recv_world(self):
        """接收世界状态"""
        try:
            data, _ = self.socket.recvfrom(4096)
            return json.loads(data.decode('utf-8'))
        except socket.timeout:
            return None

    def heartbeat(self):
        """发送心跳"""
        msg = {"type": "heartbeat", "uuid": self.uuid}
        self.socket.sendto(json.dumps(msg).encode('utf-8'), self.server)

# 使用
client = AirfighterClient()
ok, msg = client.register("fighter_alpha")
print(msg)

if ok:
    # 简单飞行循环
    for i in range(100):
        # 模拟加速
        if i % 20 == 0:
            client.vx = 10 + i * 0.1

        # 自动积分位置
        client.x += client.vx * 0.05
        client.y += client.vy * 0.05
        client.z += client.vz * 0.05

        # 发送更新
        client.send_update(action="flying")

        # 接收广播
        world = client.recv_world()
        if world:
            players = world.get('players', {})
            print(f"[World] {len(players)} players online")

        time.sleep(0.05)
```

### 例 2：Unity/Unreal 集成（伪代码）

```csharp
using UnityEngine;
using System;
using System.Net;
using System.Net.Sockets;
using System.Text;
using Newtonsoft.Json;

public class GameServer : MonoBehaviour {
    private Socket socket;
    private IPEndPoint serverEP = new IPEndPoint(IPAddress.Loopback, 8888);

    public void Register(string username) {
        var msg = new {
            type = "register",
            username = username
        };
        Send(JsonConvert.SerializeObject(msg));
    }

    public void SendUpdate() {
        var playerPos = GameManager.Instance.playerController.transform.position;
        var playerRot = GameManager.Instance.playerController.transform.rotation.eulerAngles;
        var playerVel = GameManager.Instance.playerController.velocity;

        var msg = new {
            type = "update",
            uuid = GameManager.Instance.myUUID,
            x = playerPos.x,
            y = playerPos.y,
            z = playerPos.z,
            rx = playerRot.x,
            ry = playerRot.y,
            rz = playerRot.z,
            vx = playerVel.x,
            vy = playerVel.y,
            vz = playerVel.z,
            ts = (long)(Time.time * 1000),
            action = GameManager.Instance.playerController.currentAction
        };
        Send(JsonConvert.SerializeObject(msg));
    }

    public void RecvWorldUpdate() {
        byte[] buffer = new byte[2048];
        int len = socket.ReceiveFrom(buffer, ref serverEP);
        var json = Encoding.UTF8.GetString(buffer, 0, len);
        var world = JsonConvert.DeserializeObject<WorldState>(json);

        // 更新所有玩家的视图
        GameManager.Instance.UpdatePlayers(world.players);
    }
}
```

---

## 错误处理指南

### 常见场景与应对

| 场景         | 错误              | 应对                       |
| ------------ | ----------------- | -------------------------- |
| 网络中断     | `SocketTimeout`   | 重试心跳或重新注册         |
| 用户名被占   | `name_conflict`   | 接受建议或让用户输入新名字 |
| 超时被移除   | `removed` 通知    | 清理本地数据，提示重新连接 |
| 位置异常     | `correction` 消息 | 应用纠正值，恢复合理位置   |
| 服务器无响应 | 无响应            | 30 秒后判断掉线，重连      |

### 客户端接收处理框架

```python
def handle_message(msg):
    if 'action' in msg:
        action = msg['action']
        if action == 'registered':
            # 注册/恢复成功
            my_uuid = msg['uuid']
            my_username = msg['username']
            if 'state' in msg:
                # 恢复了历史状态
                restore_player_state(msg['state'])

        elif action == 'name_conflict':
            # 用户名冲突
            suggested = msg['suggested']
            retry_register_with(suggested)

        elif action == 'correction':
            # 位置纠正（反作弊）
            corrected = msg['corrected']
            apply_correction(corrected)

        elif action == 'removed':
            # 被踢出（超时）
            disconnect()
            show_message("Connection timeout, please reconnect")

    elif 'players' in msg:
        # 世界状态广播
        update_all_players(msg['players'])
```

---

## 最佳实践

### 1. 会话恢复

**问题**：网络抖动断开，玩家不想重新开始。

**方案**：

```python
# 首次启动
uuid_file = load_from_disk("my_uuid.txt")
if uuid_file:
    register(username, resume_uuid=uuid_file)
else:
    register(username)
    save_to_disk("my_uuid.txt", my_uuid)
```

### 2. 位置同步频率

**建议**：

- **帧率游戏**（60 FPS）：每帧发送 update（~16ms 间隔）
- **慢速游戏**（10 FPS）：每帧发送 + 定期心跳（30s）
- **Web 前端**（30 FPS）：每帧或每 2 帧发送一次

### 3. 时间戳管理

```python
# 推荐：使用客户端本地时间
import time
ts = int(time.time() * 1000)  # 毫秒

# 或：递增序列（相对时间）
frame_counter += 1
ts = frame_counter * 16  # 假设 60 FPS
```

**注意**：服务器只用时间戳做速度验证，不需要与客户端同步。

### 4. 反作弊容差

```python
# 服务器的容差：0.5 米
# 客户端若想绕过反作弊，需要：
# actual_dist > expected_dist + 0.5

# 合理配置：
# 若最大速度 100 m/s，帧率 60 FPS
# 单帧期望距离 = 100 / 60 = 1.67 m
# 容差覆盖约 30% 误差，合理
```

### 5. 网络优化

```python
# 压缩消息（可选字段）
# 若某值未变，就不发
prev_x = 0
if new_x != prev_x:
    msg['x'] = new_x
```

### 6. 调试日志

```python
# 服务器打印
# "Received update for fighter_alpha"
# "Removed fighter_beta due to timeout"

# 客户端应打印
# "Registered: uuid=xxx"
# "Correction applied: pos=(1,2,3)"
# "World state: 5 players"
```

---

## 性能调优建议

### 服务器端

| 参数         | 默认值      | 调优           | 说明             |
| ------------ | ----------- | -------------- | ---------------- |
| 心跳检测间隔 | 5 秒        | ↓ 减少延迟     | 每次扫描 O(n)    |
| 超时时间     | 180 秒      | ↑ 减少网络压力 | 太短易误判       |
| 消息处理     | 单线程/消息 | ✓ 已优化       | 无等待，并发安全 |
| 内存占用     | O(n)        | -              | n=玩家数         |

### 预期吞吐量

```
单服务器实例：
• CPU: ~2核 (Rust 高效)
• 内存: 1 GB (支持 ~100k 玩家对象)
• 网络: UDP 带宽取决于
  - 消息频率：若 20 FPS，每消息 ~300 B
  - 玩家数：每次广播 ~1 KB/玩家

示例：
  100 玩家 × 300 B/update × 20 updates/s = 600 KB/s 上行
```

### 扩展方案

```
分片 / 分地区：
┌─────────────────────┐
│  Match Server (主)   │ → 玩家认证、配队
└──────────┬──────────┘
           │
    ┌──────┼──────┐
    │      │      │
┌───▼─┐ ┌──▼──┐ ┌─▼───┐
│ 区域  │ 区域  │ 区域  │
│Server│Server│Server│
└──────┘ └─────┘ └─────┘
```

---

## 调试与监控

### 服务器输出示例

```
Rust UDP server listening on 8888...
Received update for fighter_alpha
Received update for fighter_beta
Removed fighter_gamma due to timeout
```

### 客户端测试工具

```bash
# 发送单条 register 消息
echo '{"type":"register","username":"test"}' | \
  nc -u 127.0.0.1 8888

# 监听响应
nc -u -l 127.0.0.1 5000 &
# (修改客户端回应到 5000 端口)
```

---

## 常见问题 (FAQ)

**Q: 为什么用 UDP 而不是 TCP？**
A: UDP 低延迟，无重连开销，适合实时游戏。丢包无碍（玩家会继续发送最新位置）。

**Q: 服务器如何处理数据包丢失？**
A: 被动恢复。玩家定期发送状态，丢包只是延迟一帧，下一包到达时自动同步。

**Q: 能支持多少玩家？**
A: 内存 O(n)，取决于机器。单实例轻松支持 1k+，需要更多可部署多实例 + 负载均衡。

**Q: 如何持久化玩家数据？**
A: 当前服务器不存储数据（内存中）。可自行扩展：数据库 + 定期保存时刻。

**Q: 能改用 TCP 吗？**
A: 可以，但失去低延迟优势。建议仅调试时用 TCP，生产环境用 UDP。

---

## 总结与快速开始

### 最小化客户端实现（20 行代码）

```python
import socket, json, time

sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
sock.settimeout(2)

# 1. 注册
msg = {"type": "register", "username": "test"}
sock.sendto(json.dumps(msg).encode(), ("127.0.0.1", 8888))
r = json.loads(sock.recv(4096))
uuid = r['uuid']

# 2. 发送位置
for i in range(100):
    msg = {
        "type": "update",
        "uuid": uuid,
        "x": i,
        "y": 0,
        "z": 0,
        "ts": int(time.time() * 1000)
    }
    sock.sendto(json.dumps(msg).encode(), ("127.0.0.1", 8888))
    time.sleep(0.05)
```

### 集成清单

- [ ] 连接到 `127.0.0.1:8888`（或适当 IP）
- [ ] 实现注册流程（capture UUID）
- [ ] 周期性发送 update（位置/旋转/速度）
- [ ] 接收并解析 world broadcast（更新远程玩家）
- [ ] 处理 correction（反作弊纠正）
- [ ] 处理 removed（超时踢出）
- [ ] 定期心跳（可选，update 频繁时不需要）
