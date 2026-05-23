# Phase 6：group MLS native provider 与存储统一方案

**状态**：设计草案  
**适用范围**：group E2EE / `anp-mls` / OpenMLS operations / im-core secure group runtime  
**目标**：在 `im-core` 已经 Rust 化后，去掉本地 MLS operations 对 `anp-mls` 子进程 JSON 协议的长期依赖；把 `../anp/rust/src/bin/anp-mls.rs` 中真实 OpenMLS operations 抽成 library API，并设计一个符合 OpenMLS 语义、同时与 `im-core` local state 架构统一的 MLS 存储方案。

---

## 1. 结论

长期方案应从：

```text
awiki-cli / im-core
  -> spawn anp-mls binary
      -> stdin/stdout JSON
      -> OpenMLS + anp-mls state.db
  -> message service RPC
```

调整为：

```text
im-core
  -> NativeAnpMlsProvider
      -> anp::group_e2ee::operations library API
      -> OpenMLS StorageProvider backed by im-core secure local SQLite
  -> message service RPC
```

核心决策：

```text
1. 本地 MLS crypto/provider 调用改为 Rust library 直接调用，不再长期依赖 anp-mls binary。
2. message service RPC 继续保留；它负责服务端分发 KeyPackage、Welcome、Commit、notice 和 cipher message，不是本地 MLS provider 的替代品。
3. anp-mls binary 保留为 thin wrapper、compat fallback、调试工具和跨语言入口。
4. OpenMLS 状态必须继续通过 OpenMLS StorageProvider 持久化，不能降级为普通 JSON snapshot 或业务表字段。
5. MLS 存储建议统一进入 im-core local secure SQLite，但必须保持 OpenMLS StorageProvider 的表/删除/事务语义。
6. SDK public Interface 不暴露 anp-mls、OpenMLS、StorageProvider、KeyPackage、Welcome、Commit、MLS epoch 或 provider path。
```

推荐最终形态：

```text
ImCorePaths.local_state.sqlite_path
  -> im_core_local_state.sqlite
     - im-core business tables
     - direct E2EE tables
     - secure outbox table
     - group MLS app metadata tables
     - OpenMLS storage tables, with namespaced table names or dedicated attached schema
```

如果 OpenMLS storage 与业务 local state 共库的锁/迁移风险过高，备选方案是：

```text
ImCorePaths.local_state.sqlite_path
  -> im_core_local_state.sqlite

ImCorePaths.local_state secure sibling
  -> im_core_mls_state.sqlite
```

但不建议长期继续使用：

```text
workspace/mls/agents/<agent-hash>/<device>/state.db
workspace/mls/agents/<agent-hash>/<device>/state.lock
```

作为 SDK 主路径。它可以作为迁移导入源和 compat fallback。

---

## 2. 当前状态

当前 `../anp/rust` 是一个 Rust crate：

```text
package: anp
lib: src/lib.rs
bin: src/bin/anp-mls.rs
feature: mls
```

`anp` library 当前暴露：

```rust
pub mod direct_e2ee;
pub mod group_e2ee;
```

但 `group_e2ee/mod.rs` 当前主要是 wire helper、data model、AAD helper 和 contract-test artifact。真实 OpenMLS operations 仍主要在：

```text
../anp/rust/src/bin/anp-mls.rs
```

当前 `awiki-cli` 通过 `MlsExecProvider` 调用 `anp-mls`：

```text
Command::new(anp-mls)
  args: <domain> <action> --json-in - --data-dir <dir>
  stdin: JSON request
  stdout: JSON response
```

`anp-mls` real mode 当前行为：

```text
1. 要求 --data-dir。
2. 创建 data_dir。
3. 获取 data_dir/state.lock 文件锁。
4. 打开 data_dir/state.db。
5. 初始化 app schema。
6. 初始化 OpenMLS SqliteStorageProvider。
7. dispatch command：
   - key-package generate
   - group create
   - group add-member
   - group remove-member
   - group leave
   - welcome process
   - commit process / notice process
   - message encrypt
   - message decrypt
   - group status
8. 写入 operations、agents、key_packages、group_bindings、pending_commits 等 app metadata。
9. OpenMLS 自己写入 openmls_* tables。
```

