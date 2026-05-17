> **阶段口径（2026-05 P6 contract-first slice）**
>
> 本文是早期 Rust/OpenMLS 工程方案草稿，不代表当前已落地能力。
> 当前 CLI 实现已接入 `anp-mls` one-shot exec provider、P6 wire/storage/API、KeyPackage 发布、
> `group create --message-security-profile group-e2ee` / `--e2ee`、group-e2ee add/send/decrypt 编排。
> `contract-test` 非加密 artifact 只能通过显式 flag 启用；真实 OpenMLS 可用性仍以 `anp-mls` 后端和系统测试验收为准。
> HTTP server / daemon、OpenMLS `StorageProvider`、真实 MLS group state 持久化、
> snapshot、多设备同步等内容均是后续设计方向，不属于本阶段验收范围。
> 对外 discovery 必须继续隐藏 `anp.group.e2ee.v1` / `group-e2ee`，并由 feature flag
> 显式开启测试面。

下面是按你新约束改过的完整方案：**API 同时提供 HTTP JSON API 和 CLI API，但系统不能依赖一个常驻后台进程。**

核心设计思想是：

> Rust OpenMLS 组件不是“必须一直运行的 daemon”，而是一个独立二进制 `awiki-mls`。
> 它既可以用 `awiki-mls serve` 提供 HTTP JSON API，也可以被 Go CLI 每次按需调用，执行一次命令后退出。
> 所有 MLS 状态必须落盘，不能依赖进程内存。

OpenMLS 本身是 Rust 实现的 MLS 协议库，目标是作为端到端加密应用的构建块；MLS 的标准规范是 IETF RFC 9420。([GitHub][1]) OpenMLS 的 `MlsGroup` 状态会持续写入 `StorageProvider`，后续可以通过 `GroupId` 从 provider 重新 load，这正好适合“不依赖常驻进程”的设计。([OpenMLS Book][2])

---

# 1. 总体结论

你的新方案应该从：

```text
Go CLI + Rust OpenMLS sidecar
```

调整为：

```text
Go CLI + Rust OpenMLS command/server binary
```

也就是：

```text
awiki-cli / ANP Go SDK
        |
        | 方式 A：exec 调用 Rust CLI，一次一进程
        | 方式 B：HTTP JSON 调用 Rust server，可选
        v
awiki-mls
        |
        v
OpenMLS + 本地持久化 StorageProvider
```

其中默认推荐：

```text
默认模式：CLI exec mode
可选模式：HTTP server mode
```

也就是说：

* **不要求 `awiki-mls` 常驻运行**
* **Go CLI 默认每次调用 Rust CLI**
* **HTTP JSON API 只是加速、调试、服务化、长期运行场景用**
* **两种 API 调用同一套 Rust service core**
* **状态全部保存在本地数据库中**

---

# 2. 最终架构

```text
┌──────────────────────────────────────────────────────┐
│                 awiki-cli / ANP Go SDK                │
│                                                      │
│  - ANP/P6 消息封装                                    │
│  - DID / did:wba 身份                                 │
│  - 群业务逻辑                                          │
│  - 消息收发 / relay / 跨域路由                         │
│  - MLSProvider 抽象接口                               │
└───────────────────────┬──────────────────────────────┘
                        │
          ┌─────────────┴─────────────┐
          │                           │
          ▼                           ▼
┌──────────────────────┐    ┌──────────────────────────┐
│ ExecProvider          │    │ HTTPProvider              │
│                      │    │                          │
│ os/exec 调用           │    │ 调用 127.0.0.1 / UDS      │
│ awiki-mls command     │    │ awiki-mls serve           │
└──────────┬───────────┘    └───────────┬──────────────┘
           │                            │
           └─────────────┬──────────────┘
                         ▼
┌──────────────────────────────────────────────────────┐
│                    awiki-mls                          │
│                    Rust binary                        │
│                                                      │
│  CLI mode:                                           │
│    awiki-mls group create --json-in -                 │
│    awiki-mls message encrypt --json-in -              │
│                                                      │
│  Server mode:                                        │
│    awiki-mls serve --listen 127.0.0.1:8742            │
│                                                      │
│  Shared core:                                        │
│    OpenMlsEngine                                     │
│    StorageRepository                                 │
│    OperationLog                                      │
│    LockManager                                       │
└───────────────────────┬──────────────────────────────┘
                        ▼
┌──────────────────────────────────────────────────────┐
│                 Local MLS Storage                     │
│                                                      │
│  ~/.awiki/mls/sidecar.db                              │
│  ~/.awiki/mls/locks/                                  │
│  ~/.awiki/mls/snapshots/                              │
└──────────────────────────────────────────────────────┘
```

---

# 3. 两种 API 的定位

## 3.1 CLI API：默认路径

这是你当前 Go CLI 最适合用的方式。

Go 层通过 `os/exec` 调用：

```bash
awiki-mls group create --json-in -
awiki-mls key-package generate --json-in -
awiki-mls group add-member --json-in -
awiki-mls welcome process --json-in -
awiki-mls message encrypt --json-in -
awiki-mls message decrypt --json-in -
awiki-mls group restore --json-in -
awiki-mls snapshot export --json-in -
awiki-mls snapshot import --json-in -
```

特点：

```text
1. 不需要后台进程
2. 每次命令启动一次 Rust 进程
3. 命令执行完立即退出
4. 状态从本地 DB load，执行后写回 DB
5. Go CLI 仍然是纯 Go 主工程
```

这是第一阶段的主路径。

---

## 3.2 HTTP JSON API：可选路径

用于：

```text
1. 本地开发调试
2. 长时间运行的 agent runtime
3. awiki desktop / daemon 模式
4. 性能测试
5. 集成测试
```

启动方式：

```bash
awiki-mls serve \
  --listen 127.0.0.1:8742 \
  --data-dir ~/.awiki/mls
```

HTTP API 不能作为唯一依赖。Go CLI 不能假设它一定存在。

---

# 4. Go 侧 Provider 策略

Go 层定义统一接口：

