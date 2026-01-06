# 容器化与 Redis 持久化迁移方案

## 当前状态

当前实现使用 **文件系统** (`uuid_storage.json`) 作为 UUID 持久化存储：

```rust
pub struct UuidStorage {
    pub uuids: HashMap<Uuid, String>, // UUID -> username 映射
}

impl UuidStorage {
    pub fn load_from_file(path: &str) -> std::io::Result<Self> { ... }
    pub fn save_to_file(&self, path: &str) -> std::io::Result<()> { ... }
}
```

**优点**：

- 简单可靠，无外部依赖
- 快速开发和测试
- 单机开发环境最小化

**局限性**：

- 多实例/分布式环境下无法共享状态
- 文件同步困难，容易产生竞态条件
- 容器化后持久化存储困难（容器内文件系统是临时的）

---

## 迁移到 Redis 的可行性分析

### 1. 可行性评估

| 方面           | 评估        | 备注                                   |
| -------------- | ----------- | -------------------------------------- |
| **可行性**     | ✅ 高度可行 | Redis API 设计与当前实现高度兼容       |
| **迁移复杂度** | ✅ 低到中等 | 只需替换 load/save 方法                |
| **生产就绪度** | ✅ 成熟稳定 | Redis 是业界标准的 KV 存储             |
| **容器支持**   | ✅ 完美支持 | 通过 Docker Compose 或 Kubernetes 部署 |
| **成本**       | ⚠️ 中等     | 需要额外的 Redis 实例运维              |

### 2. 迁移步骤

#### Step 1: 添加 Redis 依赖

编辑 `Cargo.toml`：

```toml
[dependencies]
redis = "0.24"  # Redis 客户端库
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
uuid = { version = "1.0", features = ["v4", "serde"] }
```

#### Step 2: 创建 Redis 适配层

在 `src/lib.rs` 中添加新的 trait 和实现：

```rust
// 抽象存储接口
pub trait UuidStore: Send + Sync {
    fn load(&self) -> Result<UuidStorage, Box<dyn std::error::Error>>;
    fn save(&self, storage: &UuidStorage) -> Result<(), Box<dyn std::error::Error>>;
}

// 文件系统实现（保留原来的）
pub struct FileUuidStore {
    path: String,
}

impl UuidStore for FileUuidStore {
    fn load(&self) -> Result<UuidStorage, Box<dyn std::error::Error>> {
        // 现有的 load_from_file 逻辑
    }

    fn save(&self, storage: &UuidStorage) -> Result<(), Box<dyn std::error::Error>> {
        // 现有的 save_to_file 逻辑
    }
}

// Redis 实现（新增）
pub struct RedisUuidStore {
    client: redis::Client,
}

impl RedisUuidStore {
    pub fn new(redis_url: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let client = redis::Client::open(redis_url)?;
        Ok(RedisUuidStore { client })
    }
}

impl UuidStore for RedisUuidStore {
    fn load(&self) -> Result<UuidStorage, Box<dyn std::error::Error>> {
        let mut conn = self.client.get_connection()?;

        // 从 Redis 获取所有 UUID:username 对
        let uuids: Vec<(String, String)> = redis::cmd("HGETALL")
            .arg("game:uuids")
            .query(&mut conn)?;

        let mut map = HashMap::new();
        for (uuid_str, username) in uuids.into_iter().step_by(2) {
            if let Ok(uuid) = Uuid::parse_str(&uuid_str) {
                map.insert(uuid, username);
            }
        }

        Ok(UuidStorage { uuids: map })
    }

    fn save(&self, storage: &UuidStorage) -> Result<(), Box<dyn std::error::Error>> {
        let mut conn = self.client.get_connection()?;

        // 使用 Redis Hash 存储 UUID:username 对
        for (uuid, username) in &storage.uuids {
            redis::cmd("HSET")
                .arg("game:uuids")
                .arg(uuid.to_string())
                .arg(username)
                .execute(&mut conn);
        }

        Ok(())
    }
}
```

#### Step 3: 更新 main.rs