当前 `im-core` 已经依赖 workspace `anp`：

```toml
anp = { path = "../anp/rust", default-features = false }
```

但 `im-core` 尚未启用 `anp/mls`，也没有直接调用 group MLS operations。

---

## 3. 为什么不继续走子进程协议

子进程 JSON 协议在 Go im-core / Go CLI 时代是合理选择，因为：

```text
1. OpenMLS 是 Rust 库。
2. Go 进程不能自然链接 Rust crate。
3. 子进程边界隔离了 panic、依赖和内存。
4. JSON 协议方便跨语言。
```

现在 `im-core` 已经迁到 Rust，继续长期走子进程会带来不必要复杂度：

```text
1. 每次 MLS 操作都 spawn process，有性能和平台差异成本。
2. provider path / AWIKI_ANP_MLS_BINARY / PATH discovery 成为 SDK host 配置细节。
3. JSON Value 往返导致类型安全弱，错误分类只能靠字符串。
4. stdout/stderr 解析和 timeout 成为业务链路的一部分。
5. data_dir 和 file lock 由外部 binary 管理，和 im-core local_state 生命周期割裂。
6. 很难在 SDK unit/contract tests 中直接 fake OpenMLS operations。
```

长期应把子进程降级为 compat/fallback，不作为默认 provider path。

---

## 4. 目标架构

### 4.1 anp crate 内部重构

把 `src/bin/anp-mls.rs` 中真实 OpenMLS operations 抽到 library。`anp::group_e2ee` 不设计长时间运行的 runtime，不持有后台任务，不启动 event loop，不管理 SDK 生命周期；它只提供 one-shot MLS operation functions。

```text
../anp/rust/src/group_e2ee/
  mod.rs
  models.rs              # 现有 public wire/domain model，可逐步拆分
  aad.rs                 # build_send_aad 等 helper
  operations.rs          # 新增：one-shot MLS operation functions
  storage.rs             # 新增：OpenMLS provider + app metadata repository
  commands.rs            # 新增：anp-mls/v1 command dispatcher
  errors.rs              # 新增：typed errors
  contract.rs            # 可选：contract-test artifacts

../anp/rust/src/bin/anp-mls.rs
  # 只保留：
  # - CLI args parse
  # - stdin/stdout JSON envelope
  # - --data-dir compat wiring
  # - 调用 anp::group_e2ee::commands
```

library API 不应以 `serde_json::Value` 作为核心类型。可以提供两层 API：

```text
typed operations API：
  给 im-core 调用，使用强类型 input/output/error。

compat command API：
  给 anp-mls binary 复用，保持 anp-mls/v1 JSON command 兼容。
```

### 4.2 im-core internal provider

`im-core` 内部定义 group MLS provider trait：

```rust
pub(crate) trait GroupMlsProvider {
    fn status(&self, input: GroupMlsStatusInput) -> ImResult<GroupMlsStatusOutput>;
    fn generate_key_package(
        &self,
        input: GenerateGroupKeyPackageInput,
    ) -> ImResult<GroupKeyPackageOutput>;
    fn create_group(&self, input: CreateMlsGroupInput) -> ImResult<CreateMlsGroupOutput>;
    fn add_member(&self, input: AddMlsMemberInput) -> ImResult<AddMlsMemberOutput>;
    fn remove_member(&self, input: RemoveMlsMemberInput) -> ImResult<RemoveMlsMemberOutput>;
    fn prepare_update(&self, input: PrepareMlsUpdateInput) -> ImResult<PrepareMlsCommitOutput>;
    fn finalize_commit(&self, input: FinalizeMlsCommitInput) -> ImResult<FinalizeMlsCommitOutput>;
    fn abort_commit(&self, input: AbortMlsCommitInput) -> ImResult<AbortMlsCommitOutput>;
    fn process_welcome(&self, input: ProcessMlsWelcomeInput) -> ImResult<ProcessMlsWelcomeOutput>;
    fn process_notice(&self, input: ProcessMlsNoticeInput) -> ImResult<ProcessMlsNoticeOutput>;
    fn encrypt(&self, input: GroupMlsEncryptInput) -> ImResult<GroupMlsEncryptOutput>;
    fn decrypt(&self, input: GroupMlsDecryptInput) -> ImResult<GroupMlsDecryptOutput>;
}
```