```go
type MLSProvider interface {
    GenerateKeyPackage(ctx context.Context, req GenerateKeyPackageRequest) (*GenerateKeyPackageResponse, error)
    CreateGroup(ctx context.Context, req CreateGroupRequest) (*CreateGroupResponse, error)
    AddMember(ctx context.Context, req AddMemberRequest) (*AddMemberResponse, error)
    ProcessWelcome(ctx context.Context, req ProcessWelcomeRequest) (*ProcessWelcomeResponse, error)
    Encrypt(ctx context.Context, req EncryptRequest) (*EncryptResponse, error)
    Decrypt(ctx context.Context, req DecryptRequest) (*DecryptResponse, error)
    RestoreGroup(ctx context.Context, req RestoreGroupRequest) (*RestoreGroupResponse, error)
    ExportSnapshot(ctx context.Context, req ExportSnapshotRequest) (*ExportSnapshotResponse, error)
    ImportSnapshot(ctx context.Context, req ImportSnapshotRequest) (*ImportSnapshotResponse, error)
}
```

实现三个 provider：

```go
type ExecProvider struct {
    BinaryPath string
    DataDir    string
    Timeout    time.Duration
}

type HTTPProvider struct {
    Endpoint string
    Token    string
    Client   *http.Client
}

type AutoProvider struct {
    HTTP *HTTPProvider
    Exec *ExecProvider
}
```

推荐默认策略：

```text
AWIKI_MLS_MODE=auto
```

`auto` 模式逻辑：

```text
1. 检查 AWIKI_MLS_ENDPOINT 是否存在
2. 尝试 GET /healthz
3. 如果 HTTP server 可用，走 HTTPProvider
4. 如果 HTTP server 不可用，走 ExecProvider
5. 永远不要求用户先手动启动后台进程
```

配置项：

```bash
AWIKI_MLS_MODE=auto        # auto | exec | http
AWIKI_MLS_BIN=awiki-mls
AWIKI_MLS_ENDPOINT=http://127.0.0.1:8742
AWIKI_MLS_DATA_DIR=~/.awiki/mls
AWIKI_MLS_TIMEOUT=15s
```

---

# 5. Rust 侧二进制设计

Rust 项目叫：

```text
awiki-mls
```

目录建议：

```text
awiki-mls/
  Cargo.toml
  src/
    main.rs
    cli.rs
    http.rs
    api/
      mod.rs
      types.rs
      error.rs
    engine/
      mod.rs
      key_package.rs
      group.rs
      welcome.rs
      message.rs
      snapshot.rs
    storage/
      mod.rs
      sqlite.rs
      lock.rs
      operations.rs
    security/
      aad.rs
      canonical_json.rs
      redaction.rs
  tests/
    cli_flow_test.rs
    http_flow_test.rs
    parity_test.rs
```

关键原则：

```text
CLI 和 HTTP 不各写一套逻辑。
它们必须调用同一个 OpenMlsEngine。
```

也就是：

```rust
struct OpenMlsEngine {
    storage: StorageRepository,
    lock_manager: LockManager,
    operation_log: OperationLog,
}
```

CLI 调用：

```rust
engine.create_group(req)
```

HTTP 调用：

```rust
engine.create_group(req)
```

这样可以保证：

```text
HTTP JSON API 和 CLI API 行为完全一致。
```

---

# 6. 统一 API Envelope

为了让 HTTP 和 CLI 共享同一个协议，建议所有 API 都用统一 JSON envelope。

## 6.1 请求结构

```json
{
  "api_version": "awiki-mls/v1",
  "request_id": "req_01HT...",
  "agent_did": "did:wba:example.com:agent:alice",
  "device_id": "default",
  "params": {}
}
```

## 6.2 成功响应

```json
{
  "ok": true,
  "api_version": "awiki-mls/v1",
  "request_id": "req_01HT...",
  "result": {}
}
```

## 6.3 失败响应

```json
{
  "ok": false,
  "api_version": "awiki-mls/v1",
  "request_id": "req_01HT...",
  "error": {
    "code": "EPOCH_MISMATCH",
    "message": "Group epoch mismatch",
    "details": {
      "expected_epoch": 3,
      "actual_epoch": 2
    }
  }
}
```

CLI 也输出同样结构。

例如：

```bash
awiki-mls group create --json-in create_group.json
```

stdout：

```json
{
  "ok": true,
  "api_version": "awiki-mls/v1",
  "request_id": "req_01HT...",
  "result": {
    "anp_group_id": "group1",
    "mls_group_id": "base64url...",
    "epoch": 0
  }
}
```

stderr 只打印日志，stdout 只打印机器可读 JSON。

---

# 7. HTTP API 与 CLI API 一一映射

| 能力                | HTTP JSON API                              | CLI API                                      |
| ----------------- | ------------------------------------------ | -------------------------------------------- |
| 健康检查              | `GET /healthz`                             | `awiki-mls health --json`                    |
| 版本检查              | `GET /v1/version`                          | `awiki-mls version --json`                   |
| 生成 KeyPackage     | `POST /v1/key-packages/generate`           | `awiki-mls key-package generate --json-in -` |
| 创建群               | `POST /v1/groups/create`                   | `awiki-mls group create --json-in -`         |
| 添加成员              | `POST /v1/groups/{group_id}/members/add`   | `awiki-mls group add-member --json-in -`     |
| 合并 pending commit | `POST /v1/groups/{group_id}/commits/merge` | `awiki-mls group commit-merge --json-in -`   |
| 处理 Welcome        | `POST /v1/welcomes/process`                | `awiki-mls welcome process --json-in -`      |
| 加密消息              | `POST /v1/groups/{group_id}/encrypt`       | `awiki-mls message encrypt --json-in -`      |
| 解密消息              | `POST /v1/groups/{group_id}/decrypt`       | `awiki-mls message decrypt --json-in -`      |
| 恢复群状态             | `POST /v1/groups/{group_id}/restore`       | `awiki-mls group restore --json-in -`        |
| 导出 snapshot       | 默认关闭或本地-only                               | `awiki-mls snapshot export --json-in -`      |
| 导入 snapshot       | 默认关闭或本地-only                               | `awiki-mls snapshot import --json-in -`      |

注意：**snapshot API 不建议默认暴露 HTTP**。它涉及敏感 key material，第一阶段建议只允许 CLI 调用，HTTP 版本必须通过 `--enable-snapshot-api` 显式开启。

---

# 8. 为什么不依赖常驻进程也能工作

每一次 CLI 调用都执行这个流程：