```rust
use std::env;

fn main() -> std::io::Result<()> {
    // ... 现有代码 ...

    // 根据环境变量选择存储后端
    let store: Box<dyn UuidStore> = if let Ok(redis_url) = env::var("REDIS_URL") {
        Box::new(RedisUuidStore::new(&redis_url).expect("Failed to connect to Redis"))
    } else {
        Box::new(FileUuidStore {
            path: "uuid_storage.json".to_string()
        })
    };

    let uuid_storage: Arc<Mutex<UuidStorage>> = Arc::new(Mutex::new(
        store.load().unwrap_or_else(|_| UuidStorage {
            uuids: HashMap::new(),
        })
    ));

    // ... 其他代码 ...
}
```

#### Step 4: 定期同步到 Redis

修改后台线程中的持久化调用：

```rust
// 在标记玩家离线时
storage.add_uuid(*uuid, player.username.clone());
let _ = store.save(&storage);  // 使用 trait 方法而非直接文件操作
```

---

## 容器化部署方案

### 方案 A：Docker Compose（推荐用于开发/小规模）

#### docker-compose.yml

```yaml
version: "3.8"

services:
  redis:
    image: redis:7-alpine
    ports:
      - "6379:6379"
    volumes:
      - redis_data:/data
    command: redis-server --appendonly yes
    healthcheck:
      test: ["CMD", "redis-cli", "ping"]
      interval: 5s
      timeout: 3s
      retries: 5

  game-server:
    build:
      context: .
      dockerfile: Dockerfile
    ports:
      - "8888:8888/udp"
    environment:
      - REDIS_URL=redis://redis:6379/0
      - RUST_LOG=info
    depends_on:
      redis:
        condition: service_healthy
    volumes:
      - ./logs:/app/logs

volumes:
  redis_data:

networks:
  default:
    name: game-network
```

#### Dockerfile

```dockerfile
FROM rust:1.75-alpine as builder

WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src ./src

RUN apk add --no-cache musl-dev
RUN cargo build --release

FROM alpine:latest

WORKDIR /app

# 安装运行时依赖
RUN apk add --no-cache ca-certificates

# 复制编译结果
COPY --from=builder /app/target/release/rust_server .

# 暴露 UDP 端口
EXPOSE 8888/udp

CMD ["./rust_server"]
```

#### 启动命令

```bash
# 开发环境
docker-compose up

# 后台运行
docker-compose up -d

# 查看日志
docker-compose logs -f game-server

# 停止
docker-compose down
```

---

### 方案 B：Kubernetes（推荐用于生产）

#### k8s-redis-deployment.yaml

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: game-redis
spec:
  replicas: 1
  selector:
    matchLabels:
      app: game-redis
  template:
    metadata:
      labels:
        app: game-redis
    spec:
      containers:
        - name: redis
          image: redis:7-alpine
          ports:
            - containerPort: 6379
          volumeMounts:
            - name: redis-storage
              mountPath: /data
          command:
            - redis-server
            - --appendonly
            - "yes"
      volumes:
        - name: redis-storage
          persistentVolumeClaim:
            claimName: redis-pvc

---
apiVersion: v1
kind: Service
metadata:
  name: game-redis
spec:
  selector:
    app: game-redis
  ports:
    - port: 6379
      targetPort: 6379
  type: ClusterIP

---
apiVersion: v1
kind: PersistentVolumeClaim
metadata:
  name: redis-pvc
spec:
  accessModes:
    - ReadWriteOnce
  resources:
    requests:
      storage: 10Gi
```

#### k8s-game-server-deployment.yaml

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: game-server
spec:
  replicas: 3 # 可水平扩展
  selector:
    matchLabels:
      app: game-server
  template:
    metadata:
      labels:
        app: game-server
    spec:
      containers:
        - name: game-server
          image: your-registry/game-server:latest
          ports:
            - name: game-udp
              containerPort: 8888
              protocol: UDP
          env:
            - name: REDIS_URL
              value: "redis://game-redis:6379/0"
            - name: RUST_LOG
              value: "info"
          resources:
            requests:
              memory: "256Mi"
              cpu: "250m"
            limits:
              memory: "512Mi"
              cpu: "500m"
          livenessProbe:
            exec:
              command: ["test", "-f", "/proc/1/cmdline"]
            initialDelaySeconds: 10
            periodSeconds: 10

---
apiVersion: v1
kind: Service
metadata:
  name: game-server
spec:
  selector:
    app: game-server
  ports:
    - name: game-udp
      port: 8888
      targetPort: 8888
      protocol: UDP
  type: LoadBalancer # 或 NodePort
```