实现：

```text
NativeAnpMlsProvider
  默认长期实现，直接调用 anp::group_e2ee::operations。

ExecAnpMlsProvider
  迁移期 fallback，继续调用 anp-mls binary。

FakeGroupMlsProvider
  unit/contract test helper。
```

这些 provider 都是 `im-core` internal，不进入 public SDK API。

---

## 5. OpenMLS 存储事实

OpenMLS 不是无状态加密函数。它的核心对象 `MlsGroup` 会把 group state、epoch key material、pending proposals/commits 等内容持久化到 `StorageProvider`。

对架构设计的含义：

```text
1. MLS state 必须持久化；不能只存在内存。
2. MLS state 包含私密 key material，应视为 secure local state。
3. OpenMLS 会为了 forward secrecy 删除旧 key material；存储层必须保留删除语义，不能通过业务 snapshot、append-only log 或透明备份把旧 key material 复活。
4. 不能把 OpenMLS 内部表拆成 SDK public DTO。
5. 不能把 MLS state 当作普通业务 metadata 随意同步、导出或打印。
```

因此“统一存储”可以做，但应该是：

```text
统一 SQLite 文件 / 连接 / migration 管理
```

而不是：

```text
把 OpenMLS state 手工序列化进一张 JSON 表
```

---

## 6. 存储方案比较

### 6.1 方案 A：继续使用独立 MLS data_dir/state.db

形态：

```text
local_state.sqlite
mls/agents/<agent>/<device>/state.db
mls/agents/<agent>/<device>/state.lock
```

优点：

```text
1. 改动小。
2. 兼容当前 anp-mls。
3. OpenMLS state 和业务 DB 强隔离。
4. 子进程 fallback 最容易保留。
```

问题：

```text
1. 与 im-core local_state 生命周期割裂。
2. 多 identity/device 的目录布局继续泄漏到底层设计。
3. backup/restore/migration/doctor 要同时理解两个存储根。
4. SDK host 仍需要感知 MLS path 或 SecureRuntimePaths。
5. 不符合“im-core Rust 化后统一本地状态”的长期方向。
```

结论：只适合作为迁移期 fallback / import source，不建议作为长期默认。

### 6.2 方案 B：独立 MLS SQLite 文件，但由 im-core 管理

形态：

```text
local_state.sqlite
mls_state.sqlite
```

优点：

```text
1. im-core 统一管理路径，不再暴露 anp-mls data_dir。
2. OpenMLS storage 独占 SQLite 文件，迁移和锁风险较低。
3. 比方案 A 更容易备份/诊断。
4. Exec fallback 可通过一个临时/compat data_dir 或直接指定 mls_state.sqlite 适配。
```

问题：

```text
1. 仍然是两个数据库。
2. group message projection 与 MLS state 更新很难同事务提交。
3. direct E2EE / secure outbox / message cache 与 MLS state 仍有一定割裂。
```

结论：是稳妥过渡方案，但不是最统一的长期目标。

### 6.3 方案 C：统一到 im-core local SQLite，同库不同表

形态：

```text
im_core_local_state.sqlite
  contacts / messages / groups / direct_e2ee / e2ee_outbox
  group_mls_operations
  group_mls_agents
  group_mls_key_packages
  group_mls_bindings
  group_mls_pending_commits
  openmls_* tables
```

优点：

```text
1. 一个 local_state root，一个 SQLite 文件，SDK host 不需要理解 MLS path。
2. `owner_identity_id` / device / group 的隔离规则可以统一。
3. group send/decrypt projection、pending notice processing、local message cache 可在同一 SQLite 事务边界内协调。
4. doctor、migration、backup、local reset 的入口统一。
5. 和 direct E2EE SQLite 迁移方向一致。
```

风险：

```text
1. OpenMLS SqliteStorageProvider 默认表名可能与业务 migration 管理混用，需要 namespace 约束或 storage wrapper。
2. OpenMLS storage 可能自己运行 migrations；必须避免和 im-core migration transaction 冲突。
3. SQLite writer lock 竞争更集中，group E2EE 操作可能阻塞普通 message cache 写入。
4. 备份策略必须尊重 OpenMLS forward secrecy 删除语义，不能默认做长期明文历史备份。
```