```text
1. Go CLI 启动 awiki-mls 子进程
2. awiki-mls 读取 JSON 请求
3. 打开 ~/.awiki/mls/sidecar.db
4. 获取对应 agent/group 的文件锁
5. 根据 anp_group_id 查 mls_group_id
6. 从 OpenMLS StorageProvider load MlsGroup
7. 执行 create/add/encrypt/decrypt 等操作
8. OpenMLS 写回 StorageProvider
9. awiki-mls 写 metadata / operation log
10. 输出 JSON response
11. 进程退出
```

这个模型的关键是：

```text
进程不是状态载体。
本地 storage 才是状态载体。
```

OpenMLS 官方文档说明，`MlsGroup` 会持续写入配置的 `StorageProvider`，之后可以通过 `GroupId` 重新 load。([OpenMLS Book][2]) 所以 `awiki-mls` 完全可以按需启动、按需退出。

---

# 9. 本地存储设计

## 9.1 路径

默认路径：

```text
~/.awiki/mls/
  sidecar.db
  sidecar.lock
  locks/
  snapshots/
  runtime/
```

建议权限：

```text
~/.awiki/mls/                 0700
~/.awiki/mls/sidecar.db        0600
~/.awiki/mls/snapshots/        0700
```

## 9.2 数据库分层

本地 DB 分两层：

```text
1. OpenMLS provider storage
2. ANP/P6 metadata storage
```

OpenMLS provider storage 保存：

```text
- group state
- key material
- signature keys
- key package private material
```

ANP/P6 metadata storage 保存：

```text
- agent_did / device_id
- anp_group_id -> mls_group_id 映射
- member_did -> leaf_index 映射
- epoch 记录
- request_id 幂等记录
- pending commit 状态
```

OpenMLS README 中也列出了 sqlite provider 相关 feature，因此第一阶段可以优先使用 SQLite 存储。([GitHub][1])

---

# 10. 数据表建议

## 10.1 group bindings

```sql
CREATE TABLE group_bindings (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  agent_did TEXT NOT NULL,
  device_id TEXT NOT NULL,
  anp_group_id TEXT NOT NULL,
  mls_group_id BLOB NOT NULL,
  epoch INTEGER NOT NULL,
  self_leaf_index INTEGER,
  status TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE(agent_did, device_id, anp_group_id)
);
```

`status` 可选值：

```text
active
pending_commit
stale
corrupted
deleted
```

## 10.2 members

```sql
CREATE TABLE group_members (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  agent_did TEXT NOT NULL,
  device_id TEXT NOT NULL,
  anp_group_id TEXT NOT NULL,
  member_did TEXT NOT NULL,
  member_device_id TEXT,
  leaf_index INTEGER,
  credential_hash TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE(agent_did, device_id, anp_group_id, member_did, member_device_id)
);
```

## 10.3 key packages

```sql
CREATE TABLE key_packages (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  agent_did TEXT NOT NULL,
  device_id TEXT NOT NULL,
  key_package_id TEXT NOT NULL,
  key_package BLOB NOT NULL,
  used INTEGER DEFAULT 0,
  expires_at TEXT,
  created_at TEXT NOT NULL,
  UNIQUE(agent_did, device_id, key_package_id)
);
```

## 10.4 operation log

```sql
CREATE TABLE operations (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  request_id TEXT NOT NULL,
  operation_type TEXT NOT NULL,
  input_hash TEXT NOT NULL,
  status TEXT NOT NULL,
  response_json TEXT,
  error_json TEXT,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE(request_id)
);
```

作用：

```text
1. 防止 CLI 重试导致重复执行
2. 支持崩溃恢复
3. 支持 pending commit 查询
4. 支持 Go CLI 重新拿上次结果
```

## 10.5 pending commits

```sql
CREATE TABLE pending_commits (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  request_id TEXT NOT NULL,
  agent_did TEXT NOT NULL,
  device_id TEXT NOT NULL,
  anp_group_id TEXT NOT NULL,
  old_epoch INTEGER NOT NULL,
  new_epoch INTEGER NOT NULL,
  commit BLOB NOT NULL,
  welcome BLOB,
  status TEXT NOT NULL,
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL,
  UNIQUE(request_id)
);
```

`status`：

```text
created
delivered
merged
aborted
expired
```

---

# 11. 文件锁设计

因为你不能保证只有一个 CLI 进程在跑，所以必须做锁。

否则可能出现：

```text
两个 awiki-cli 同时 encrypt
两个 awiki-cli 同时 add_member
一个 HTTP server 和一个 CLI command 同时写同一个 group
```

建议锁粒度：

```text
identity lock:
  ~/.awiki/mls/locks/identity_<hash(agent_did, device_id)>.lock

group lock:
  ~/.awiki/mls/locks/group_<hash(agent_did, device_id, anp_group_id)>.lock
```

规则：

| 操作                   | 锁                          |
| -------------------- | -------------------------- |
| generate_key_package | identity lock              |
| create_group         | identity lock + group lock |
| add_member           | group lock                 |
| commit_merge         | group lock                 |
| process_welcome      | identity lock + group lock |
| encrypt              | group lock                 |
| decrypt              | group lock                 |
| restore              | group lock                 |
| snapshot export      | identity lock 或全局 lock     |
| snapshot import      | 全局 lock                    |

注意：`decrypt` 也建议加写锁。MLS 在处理消息时可能更新本地状态、删除旧密钥或推进 ratchet，所以不能把 decrypt 当成纯读操作。

OpenMLS 文档特别提醒，StorageProvider 中包含敏感 key material，并且 OpenMLS 会为了 forward secrecy 删除旧 key material；storage 实现必须确保删除不可恢复。([OpenMLS Book][2]) 因此锁和存储一致性非常重要。

---

# 12. 核心 API 详细设计

## 12.1 generate_key_package

### HTTP

```http
POST /v1/key-packages/generate
```

### CLI

```bash
awiki-mls key-package generate --json-in -
```

### Request

```json
{
  "api_version": "awiki-mls/v1",
  "request_id": "req_kp_001",
  "agent_did": "did:wba:example.com:agent:bob",
  "device_id": "default",
  "params": {
    "credential_identity": "did:wba:example.com:agent:bob",
    "ciphersuite": "MLS_128_DHKEMX25519_AES128GCM_SHA256_Ed25519",
    "expires_at": "2026-05-01T00:00:00Z"
  }
}
```

### Response