#### 部署命令

```bash
# 应用 Redis
kubectl apply -f k8s-redis-deployment.yaml

# 应用游戏服务器
kubectl apply -f k8s-game-server-deployment.yaml

# 查看部署状态
kubectl get pods
kubectl get svc

# 查看日志
kubectl logs -f deployment/game-server
```

---

## Redis 数据结构设计

### 存储架构

```
game:uuids (Hash)
  ├─ "550e8400-e29b-41d4-a716-446655440000" -> "player_1"
  ├─ "650e8400-e29b-41d4-a716-446655440001" -> "player_2"
  └─ ...

game:online_status (Set)
  ├─ "550e8400-e29b-41d4-a716-446655440000"  (在线玩家 UUID)
  └─ ...

game:player_state:{uuid} (Hash)  [可选扩展]
  ├─ "x" -> "100.5"
  ├─ "y" -> "200.0"
  ├─ "username" -> "player_1"
  └─ ...
```

### Redis 命令示例

```bash
# 保存 UUID
HSET game:uuids "550e8400-e29b-41d4-a716-446655440000" "player_1"

# 获取所有 UUID
HGETALL game:uuids

# 检查 UUID 是否存在
HEXISTS game:uuids "550e8400-e29b-41d4-a716-446655440000"

# 删除 UUID
HDEL game:uuids "550e8400-e29b-41d4-a716-446655440000"

# 获取用户名
HGET game:uuids "550e8400-e29b-41d4-a716-446655440000"
```

---

## 性能对比

| 指标           | 文件系统 | Redis                    |
| -------------- | -------- | ------------------------ |
| **读取延迟**   | 5-20ms   | 1-5ms                    |
| **写入延迟**   | 10-50ms  | 1-5ms                    |
| **并发支持**   | ⚠️ 差    | ✅ 优秀                  |
| **分布式支持** | ❌ 否    | ✅ 是                    |
| **数据复制**   | ❌ 否    | ✅ 是 (Sentinel/Cluster) |
| **自动清理**   | ❌ 否    | ✅ 是 (TTL)              |
| **内存占用**   | 极低     | 中等                     |

---

## 迁移检查清单

- [ ] 添加 `redis` crate 到 Cargo.toml
- [ ] 创建 `UuidStore` trait 抽象
- [ ] 实现 `FileUuidStore`（保留现有逻辑）
- [ ] 实现 `RedisUuidStore`
- [ ] 添加环境变量支持（REDIS_URL）
- [ ] 更新 main.rs 使用存储 trait
- [ ] 创建 Dockerfile
- [ ] 创建 docker-compose.yml
- [ ] 本地使用 Docker Compose 测试
- [ ] 创建 Kubernetes YAML
- [ ] 在测试集群上验证
- [ ] 监控 Redis 内存/连接数
- [ ] 配置 Redis 备份和恢复计划

---

## 推荐路径

### 短期（现在）✅

使用文件系统存储，适合单机开发和测试。

### 中期（2-4 周）

添加 Redis 支持，但保留文件系统作为备选。部署 Docker Compose 进行测试。

### 长期（1-3 月）

完全迁移到 Redis，部署到 Kubernetes，配置数据持久化和高可用。

---

## 常见问题

### Q: Redis 单点故障怎么办？

A: 部署 Redis Sentinel 以实现自动故障转移，或使用 Redis Cluster 分布式部署。

### Q: 如何处理 Redis 连接失败？

A: 实现重试逻辑和回退到本地缓存，或使用连接池管理。

### Q: Redis 数据会丢失吗？

A: 启用 AOF 持久化（Append-Only File）确保数据安全。在 docker-compose.yml 中已包含。

### Q: 需要修改现有的测试吗？

A: 测试仍然使用文件系统实现，生产环境使用 Redis。可通过环境变量切换。

---

## 总结

**可行性：✅ 100% 可行**

迁移到 Redis 并容器化：

- 需要修改量小（仅 UuidStorage 接口）
- 开发时间：2-3 天
- 零停机迁移可能（双写策略）
- 性能提升：5-10 倍
- 分布式支持：完全支持

**建议：现在使用文件系统，待项目规模扩大再迁移到 Redis。**