结论：推荐长期方案，但需要在 `anp` library 层抽象 storage open/migration/lock，不要让 im-core 直接操作 OpenMLS 表。

### 6.4 方案 D：自定义 OpenMLS StorageProvider，映射到 im-core repository

形态：

```text
OpenMLS StorageProvider trait
  -> im-core typed repositories
  -> local_state.sqlite
```

优点：

```text
1. 可以完全控制表名、owner_identity_id、事务、删除策略。
2. 与 im-core 架构最一致。
```

问题：

```text
1. 实现成本最高。
2. 必须完整、正确实现 OpenMLS StorageProvider。
3. 容易破坏 OpenMLS 版本升级兼容。
4. 安全风险高，尤其是旧 key material 删除和 pending state 一致性。
```

结论：不建议 Phase 6 自研。优先复用 `openmls_sqlite_storage::SqliteStorageProvider`。

---

## 7. 推荐存储设计

推荐采用：

```text
方案 C：统一到 im-core local SQLite，同库不同表。
```

但要加几个硬约束：

```text
1. OpenMLS 内部状态只能通过 anp::group_e2ee::operations/storage 调用 OpenMLS StorageProvider 访问。
2. im-core 只管理 SQLite 文件、连接生命周期、owner identity/device scope、事务入口和 high-level metadata。
3. im-core 不直接读写 openmls_* tables。
4. app metadata tables 必须 namespaced 为 group_mls_*。
5. 所有 MLS private state 不进入 public DTO、不进入日志、不进入 diagnostics raw output。
6. backup/export 默认排除 MLS private state，除非用户显式选择 secure local-state backup。
```

### 7.1 SQLite 文件

使用：

```text
ImCorePaths.local_state.sqlite_path
```

不新增 SDK public path 字段。

如实现期发现 `openmls_sqlite_storage` 无法安全和业务 local_state 共用连接或 migrations，则降级为 internal sibling path：

```text
<local_state_dir>/mls_state.sqlite
```

这个 sibling path 由 im-core 内部从 `ImCorePaths.local_state.sqlite_path` 推导，不进入 public Interface。

### 7.2 group MLS metadata tables

建议把当前 `anp-mls` app schema 改造成 owner-aware tables：

```sql
CREATE TABLE IF NOT EXISTS group_mls_operations (
    owner_identity_id TEXT NOT NULL,
    device_id         TEXT NOT NULL,
    operation_id      TEXT NOT NULL,
    command           TEXT NOT NULL,
    input_digest      TEXT NOT NULL,
    response_json     TEXT NOT NULL,
    status            TEXT NOT NULL,
    created_at        TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at        TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (owner_identity_id, device_id, operation_id)
);

CREATE TABLE IF NOT EXISTS group_mls_agents (
    owner_identity_id    TEXT NOT NULL,
    owner_did            TEXT NOT NULL,
    device_id            TEXT NOT NULL,
    signature_public_key BLOB NOT NULL,
    signature_scheme     TEXT NOT NULL,
    created_at           TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at           TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (owner_identity_id, device_id)
);

CREATE TABLE IF NOT EXISTS group_mls_key_packages (
    owner_identity_id TEXT NOT NULL,
    owner_did         TEXT NOT NULL,
    device_id         TEXT NOT NULL,
    key_package_id    TEXT NOT NULL,
    public_json       TEXT NOT NULL,
    status            TEXT NOT NULL,
    created_at        TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    consumed_at       TEXT,
    PRIMARY KEY (owner_identity_id, device_id, key_package_id)
);

CREATE TABLE IF NOT EXISTS group_mls_bindings (
    owner_identity_id       TEXT NOT NULL,
    owner_did               TEXT NOT NULL,
    device_id               TEXT NOT NULL,
    group_did               TEXT NOT NULL,
    crypto_group_id_b64u    TEXT NOT NULL,
    openmls_group_id_b64u   TEXT NOT NULL,
    epoch                   INTEGER NOT NULL,
    role                    TEXT NOT NULL,
    status                  TEXT NOT NULL,
    updated_at              TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (owner_identity_id, device_id, group_did)
);

CREATE TABLE IF NOT EXISTS group_mls_pending_commits (
    owner_identity_id       TEXT NOT NULL,
    device_id               TEXT NOT NULL,
    pending_commit_id       TEXT NOT NULL,
    operation_id            TEXT NOT NULL,
    command                 TEXT NOT NULL,
    owner_did               TEXT NOT NULL,
    group_did               TEXT NOT NULL,
    crypto_group_id_b64u    TEXT NOT NULL,
    subject_did             TEXT NOT NULL,
    subject_status          TEXT NOT NULL,
    from_epoch              INTEGER NOT NULL,
    to_epoch                INTEGER NOT NULL,
    commit_b64u             TEXT NOT NULL,
    ratchet_tree_b64u       TEXT,
    group_info_b64u         TEXT,
    epoch_authenticator_b64u TEXT,
    status                  TEXT NOT NULL,
    response_json           TEXT NOT NULL,
    created_at              TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at              TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (owner_identity_id, device_id, pending_commit_id),
    UNIQUE (owner_identity_id, device_id, operation_id)
);
```