```json
{
  "ok": true,
  "api_version": "awiki-mls/v1",
  "request_id": "req_kp_001",
  "result": {
    "key_package_id": "kp_01HT...",
    "agent_did": "did:wba:example.com:agent:bob",
    "device_id": "default",
    "ciphersuite": "MLS_128_DHKEMX25519_AES128GCM_SHA256_Ed25519",
    "key_package": "base64url...",
    "created_at": "2026-04-26T12:00:00Z",
    "expires_at": "2026-05-01T00:00:00Z"
  }
}
```

### 行为

```text
1. 生成 Bob 的 MLS KeyPackage
2. 保存对应 private init key material
3. 返回 public KeyPackage
4. 标记 used=false
```

---

## 12.2 create_group

### HTTP

```http
POST /v1/groups/create
```

### CLI

```bash
awiki-mls group create --json-in -
```

### Request

```json
{
  "api_version": "awiki-mls/v1",
  "request_id": "req_group_create_001",
  "agent_did": "did:wba:example.com:agent:alice",
  "device_id": "default",
  "params": {
    "anp_group_id": "anp-group-001",
    "credential_identity": "did:wba:example.com:agent:alice",
    "use_ratchet_tree_extension": true
  }
}
```

### Response

```json
{
  "ok": true,
  "api_version": "awiki-mls/v1",
  "request_id": "req_group_create_001",
  "result": {
    "anp_group_id": "anp-group-001",
    "mls_group_id": "base64url...",
    "epoch": 0,
    "self_leaf_index": 0,
    "members": [
      {
        "agent_did": "did:wba:example.com:agent:alice",
        "device_id": "default",
        "leaf_index": 0
      }
    ]
  }
}
```

### 行为

```text
1. 创建 MLS group
2. 写入 OpenMLS storage
3. 写入 group_bindings
4. 初始 epoch = 0
5. Alice 是第一个成员
```

---

## 12.3 add_member

这里要特别设计好，因为没有后台进程时，`add_member` 之后可能 Go CLI 还没来得及发送 commit/welcome 就崩溃。

所以推荐支持两种策略：

```text
1. immediate_merge：PoC / 本地测试用
2. staged：真实 ANP/P6 网络发送用
```

第一阶段可以默认 `staged`。

### HTTP

```http
POST /v1/groups/{anp_group_id}/members/add
```

### CLI

```bash
awiki-mls group add-member --json-in -
```

### Request

```json
{
  "api_version": "awiki-mls/v1",
  "request_id": "req_add_bob_001",
  "agent_did": "did:wba:example.com:agent:alice",
  "device_id": "default",
  "params": {
    "anp_group_id": "anp-group-001",
    "target_agent_did": "did:wba:example.com:agent:bob",
    "target_device_id": "default",
    "key_package": "base64url...",
    "merge_strategy": "staged",
    "aad": {
      "anp_version": "1.0",
      "profile": "group-e2ee",
      "operation": "group.add",
      "anp_group_id": "anp-group-001",
      "sender_did": "did:wba:example.com:agent:alice",
      "target_did": "did:wba:example.com:agent:bob",
      "message_id": "msg_add_bob_001",
      "created_at": "2026-04-26T12:00:00Z"
    }
  }
}
```

### Response

```json
{
  "ok": true,
  "api_version": "awiki-mls/v1",
  "request_id": "req_add_bob_001",
  "result": {
    "anp_group_id": "anp-group-001",
    "mls_group_id": "base64url...",
    "old_epoch": 0,
    "new_epoch": 1,
    "merge_strategy": "staged",
    "pending_commit_id": "pc_req_add_bob_001",
    "commit": "base64url...",
    "welcome": "base64url...",
    "ratchet_tree": "base64url-optional",
    "added_member": {
      "agent_did": "did:wba:example.com:agent:bob",
      "device_id": "default"
    }
  }
}
```

### staged 模式流程

```text
1. Alice 本地生成 Add Commit 和 Welcome
2. sidecar 保存 pending_commit
3. sidecar 暂不 merge 到新 epoch
4. Go CLI 把 commit/welcome 发送到 ANP 网络
5. 发送成功后，Go CLI 调用 commit_merge
6. sidecar merge pending commit，Alice 进入 epoch 1
```

### 为什么需要 staged

否则会出现：

```text
Alice 本地已经 epoch=1
但 commit/welcome 没发出去
Bob 还不知道自己被加入
群状态不一致
```

为了第一阶段快速 PoC，也可以允许：

```json
"merge_strategy": "immediate"
```

但真实网络场景建议用：

```json
"merge_strategy": "staged"
```

---

## 12.4 commit_merge

虽然你最初列出的最小闭环没有单独写 `commit_merge`，但真实发送流程里最好加这个支持命令。

### HTTP

```http
POST /v1/groups/{anp_group_id}/commits/merge
```

### CLI

```bash
awiki-mls group commit-merge --json-in -
```

### Request

```json
{
  "api_version": "awiki-mls/v1",
  "request_id": "req_merge_001",
  "agent_did": "did:wba:example.com:agent:alice",
  "device_id": "default",
  "params": {
    "anp_group_id": "anp-group-001",
    "pending_commit_id": "pc_req_add_bob_001",
    "delivery_result": "accepted"
  }
}
```

### Response

```json
{
  "ok": true,
  "api_version": "awiki-mls/v1",
  "request_id": "req_merge_001",
  "result": {
    "anp_group_id": "anp-group-001",
    "mls_group_id": "base64url...",
    "old_epoch": 0,
    "new_epoch": 1,
    "status": "merged"
  }
}
```

---

## 12.5 process_welcome

### HTTP

```http
POST /v1/welcomes/process
```

### CLI

```bash
awiki-mls welcome process --json-in -
```

### Request

```json
{
  "api_version": "awiki-mls/v1",
  "request_id": "req_process_welcome_001",
  "agent_did": "did:wba:example.com:agent:bob",
  "device_id": "default",
  "params": {
    "anp_group_id": "anp-group-001",
    "welcome": "base64url...",
    "ratchet_tree": "base64url-optional"
  }
}
```

### Response

```json
{
  "ok": true,
  "api_version": "awiki-mls/v1",
  "request_id": "req_process_welcome_001",
  "result": {
    "anp_group_id": "anp-group-001",
    "mls_group_id": "base64url...",
    "epoch": 1,
    "self_leaf_index": 1,
    "members": [
      {
        "agent_did": "did:wba:example.com:agent:alice",
        "leaf_index": 0
      },
      {
        "agent_did": "did:wba:example.com:agent:bob",
        "leaf_index": 1
      }
    ]
  }
}
```