这些表保存的是 group MLS app metadata 和 public/delivery artifacts。OpenMLS private material 仍由 `openmls_sqlite_storage` 的 `openmls_*` tables 管理。

### 7.3 owner / device scope

当前 `anp-mls` 使用：

```text
agent_did
device_id
```

SDK 长期应使用：

```text
owner_identity_id
owner_did
device_id
```

规则：

```text
1. owner_identity_id 是本地隔离主键。
2. owner_did 是协议身份和可读诊断字段。
3. device_id 默认 default，但必须保留多设备扩展。
4. group_did 是业务 group identity。
5. openmls_group_id_b64u 是 OpenMLS group id，不进入 public SDK DTO。
```

### 7.4 lock / transaction

子进程时代需要 `state.lock` 文件锁。Native provider 后应改成 SQLite transaction + process-local mutex：

```text
1. 同一 owner_identity_id/device_id 的 MLS mutating operation 串行执行。
2. 每个 create/add/remove/update/recover/process/encrypt/decrypt 操作用 IMMEDIATE transaction。
3. 操作内同时更新 group_mls_* metadata 和 OpenMLS StorageProvider。
4. 如果 OpenMLS StorageProvider 必须独立 connection，则用同一个 sqlite file + short-lived transaction，并通过 im-core scoped lock 串行化。
5. 不再使用 public path 上的 state.lock 作为主锁。
```

如果后续仍需要跨进程安全，例如用户同时运行多个 CLI 进程，则必须保留 SQLite 层面的跨进程 writer 串行能力：

```text
PRAGMA journal_mode=WAL
PRAGMA busy_timeout=<reasonable>
BEGIN IMMEDIATE
```

process-local mutex 只能降低同进程竞争，不能替代 SQLite transaction。

### 7.5 backup / export

MLS storage 包含 forward secrecy 敏感材料。默认策略：

```text
1. 普通 message export 不包含 OpenMLS private state。
2. diagnostics 不输出 openmls_* raw rows。
3. backup 如果包含 local_state.sqlite，必须标记为 secure backup。
4. 不做 append-only MLS snapshot。
5. 不在 operation response log 中持久化 application plaintext。
6. 继续保留当前 response_for_operation_log 的 redaction 原则。
```

---

## 8. anp library API 设计

### 8.1 one-shot operations API

建议 `anp` 暴露一个 feature-gated operations API：

```rust
#[cfg(feature = "mls")]
pub mod group_e2ee {
    pub mod operations;
}
```

核心类型示意：

```rust
pub struct GroupMlsContext<'a, S> {
    pub store: &'a S,
    pub owner_identity_id: String,
    pub owner_did: String,
    pub device_id: String,
}

pub trait GroupMlsStore {
    type Transaction<'a>;

    fn begin_operation<'a>(
        &'a self,
        scope: GroupMlsScope<'a>,
    ) -> Result<Self::Transaction<'a>, GroupMlsError>;
}

pub struct GroupMlsScope<'a> {
    pub owner_identity_id: &'a str,
    pub owner_did: &'a str,
    pub device_id: &'a str,
}
```

Operation functions：

```rust
pub fn status<S>(
    ctx: &GroupMlsContext<'_, S>,
    input: StatusInput,
) -> Result<StatusOutput, GroupMlsError>
where
    S: GroupMlsStore;

pub fn generate_key_package<S>(
    ctx: &GroupMlsContext<'_, S>,
    input: GenerateKeyPackageInput,
) -> Result<GenerateKeyPackageOutput, GroupMlsError>
where
    S: GroupMlsStore;

pub fn create_group<S>(
    ctx: &GroupMlsContext<'_, S>,
    input: CreateGroupInput,
) -> Result<CreateGroupOutput, GroupMlsError>
where
    S: GroupMlsStore;

pub fn process_welcome<S>(
    ctx: &GroupMlsContext<'_, S>,
    input: ProcessWelcomeInput,
) -> Result<ProcessWelcomeOutput, GroupMlsError>
where
    S: GroupMlsStore;

pub fn encrypt<S>(
    ctx: &GroupMlsContext<'_, S>,
    input: EncryptInput,
) -> Result<EncryptOutput, GroupMlsError>
where
    S: GroupMlsStore;

pub fn decrypt<S>(
    ctx: &GroupMlsContext<'_, S>,
    input: DecryptInput,
) -> Result<DecryptOutput, GroupMlsError>
where
    S: GroupMlsStore;
```

这些函数每次调用显式打开 operation scope / transaction，执行后返回；不创建长期运行对象，不持有后台 worker，不保存跨调用内存状态。

### 8.2 command compatibility layer

保留 anp-mls/v1 JSON command compatibility：

```rust
pub fn execute_compat_command(
    ctx: &GroupMlsContext<'_, impl GroupMlsStore>,
    command: &str,
    request: serde_json::Value,
) -> Result<serde_json::Value, GroupMlsCommandError>;
```

`src/bin/anp-mls.rs` 变成：

```text
parse args
read stdin JSON
open CompatDataDirStore
execute_compat_command(...)
print JSON response
```

这样 binary 行为可以保持不变，但真实 operations 不再被困在 bin target。

---

## 9. im-core 接入方式

`im-core` feature：

```toml
[features]
group-e2ee = ["anp/mls", "sqlite"]
```

内部模块：

```text
crates/im-core/src/internal/group_e2ee/
  mod.rs
  provider.rs             # GroupMlsProvider trait
  native_provider.rs      # NativeAnpMlsProvider
  exec_provider.rs        # optional compat fallback
  storage.rs              # im-core local_state -> anp GroupMlsStore adapter
  runtime.rs              # group E2EE orchestration
  transport.rs            # message service RPC transport
  projection.rs           # decrypt/send local projection
```

Public API 不变：

```rust
client.messages().send(SendMessageRequest {
    target: MessageTarget::Group(group),
    security: MessageSecurityPolicy::E2eeRequired,
    ..
})

client.secure().group(group).status()
client.secure().group(group).prepare()
client.secure().group(group).repair()
```

SDK 不新增：

```text
set_anp_mls_binary(...)
set_mls_data_dir(...)
process_key_package(...)
process_welcome(...)
process_commit(...)
raw_mls_provider(...)
```

---

## 10. 迁移路径

### PR M1：anp MLS operations extraction

目标：

```text
把 src/bin/anp-mls.rs 的 real OpenMLS operations 抽到 anp::group_e2ee::operations/storage/commands。
```

完成标准：

```text
1. anp-mls binary 输出兼容现有 contract tests。
2. binary 只做 CLI/JSON envelope。
3. real_key_package / real_group_create / real_message_encrypt / real_message_decrypt 等逻辑不再只存在于 bin target。
4. library API 有 typed errors。
```

### PR M2：storage abstraction

目标：

```text
抽出 GroupMlsStore，并提供两个 store：
1. CompatDataDirStore：兼容 --data-dir/state.db。
2. ImCoreSqliteGroupMlsStore：面向 im-core local SQLite。
```

完成标准：

```text
1. CompatDataDirStore 让 anp-mls binary 行为不变。
2. ImCoreSqliteGroupMlsStore 不需要 anp-mls binary path。
3. OpenMLS tables 仍由 openmls_sqlite_storage migrations 管理。
4. group_mls_* metadata tables 用 owner_identity_id 隔离。
```