### 行为

```text
1. Bob 收到 Welcome
2. 调用 process_welcome
3. sidecar 创建 Bob 本地 group state
4. 写入 group_bindings
5. Bob 之后可以 decrypt/encrypt 群消息
```

---

## 12.6 encrypt

OpenMLS 的 AAD 是 authenticated 但不 encrypted 的数据，适合放 ANP 元数据，例如 group_id、sender_did、message_id、epoch 等。AAD 在传输中可以被查看，但不能被篡改。([OpenMLS Book][3])

### HTTP

```http
POST /v1/groups/{anp_group_id}/encrypt
```

### CLI

```bash
awiki-mls message encrypt --json-in -
```

### Request

```json
{
  "api_version": "awiki-mls/v1",
  "request_id": "req_encrypt_001",
  "agent_did": "did:wba:example.com:agent:alice",
  "device_id": "default",
  "params": {
    "anp_group_id": "anp-group-001",
    "plaintext": "base64url...",
    "aad": {
      "anp_version": "1.0",
      "profile": "group-e2ee",
      "security_profile": "mls-rfc9420",
      "operation": "group.send",
      "anp_group_id": "anp-group-001",
      "mls_group_id": "base64url...",
      "epoch": 1,
      "sender_did": "did:wba:example.com:agent:alice",
      "message_id": "msg_001",
      "content_type": "text/plain",
      "created_at": "2026-04-26T12:00:00Z"
    }
  }
}
```

### Response

```json
{
  "ok": true,
  "api_version": "awiki-mls/v1",
  "request_id": "req_encrypt_001",
  "result": {
    "anp_group_id": "anp-group-001",
    "mls_group_id": "base64url...",
    "epoch": 1,
    "ciphertext": "base64url...",
    "aad_hash": "base64url...",
    "sender_leaf_index": 0
  }
}
```

### AAD 序列化

建议：

```text
JSON Canonicalization Scheme / JCS
UTF-8 bytes
SHA-256 hash for aad_hash
```

ANP 外层 envelope 和 MLS AAD 必须绑定。

---

## 12.7 decrypt

### HTTP

```http
POST /v1/groups/{anp_group_id}/decrypt
```

### CLI

```bash
awiki-mls message decrypt --json-in -
```

### Request

```json
{
  "api_version": "awiki-mls/v1",
  "request_id": "req_decrypt_001",
  "agent_did": "did:wba:example.com:agent:bob",
  "device_id": "default",
  "params": {
    "anp_group_id": "anp-group-001",
    "ciphertext": "base64url...",
    "expected_aad": {
      "anp_version": "1.0",
      "profile": "group-e2ee",
      "security_profile": "mls-rfc9420",
      "operation": "group.send",
      "anp_group_id": "anp-group-001",
      "mls_group_id": "base64url...",
      "epoch": 1,
      "sender_did": "did:wba:example.com:agent:alice",
      "message_id": "msg_001",
      "content_type": "text/plain",
      "created_at": "2026-04-26T12:00:00Z"
    }
  }
}
```

### Response

```json
{
  "ok": true,
  "api_version": "awiki-mls/v1",
  "request_id": "req_decrypt_001",
  "result": {
    "anp_group_id": "anp-group-001",
    "mls_group_id": "base64url...",
    "epoch": 1,
    "sender": {
      "leaf_index": 0,
      "agent_did": "did:wba:example.com:agent:alice"
    },
    "plaintext": "base64url...",
    "aad": {
      "anp_version": "1.0",
      "profile": "group-e2ee",
      "security_profile": "mls-rfc9420",
      "operation": "group.send",
      "anp_group_id": "anp-group-001",
      "mls_group_id": "base64url...",
      "epoch": 1,
      "sender_did": "did:wba:example.com:agent:alice",
      "message_id": "msg_001",
      "content_type": "text/plain",
      "created_at": "2026-04-26T12:00:00Z"
    }
  }
}
```

### Go 层必须校验

```text
1. 外层 group_id == AAD anp_group_id
2. 外层 sender_did == AAD sender_did
3. 外层 message_id == AAD message_id
4. 外层 created_at == AAD created_at
5. AAD epoch == MLS 解密返回 epoch
6. sender leaf_index 能映射到 sender_did
7. content_type 符合预期
```

---

# 13. restore 与 snapshot 重新定义

因为不依赖后台进程，所以这里要区分两个概念：

```text
1. group restore / state load
2. snapshot export/import
```

---

## 13.1 group restore：每次操作自动发生

在这个设计里，`restore` 不是“把状态恢复到后台进程内存”。

因为没有常驻进程。

真正的 restore 是：

```text
从本地 StorageProvider load group state。
```

所以每次命令都会自动做：

```text
load group -> execute operation -> persist group
```

显式 restore API 主要用于：

```text
1. 检查 group state 是否存在
2. 检查当前 epoch
3. 检查成员映射
4. 检查状态是否损坏
5. 给 Go CLI 做诊断
```

### HTTP

```http
POST /v1/groups/{anp_group_id}/restore
```

### CLI

```bash
awiki-mls group restore --json-in -
```

### Response

```json
{
  "ok": true,
  "api_version": "awiki-mls/v1",
  "request_id": "req_restore_001",
  "result": {
    "anp_group_id": "anp-group-001",
    "mls_group_id": "base64url...",
    "epoch": 3,
    "self_leaf_index": 0,
    "status": "active",
    "restored": true
  }
}
```

---

## 13.2 snapshot export/import：备份与迁移

`snapshot` 不是常规消息流程的一部分。

它只用于：

```text
1. 开发调试
2. 设备迁移
3. 本地灾备
4. 测试复现
```

不建议默认频繁导出 snapshot，因为 snapshot 可能保留旧 key material，影响 forward secrecy。OpenMLS 文档明确提到，为了 forward secrecy，旧 key material 会被删除，StorageProvider 实现必须确保删除不可恢复且没有副本。([OpenMLS Book][2])

### snapshot export CLI

```bash
awiki-mls snapshot export --json-in -
```

Request：

```json
{
  "api_version": "awiki-mls/v1",
  "request_id": "req_snapshot_export_001",
  "agent_did": "did:wba:example.com:agent:alice",
  "device_id": "default",
  "params": {
    "anp_group_id": "anp-group-001",
    "output_file": "~/.awiki/mls/snapshots/group1.snapshot.enc",
    "encryption": {
      "type": "passphrase",
      "kdf": "argon2id"
    }
  }
}
```

Response：

```json
{
  "ok": true,
  "api_version": "awiki-mls/v1",
  "request_id": "req_snapshot_export_001",
  "result": {
    "snapshot_file": "~/.awiki/mls/snapshots/group1.snapshot.enc",
    "snapshot_version": "awiki-mls-snapshot/v1",
    "anp_group_id": "anp-group-001",
    "epoch": 3,
    "created_at": "2026-04-26T12:00:00Z"
  }
}
```

### snapshot import CLI

```bash
awiki-mls snapshot import --json-in -
```

Request：

```json
{
  "api_version": "awiki-mls/v1",
  "request_id": "req_snapshot_import_001",
  "agent_did": "did:wba:example.com:agent:alice",
  "device_id": "default",
  "params": {
    "snapshot_file": "~/.awiki/mls/snapshots/group1.snapshot.enc",
    "mode": "create_if_missing",
    "encryption": {
      "type": "passphrase"
    }
  }
}
```

安全策略：

```text
1. snapshot 必须加密
2. 默认不允许覆盖本地更新的 epoch
3. 默认不上传云端
4. 默认不保留多版本历史 snapshot
5. HTTP snapshot API 默认关闭
```

---

# 14. Go CLI 调用 Rust CLI 的方式

不要把敏感内容放在命令行参数里，因为命令行参数可能被系统进程列表看到。

错误示例：

```bash
awiki-mls message encrypt --plaintext "secret text"
```

推荐方式：

```bash
awiki-mls message encrypt --json-in -
```

Go 代码逻辑：

```go
func (p *ExecProvider) call(ctx context.Context, command []string, req any, resp any) error {
    body, err := json.Marshal(req)
    if err != nil {
        return err
    }

    args := append(command, "--json-in", "-")
    cmd := exec.CommandContext(ctx, p.BinaryPath, args...)
    cmd.Stdin = bytes.NewReader(body)

    var stdout bytes.Buffer
    var stderr bytes.Buffer
    cmd.Stdout = &stdout
    cmd.Stderr = &stderr

    if err := cmd.Run(); err != nil {
        return parseExecError(err, stderr.String())
    }

    return json.Unmarshal(stdout.Bytes(), resp)
}
```

原则：

```text
1. JSON request 走 stdin
2. JSON response 走 stdout
3. 日志走 stderr
4. Go 不解析 stderr 作为正常结果
5. 所有二进制字段 base64url 编码
6. plaintext 不进入 argv
```

---

# 15. Go CLI 对用户暴露的命令

用户不一定直接调用 `awiki-mls`。

你可以在 `awiki-cli` 里包一层：

```bash
awiki-cli p6 key-package create
awiki-cli p6 group create
awiki-cli p6 group add-member
awiki-cli p6 welcome process
awiki-cli p6 message encrypt
awiki-cli p6 message decrypt
awiki-cli p6 group restore
awiki-cli p6 snapshot export
awiki-cli p6 snapshot import
```

Go CLI 内部调用：

```text
awiki-cli p6 message encrypt
        |
        v
ExecProvider or HTTPProvider
        |
        v
awiki-mls message encrypt
```

---

# 16. 完整最小流程

## 16.1 Bob 生成 KeyPackage

```bash
awiki-cli p6 key-package create \
  --agent-did did:wba:example.com:agent:bob \
  --out bob_key_package.json
```

内部调用：

```bash
awiki-mls key-package generate --json-in -
```

输出 Bob 的 public KeyPackage。

---

## 16.2 Alice 创建群

```bash
awiki-cli p6 group create \
  --agent-did did:wba:example.com:agent:alice \
  --group-id anp-group-001
```

内部调用：

```bash
awiki-mls group create --json-in -
```

Alice 本地创建 group state，epoch = 0。

---

## 16.3 Alice 添加 Bob

```bash
awiki-cli p6 group add-member \
  --agent-did did:wba:example.com:agent:alice \
  --group-id anp-group-001 \
  --key-package bob_key_package.json \
  --merge-strategy staged \
  --out add_bob_result.json
```

内部调用：

```bash
awiki-mls group add-member --json-in -
```

返回：

```text
commit
welcome
pending_commit_id
new_epoch
```

---

## 16.4 Go CLI 发送 ANP 消息

Alice 的 Go CLI 发送两条 ANP 消息：

```text
1. anp-group-mls-commit+json
2. anp-group-mls-welcome+json
```

Bob 收到 welcome。

---

## 16.5 Alice merge pending commit

发送成功后：

```bash
awiki-cli p6 group commit-merge \
  --agent-did did:wba:example.com:agent:alice \
  --group-id anp-group-001 \
  --pending-commit-id pc_req_add_bob_001
```

内部调用：

```bash
awiki-mls group commit-merge --json-in -
```

Alice 本地进入 epoch = 1。

---

## 16.6 Bob process welcome

```bash
awiki-cli p6 welcome process \
  --agent-did did:wba:example.com:agent:bob \
  --group-id anp-group-001 \
  --welcome welcome.json
```

内部调用：

```bash
awiki-mls welcome process --json-in -
```

Bob 本地生成 group state，epoch = 1。

---

## 16.7 Alice 发送加密消息

```bash
awiki-cli p6 message encrypt \
  --agent-did did:wba:example.com:agent:alice \
  --group-id anp-group-001 \
  --text "hello bob"
```

内部调用：

```bash
awiki-mls message encrypt --json-in -
```

Go CLI 把返回的 ciphertext 封装成：

```json
{
  "type": "application/anp-group-mls-cipher+json",
  "group_id": "anp-group-001",
  "sender_did": "did:wba:example.com:agent:alice",
  "message_id": "msg_001",
  "created_at": "2026-04-26T12:00:00Z",
  "mls": {
    "mls_group_id": "base64url...",
    "epoch": 1,
    "aad": {},
    "ciphertext": "base64url..."
  }
}
```

---

## 16.8 Bob 解密消息

```bash
awiki-cli p6 message decrypt \
  --agent-did did:wba:example.com:agent:bob \
  --group-id anp-group-001 \
  --message cipher_message.json
```