### PR M3：im-core NativeAnpMlsProvider

目标：

```text
im-core group-e2ee feature 下新增 NativeAnpMlsProvider。
```

完成标准：

```text
1. im-core 直接调用 anp library。
2. 不 spawn anp-mls。
3. provider path 不进入 public API。
4. fake provider 可覆盖 status/prepare/repair/send/decrypt tests。
```

### PR M4：group send/decrypt 切到 native provider

目标：

```text
group E2EE send/decrypt/status/repair 默认走 NativeAnpMlsProvider。
```

完成标准：

```text
1. ExecAnpMlsProvider 只作为 fallback feature 或 compat adapter。
2. group E2EE send 成功后仍通过 message service RPC 发送 cipher。
3. local projection 不保存 application plaintext 到 MLS operation log。
4. status DTO 不暴露 MLS epoch / KeyPackage / raw notice / OpenMLS group id。
```

### PR M5：legacy data import

目标：

```text
提供从 legacy mls/agents/<agent>/<device>/state.db 导入或迁移到 im-core local SQLite 的工具。
```

建议：

```text
1. 未上线场景可以直接 clean migration。
2. 如果需要保留本地开发数据，提供 explicit import command/helper。
3. 不做自动静默合并，避免误把旧 key material 复制到多个位置。
4. import 完成后 legacy data_dir 只读，不再作为默认写入目标。
```

---

## 11. 风险和约束

### 11.1 OpenMLS 版本耦合

直接依赖 library 后，`im-core` 和 `anp` 的 OpenMLS 版本会一起升级。需要：

```text
1. workspace dependency pin。
2. anp/mls feature compatibility tests。
3. anp-mls binary compatibility tests 继续保留。
```

### 11.2 panic / process isolation

子进程边界消失后，panic 会影响 im-core process。需要：

```text
1. anp operations 尽量不用 unwrap/expect 处理输入。
2. im-core provider 边界做 error mapping。
3. 对高风险 native calls 可用 catch_unwind 包住 provider boundary，但不要依赖它处理内存安全问题。
```

### 11.3 SQLite contention

统一 local SQLite 后，MLS mutating operations 和 message cache 会争用 writer lock。需要：

```text
1. MLS 操作短事务。
2. busy_timeout。
3. 每个 owner/device 串行化。
4. 避免在 transaction 内做网络 RPC；网络调用前 prepare，网络成功后 finalize，失败后 abort。
```

### 11.4 forward secrecy

不能因为“统一存储”破坏 OpenMLS 删除旧 key material 的语义：

```text
1. 不做 MLS state append-only audit log。
2. 不把 openmls_* rows 复制到 diagnostics。
3. 不在 operation log 里保存 plaintext。
4. backup/export 必须标记 secure，并有清晰恢复语义。
```

---

## 12. 与 SDK Interface 的关系

这个方案不改变 SDK public interface。

调用方仍然只看到：

```rust
client.messages().send(... E2eeRequired ...)
client.secure().group(group).status()
client.secure().group(group).prepare()
client.secure().group(group).repair()
```

调用方不会看到：

```text
anp-mls binary path
OpenMLS StorageProvider
OpenMLS group id
MLS epoch
KeyPackage raw bytes
Welcome / Commit raw body
pending notice raw payload
state.db path
state.lock path
```

---

## 13. 最终建议

一步到位的长期方案应定为：

```text
1. 先在 ../anp/rust 把 anp-mls real OpenMLS operations 抽成 library API。
2. im-core 通过 NativeAnpMlsProvider 直接调用 anp library。
3. anp-mls binary 保留为 thin wrapper 和 compat fallback。
4. MLS storage 统一进入 im-core local SQLite，但 OpenMLS private state 仍由 OpenMLS StorageProvider 管理。
5. group_mls_* metadata 表按 owner_identity_id / device_id 隔离。
6. 不把 provider/path/OpenMLS 细节暴露到 SDK Interface。
7. message service RPC 继续保留。
```

如果实现时发现 `openmls_sqlite_storage` 很难与现有 local_state 共用同一个 SQLite 文件，则接受一个 internal `mls_state.sqlite` sibling 作为过渡，但仍由 `ImCorePaths.local_state` 推导，不能进入 SDK public Interface。