内部调用：

```bash
awiki-mls message decrypt --json-in -
```

Bob 拿到 plaintext。

---

# 17. HTTP server 模式

HTTP server 模式只是一种加速路径。

启动：

```bash
awiki-mls serve \
  --listen 127.0.0.1:8742 \
  --data-dir ~/.awiki/mls \
  --auth-token-file ~/.awiki/mls/runtime/token
```

Go CLI 使用：

```bash
AWIKI_MLS_MODE=http awiki-cli p6 message encrypt ...
```

或者自动：

```bash
AWIKI_MLS_MODE=auto awiki-cli p6 message encrypt ...
```

`auto` 会先尝试 HTTP：

```http
GET /healthz
```

失败则 fallback 到 exec。

HTTP server 也不应该把 group state 只保存在内存里。第一阶段建议 HTTP 每次请求也走：

```text
load group from storage
execute
persist
```

这样即使 HTTP server 被 kill，也不会丢状态。

---

# 18. HTTP 安全设计

HTTP server 只绑定本地：

```text
127.0.0.1
```

或者 Unix Domain Socket：

```text
~/.awiki/mls/runtime/awiki-mls.sock
```

不允许默认监听：

```text
0.0.0.0
```

认证：

```http
Authorization: Bearer <local-token>
```

token 文件：

```text
~/.awiki/mls/runtime/token
```

权限：

```text
0600
```

HTTP snapshot API 默认关闭：

```bash
awiki-mls serve --enable-snapshot-api=false
```

---

# 19. 错误码设计

| 错误码                             | 含义                  |
| ------------------------------- | ------------------- |
| `BAD_REQUEST`                   | 请求 JSON 错误          |
| `UNSUPPORTED_API_VERSION`       | API 版本不兼容           |
| `GROUP_NOT_FOUND`               | 本地没有 group state    |
| `KEY_PACKAGE_NOT_FOUND`         | 找不到 KeyPackage 私有材料 |
| `KEY_PACKAGE_ALREADY_USED`      | KeyPackage 已使用      |
| `EPOCH_MISMATCH`                | epoch 不一致           |
| `AAD_MISMATCH`                  | AAD 与预期不一致          |
| `DECRYPT_FAILED`                | 解密失败                |
| `STORAGE_LOCKED`                | 本地锁被占用              |
| `PENDING_COMMIT_NOT_FOUND`      | 找不到 pending commit  |
| `PENDING_COMMIT_ALREADY_MERGED` | commit 已经 merge     |
| `SNAPSHOT_REJECTED`             | snapshot 导入被拒绝      |
| `INTERNAL_ERROR`                | 内部错误                |

CLI exit code 建议：

| Exit Code | 含义                    |
| --------: | --------------------- |
|         0 | 成功                    |
|        10 | bad request           |
|        11 | not found             |
|        20 | state conflict        |
|        21 | epoch mismatch        |
|        22 | AAD mismatch          |
|        30 | crypto/decrypt failed |
|        40 | storage locked        |
|        50 | internal error        |

---

# 20. 幂等性与崩溃恢复

所有写操作都必须带 `request_id`：

```text
generate_key_package
create_group
add_member
commit_merge
process_welcome
encrypt
decrypt
snapshot import
```

处理逻辑：

```text
1. 收到 request_id
2. 查询 operations 表
3. 如果已成功且 input_hash 相同，直接返回上次 response
4. 如果已成功但 input_hash 不同，返回 REQUEST_ID_CONFLICT
5. 如果之前执行中崩溃，进入 recovery 逻辑
```

典型恢复场景：

## 场景一：add_member 成功，但 Go CLI 崩溃

```text
1. pending_commit 已写入 DB
2. commit/welcome response 可能没被 Go 拿到
3. Go CLI 使用同一个 request_id 重试
4. awiki-mls 返回同一个 commit/welcome
```

## 场景二：commit/welcome 已发送，但 commit_merge 前崩溃

```text
1. operations 表里 pending_commit 状态是 created
2. Go CLI 下次启动可以 list pending commits
3. 用户或自动流程继续 commit_merge
```

增加命令：

```bash
awiki-mls group pending-commits --json-in -
```

Go 包一层：

```bash
awiki-cli p6 group pending-commits --group-id anp-group-001
```

---

# 21. ANP/P6 消息类型

第一阶段建议定义 3 个 wire message：

```text
application/anp-group-mls-commit+json
application/anp-group-mls-welcome+json
application/anp-group-mls-cipher+json
```

## 21.1 Commit

```json
{
  "type": "application/anp-group-mls-commit+json",
  "anp_version": "1.0",
  "group_id": "anp-group-001",
  "sender_did": "did:wba:example.com:agent:alice",
  "message_id": "msg_commit_001",
  "created_at": "2026-04-26T12:00:00Z",
  "mls": {
    "mls_group_id": "base64url...",
    "old_epoch": 0,
    "new_epoch": 1,
    "commit": "base64url..."
  }
}
```

## 21.2 Welcome

```json
{
  "type": "application/anp-group-mls-welcome+json",
  "anp_version": "1.0",
  "group_id": "anp-group-001",
  "sender_did": "did:wba:example.com:agent:alice",
  "target_did": "did:wba:example.com:agent:bob",
  "message_id": "msg_welcome_001",
  "created_at": "2026-04-26T12:00:00Z",
  "mls": {
    "mls_group_id": "base64url...",
    "epoch": 1,
    "welcome": "base64url...",
    "ratchet_tree": "base64url-optional"
  }
}
```

## 21.3 Cipher

```json
{
  "type": "application/anp-group-mls-cipher+json",
  "anp_version": "1.0",
  "group_id": "anp-group-001",
  "sender_did": "did:wba:example.com:agent:alice",
  "message_id": "msg_001",
  "created_at": "2026-04-26T12:00:00Z",
  "content_type": "text/plain",
  "mls": {
    "mls_group_id": "base64url...",
    "epoch": 1,
    "aad": {
      "anp_version": "1.0",
      "profile": "group-e2ee",
      "security_profile": "mls-rfc9420",
      "operation": "group.send",
      "anp_group_id": "anp-group-001",
      "mls_group_id": "base64url...",
      "epoch": 1,
      "sender_did": "did:wba:example.com:agent:alice",
      "message_id": "msg_001",
      "content_type": "text/plain",
      "created_at": "2026-04-26T12:00:00Z"
    },
    "ciphertext": "base64url..."
  }
}
```

---

# 22. CLI 与 HTTP 一致性测试

必须加一个 parity test：

```text
同一个输入：
  CLI 输出
  HTTP 输出

除了时间戳、随机值、request_id 外，语义必须一致。
```

测试矩阵：

```text
1. generate_key_package CLI vs HTTP
2. create_group CLI vs HTTP
3. add_member CLI vs HTTP
4. process_welcome CLI vs HTTP
5. encrypt CLI vs HTTP
6. decrypt CLI vs HTTP
7. restore CLI vs HTTP
```

---

# 23. 第一阶段验收标准

## 23.1 无后台进程测试

这个是你新约束下最重要的验收。

```bash
awiki-cli p6 group create ...
# awiki-mls 进程退出

awiki-cli p6 key-package create ...
# awiki-mls 进程退出

awiki-cli p6 group add-member ...
# awiki-mls 进程退出

awiki-cli p6 welcome process ...
# awiki-mls 进程退出

awiki-cli p6 message encrypt ...
# awiki-mls 进程退出

awiki-cli p6 message decrypt ...
# awiki-mls 进程退出
```

通过标准：

```text
1. 每一步都不依赖常驻进程
2. 每一步都能从 storage restore 状态
3. Alice/Bob 能完成加密解密
4. kill 所有 awiki-mls 进程后，下一次命令仍能继续
```

---

## 23.2 HTTP server 可选测试

```bash
awiki-mls serve --listen 127.0.0.1:8742
AWIKI_MLS_MODE=http awiki-cli p6 message encrypt ...
```

通过标准：

```text
1. HTTP 模式结果与 CLI 模式一致
2. kill HTTP server 后，AWIKI_MLS_MODE=auto 能 fallback 到 exec
3. HTTP server 重启后能恢复 group state
```

---

## 23.3 snapshot 测试

```bash
awiki-cli p6 snapshot export --group-id anp-group-001
rm -rf ~/.awiki/mls/test-restore
awiki-cli p6 snapshot import --snapshot group1.snapshot.enc
awiki-cli p6 group restore --group-id anp-group-001
```

通过标准：

```text
1. snapshot 是加密文件
2. import 后能 restore group
3. epoch 不倒退
4. 不允许默认覆盖更新状态
```

---

# 24. 里程碑拆分

## M0：Rust 单体 CLI PoC

目标：

```text
先不做 HTTP。
只做 awiki-mls CLI。
```

完成：

```text
awiki-mls key-package generate
awiki-mls group create
awiki-mls group add-member
awiki-mls welcome process
awiki-mls message encrypt
awiki-mls message decrypt
```

验收：

```text
Alice/Bob 在没有后台进程的情况下完成加密解密。
```

---

## M1：持久化与 restore

完成：

```text
1. SQLite storage
2. group_bindings
3. operation_log
4. file lock
5. group restore
```

验收：

```text
每个命令都是独立进程。
重启后仍可继续 encrypt/decrypt。
```

---

## M2：Go ExecProvider 接入

完成：

```text
1. Go MLSProvider interface
2. ExecProvider
3. awiki-cli p6 子命令
4. JSON stdin/stdout 调用
```

验收：

```text
awiki-cli 不需要常驻 sidecar。
Go CLI 可以完整跑通 P6 最小闭环。
```

---

## M3：HTTP JSON API

完成：

```text
1. awiki-mls serve
2. /healthz
3. /v1/version
4. 所有核心 HTTP endpoints
5. token auth
```

验收：

```text
AWIKI_MLS_MODE=http 可用。
AWIKI_MLS_MODE=auto 可 fallback。
```

---

## M4：staged add_member 与崩溃恢复

完成：

```text
1. pending_commits
2. commit_merge
3. request_id 幂等
4. pending commit list
```

验收：

```text
add_member 之后 Go CLI 崩溃，重试可恢复。
```

---

## M5：snapshot export/import

完成：

```text
1. 加密 snapshot
2. import 冲突检查
3. epoch 防倒退
4. 默认不开放 HTTP snapshot API
```

---

# 25. 第一阶段不做的事情

为了避免范围失控，第一阶段不做：

```text
1. 不做 OpenMLS Go 重写
2. 不做 Rust FFI
3. 不做常驻 daemon 依赖
4. 不做多设备同步
5. 不做完整 remove member
6. 不做 external commit
7. 不做云端 snapshot 自动备份
8. 不做大型附件直接 MLS 加密
9. 不做跨实现互操作承诺
```

大型附件后续应该采用：

```text
MLS 加密内容密钥
内容本身用 AEAD 流式加密
```

不要直接把大文件塞进 MLS application message。

---

# 26. 推荐默认策略

最终建议：

```text
默认：
  AWIKI_MLS_MODE=exec

开发：
  AWIKI_MLS_MODE=auto

服务化：
  AWIKI_MLS_MODE=http
```

也就是说：

```text
awiki-cli 默认不依赖任何后台进程。
```

HTTP server 只是优化：

```text
1. 减少进程启动开销
2. 方便本地 agent runtime 长期运行
3. 方便调试
4. 方便后续桌面端 / daemon 集成
```

---

# 27. 最终一句话方案

你应该把第一阶段设计成：

> **`awiki-mls` 是一个 Rust OpenMLS 能力二进制，支持 HTTP server mode 和 one-shot CLI mode；`awiki-cli` 默认通过 exec 调用 CLI mode，不依赖常驻后台进程；所有 MLS group state、key package、pending commit、operation log 都持久化到本地 storage，HTTP 和 CLI 共用同一个 Rust engine。**

这样既满足：

```text
Go CLI 当前架构
```

也满足：

```text
不能保证后台进程一直运行
```

还保留了未来升级路径：

```text
exec mode -> HTTP mode -> daemon mode -> Go 原生 MLS provider
```

[1]: https://github.com/openmls/openmls "GitHub - openmls/openmls: Rust implementation of the Messaging Layer Security (MLS) protocol · GitHub"
[2]: https://book.openmls.tech/user_manual/persistence.html "Persistence of group state - OpenMLS Book"
[3]: https://book.openmls.tech/user_manual/aad.html "Using Additional Authenticated Data (AAD) - OpenMLS Book"
