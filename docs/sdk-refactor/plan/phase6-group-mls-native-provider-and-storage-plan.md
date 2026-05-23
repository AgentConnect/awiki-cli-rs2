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
      -> OpenMLS StorageProvider backed by im-core-managed sensitive MLS SQLite
  -> message service RPC
```

关键分层边界：

```text
im-core:
  只调用 anp::group_e2ee::operations，不直接读写 openmls_* tables。
  负责 SDK 编排、identity/auth、message service RPC、local projection 和 public DTO。

anp::group_e2ee::operations:
  编排 one-shot MLS 操作，不持有长期 runtime 状态。
  所有可能推进 MLS epoch 的操作只做 prepare/finalize/abort 边界，不在 prepare 后静默推进本地 binding epoch。

anp::group_e2ee::storage:
  统一管理 OpenMLS StorageProvider + group_mls_* metadata tables。
  负责打开/migrate/lock SQLite storage，并隐藏 openmls_* 表结构。

OpenMLS:
  继续通过 StorageProvider 管 openmls_* private state。
```

核心决策：

```text
1. 本地 MLS crypto/provider 调用改为 Rust library 直接调用，不再长期依赖 anp-mls binary。
2. message service RPC 继续保留；它负责服务端分发 KeyPackage、Welcome、Commit、notice 和 cipher message，不是本地 MLS provider 的替代品。
3. anp-mls binary 保留为 thin wrapper、compat fallback、调试工具和跨语言入口。
4. OpenMLS 状态必须继续通过 OpenMLS StorageProvider 持久化，不能降级为普通 JSON snapshot 或业务表字段。
5. MLS 存储建议统一进入 im-core-managed sensitive local SQLite，但必须保持 OpenMLS StorageProvider 的表/删除/事务语义。
6. SDK public Interface 不暴露 anp-mls、OpenMLS、StorageProvider、KeyPackage、Welcome、Commit、MLS epoch 或 provider path。
7. 同库同事务只能作为经过 spike 验证后的目标，不能在首个 native provider 版本默认承诺。
```

这里的 sensitive local SQLite 不是承诺已经有 at-rest encryption。Phase 6 当前只承诺：

```text
1. MLS private state 是 sensitive local state。
2. 不进入 public DTO。
3. 不进入日志。
4. 不进入普通 diagnostics/export。
5. 路径由 im-core 管理。
6. 依赖 OS/user profile 文件权限。
```

如果需要真正的本地静态加密，应另开设计：

```text
SQLCipher
platform key wrapping
per-identity local-state encryption key
backup/restore key handling
plaintext sqlite migration
```

推荐长期形态：

```text
ImCorePaths.local_state.sqlite_path
  -> im_core_local_state.sqlite
     - im-core business tables
     - direct E2EE tables
     - secure outbox table
     - group MLS app metadata tables
     - OpenMLS storage tables, with namespaced table names or dedicated attached schema
```

首个可落地版本不强制同库同事务。如果 OpenMLS storage 与业务 local state 共库的锁/迁移风险过高，允许使用 internal sibling：

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

    fn create_group_prepare(
        &self,
        input: CreateMlsGroupInput,
    ) -> ImResult<PreparedMlsCommitOutput>;
    fn add_member_prepare(
        &self,
        input: AddMlsMemberInput,
    ) -> ImResult<PreparedMlsCommitOutput>;
    fn remove_member_prepare(
        &self,
        input: RemoveMlsMemberInput,
    ) -> ImResult<PreparedMlsCommitOutput>;
    fn leave_prepare(
        &self,
        input: LeaveMlsGroupInput,
    ) -> ImResult<PreparedMlsCommitOutput>;
    fn update_member_prepare(
        &self,
        input: UpdateMlsMemberInput,
    ) -> ImResult<PreparedMlsCommitOutput>;
    fn recover_member_prepare(
        &self,
        input: RecoverMlsMemberInput,
    ) -> ImResult<PreparedMlsCommitOutput>;

    fn finalize_commit(&self, input: FinalizeMlsCommitInput) -> ImResult<FinalizeMlsCommitOutput>;
    fn abort_commit(&self, input: AbortMlsCommitInput) -> ImResult<AbortMlsCommitOutput>;

    fn process_welcome(&self, input: ProcessMlsWelcomeInput) -> ImResult<ProcessMlsWelcomeOutput>;
    fn process_notice(&self, input: ProcessMlsNoticeInput) -> ImResult<ProcessMlsNoticeOutput>;
    fn encrypt(&self, input: GroupMlsEncryptInput) -> ImResult<GroupMlsEncryptOutput>;
    fn decrypt(&self, input: GroupMlsDecryptInput) -> ImResult<GroupMlsDecryptOutput>;
}
```

所有可能推进 MLS epoch 的操作都必须遵守：

```text
1. local prepare：
   生成 commit / welcome / ratchet_tree / group_info 等 public delivery artifacts；
   持久化 pending_commit；
   不更新 active binding epoch 为新 epoch。
2. service RPC：
   im-core 把 prepared artifacts 提交给 message service。
3. service accepted：
   finalize/merge pending commit；
   更新 binding epoch/status；
   写 local projection。
4. service rejected / network failed：
   abort pending commit，或保留 pending/retry 状态；
   不推进 local binding epoch。
```

当前 `anp-mls` 的 `group add-member` 和 `group create` 路径需要在 extraction 后修正为 prepare/finalize/abort 语义，不能把现有 one-shot merge 行为直接作为 native provider 长期实现。

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
2. MLS state 包含私密 key material，应视为 sensitive local state。
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

长期目标采用：

```text
方案 C：统一到 im-core local SQLite，同库不同表。
```

但它必须经过 PR0 spike 验证后才能作为默认实现。首个可落地版本可以采用：

```text
1. same-file / two-connection：
   im_core_local_state.sqlite 同库；
   im-core business connection 和 openmls_sqlite_storage connection 分开；
   不承诺同一个 Rust transaction object。

2. internal sibling mls_state.sqlite：
   <local_state_dir>/mls_state.sqlite；
   路径由 ImCorePaths.local_state.sqlite_path 推导；
   不进入 SDK public Interface。
```

PR0 需要验证：

```text
1. openmls_sqlite_storage 是否能安全打开 im-core local_state.sqlite。
2. OpenMLS migrations 是否影响 im-core PRAGMA user_version / schema migration。
3. OpenMLS storage 操作是否能接收外部 rusqlite connection 或 transaction。
4. OpenMLS mutating operation 是否可纳入 im-core BEGIN IMMEDIATE 边界。
5. crash after prepare / before finalize 后 pending_commit 是否可恢复或 abort。
6. SQLite writer lock / busy_timeout 对 message cache 和 realtime projection 的影响。
```

PR0 输出必须是以下之一：

```text
A. 可同库同事务。
B. 可同库不同 connection，但不能承诺同事务。
C. 只能 internal sibling mls_state.sqlite。
```

当前 spike 证据记录：

```text
测试文件：../anp/rust/tests/group_e2ee_storage_spike_tests.rs
命令：cargo test --test group_e2ee_storage_spike_tests --features mls -- --nocapture
结果：4 passed
```

当前 M3 落地证据记录：

```text
../anp/rust local commit 8aeb96f
  feat: add im-core scoped group mls store

测试文件：../anp/rust/tests/group_e2ee_typed_operations_tests.rs
命令：cargo test --test group_e2ee_typed_operations_tests --features mls
结果：2 passed
```

已验证事实：

```text
1. openmls_sqlite_storage 可以在 im-core-like SQLite 文件中创建 openmls_* tables。
2. OpenMLS / anp-mls migrations 未覆盖已有 PRAGMA user_version = 13。
3. 同文件双连接可共存，但当 im-core-like connection 持有 BEGIN IMMEDIATE 写事务时，anp-mls 另一连接无法加入同一事务，只会遇到 SQLite locked/busy。
4. 初始 spike 曾验证旧 group add-member 会立即 merge pending commit 并把 binding epoch 推进到 1；不能作为 NativeAnpMlsProvider 的长期语义。
5. 当前 group add-member 已在 ../anp/rust local commit 54982b6 改为 prepare 语义：prepare 返回 pending_commit_id，local_epoch 仍停留在旧 epoch，pending_commits 可见。
6. 当前 group remove-member 已是 prepare 语义：prepare 返回 pending commit，local_epoch 仍停留在旧 epoch，pending_commits 可见。
7. 当前 group create 已改为 metadata-level prepare 语义：OpenMLS 本地 group state 先创建，但 binding 状态为 pending_create；finalize 后才 active，abort 会清理 pending binding 和对应 openmls group state。
```

基于当前证据，默认落地路径应按 B/C 设计：

```text
优先实现 same-file / two-connection 或 internal sibling mls_state.sqlite。
不要在 M2/M3 承诺 OpenMLS private state 与 im-core business projection 同一个 Rust transaction 原子提交。
同库同事务只有在后续证明 openmls_sqlite_storage 可接收外部 transaction 后才能升级。
```

M3 当前实现选择：

```text
internal sibling scoped mls_state.sqlite。

原因：
1. openmls_sqlite_storage 的 openmls_* tables 不带 owner_identity_id / device_id。
2. OpenMLS group id 对同一个业务 group_did 可能在不同本地 identity/device 下相同。
3. 如果把多个 owner/device 的 OpenMLS private state 放进同一个 SQLite 文件，仅靠 group_mls_* metadata 隔离还不够。
4. 因此 ImCoreSqliteGroupMlsStore 先从 ImCorePaths.local_state.sqlite_path 推导：
   <local_state_dir>/group_mls/<owner-device-scope-hash>/mls_state.sqlite
5. sibling path 仍是 im-core/anp 内部实现细节，不进入 SDK public Interface。
6. 每个 scoped DB 内，OpenMLS private state 继续由 openmls_sqlite_storage 管 openmls_* tables；app metadata 使用 group_mls_* tables。
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

如 PR0 发现 `openmls_sqlite_storage` 无法安全和业务 local_state 共用连接或 migrations，则降级为 internal sibling path：

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
    redaction_version TEXT NOT NULL DEFAULT 'v1',
    contains_sensitive INTEGER NOT NULL DEFAULT 0,
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

`group_mls_operations.response_json` 必须使用系统化 redaction policy。

允许保存：

```text
operation_id
command
status
epoch / group_state_ref
commit / welcome / ratchet_tree / group_info 等 public delivery artifact
group_cipher_object
error code/category
redacted markers
```

禁止保存：

```text
application_plaintext
user message plaintext
private key material
OpenMLS private state raw rows
unredacted decrypted payload
raw openmls_* row dump
```

`contains_sensitive` 默认必须是 `0`。如果某个兼容路径无法证明 response 已脱敏，必须拒绝持久化或设置显式 sensitive marker 并禁止普通 diagnostics/export 读取。

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
6. group_mls_* 表的 owner_identity_id 必须 NOT NULL。
7. 进入 Phase 6 前必须确认 identity runtime 能稳定提供 owner_identity_id。
8. MLS 表不能沿用 owner_did-only 的旧兼容隔离方式。
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

pub fn create_group_prepare<S>(
    ctx: &GroupMlsContext<'_, S>,
    input: CreateGroupInput,
) -> Result<PreparedMlsCommitOutput, GroupMlsError>
where
    S: GroupMlsStore;

pub fn add_member_prepare<S>(
    ctx: &GroupMlsContext<'_, S>,
    input: AddMemberInput,
) -> Result<PreparedMlsCommitOutput, GroupMlsError>
where
    S: GroupMlsStore;

pub fn remove_member_prepare<S>(
    ctx: &GroupMlsContext<'_, S>,
    input: RemoveMemberInput,
) -> Result<PreparedMlsCommitOutput, GroupMlsError>
where
    S: GroupMlsStore;

pub fn leave_prepare<S>(
    ctx: &GroupMlsContext<'_, S>,
    input: LeaveGroupInput,
) -> Result<PreparedMlsCommitOutput, GroupMlsError>
where
    S: GroupMlsStore;

pub fn update_member_prepare<S>(
    ctx: &GroupMlsContext<'_, S>,
    input: UpdateMemberInput,
) -> Result<PreparedMlsCommitOutput, GroupMlsError>
where
    S: GroupMlsStore;

pub fn recover_member_prepare<S>(
    ctx: &GroupMlsContext<'_, S>,
    input: RecoverMemberInput,
) -> Result<PreparedMlsCommitOutput, GroupMlsError>
where
    S: GroupMlsStore;

pub fn finalize_commit<S>(
    ctx: &GroupMlsContext<'_, S>,
    input: FinalizeCommitInput,
) -> Result<FinalizeCommitOutput, GroupMlsError>
where
    S: GroupMlsStore;

pub fn abort_commit<S>(
    ctx: &GroupMlsContext<'_, S>,
    input: AbortCommitInput,
) -> Result<AbortCommitOutput, GroupMlsError>
where
    S: GroupMlsStore;

pub fn process_welcome<S>(
    ctx: &GroupMlsContext<'_, S>,
    input: ProcessWelcomeInput,
) -> Result<ProcessWelcomeOutput, GroupMlsError>
where
    S: GroupMlsStore;

pub fn process_notice<S>(
    ctx: &GroupMlsContext<'_, S>,
    input: ProcessNoticeInput,
) -> Result<ProcessNoticeOutput, GroupMlsError>
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

### 8.2 compat command 到 typed operation 映射

`anp-mls/v1` compatibility 必须覆盖现有 command matrix：

| compat command | typed operation | epoch-changing | prepare/finalize/abort |
| --- | --- | ---: | ---: |
| `system version` | `system_version` | 否 | 否 |
| `key-package generate` | `generate_key_package` | 否 | 否 |
| `group create` | `create_group_prepare` + `finalize_commit` / `abort_commit` | 是 | 是 |
| `group add-member` | `add_member_prepare` + `finalize_commit` / `abort_commit` | 是 | 是 |
| `group remove-member` | `remove_member_prepare` + `finalize_commit` / `abort_commit` | 是 | 是 |
| `group leave` | `leave_prepare` + `finalize_commit` / `abort_commit` | 是 | 是 |
| `group update-member-prepare` | `update_member_prepare` | 是 | 是 |
| `group update-member-finalize` | `finalize_commit` | 是 | 是 |
| `group update-member-abort` | `abort_commit` | 是 | 是 |
| `group recover-member-prepare` | `recover_member_prepare` | 是 | 是 |
| `group recover-member-finalize` | `finalize_commit` | 是 | 是 |
| `group recover-member-abort` | `abort_commit` | 是 | 是 |
| `group commit-finalize` | `finalize_commit` | 是 | 是 |
| `group commit-abort` | `abort_commit` | 是 | 是 |
| `welcome process` | `process_welcome` | 是，本地加入/恢复 state | 需要事务 |
| `commit process` | `process_notice` / `process_commit` | 是 | 需要事务 |
| `notice process` | `process_notice` | 是 | 需要事务 |
| `message encrypt` | `encrypt` | 否，但依赖当前 epoch | 需要状态校验 |
| `message decrypt` | `decrypt` | 否，可能更新 replay/secret state | 需要状态校验 |
| `group restore` | `restore_or_status_repair` | 视实现 | 单独定义 |
| `group status` | `status` | 否 | 否 |

如果某个 legacy command 现在是 one-shot 且会推进 epoch，extraction 后不能直接保持 native typed API 的 one-shot 语义；只能在 compat layer 内模拟旧 JSON command，native provider 必须使用 prepare/finalize/abort。

### 8.3 command compatibility layer

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

[dependencies]
anp = { workspace = true, default-features = false }
```

`anp` workspace dependency 必须继续 `default-features = false`。`im-core` 只在 `group-e2ee` feature 下启用 `anp/mls`，避免默认把 `anp` 的 `network` 或其他非必要 feature 带入 SDK / Flutter packaging。

Required check：

```bash
cargo check -p im-core --no-default-features
cargo check -p im-core --features group-e2ee
cargo test -p im-core --features group-e2ee
cargo check -p im-core-dart
```

移动端/Flutter packaging 需要额外确认 `anp/mls` 拉入的 OpenMLS / rusqlite / bundled SQLite 依赖在 iOS、macOS、Android 目标上可构建。

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

`awiki-cli` 迁移规则：

```text
1. awiki-cli group_e2ee_* 不再长期直接 new MlsExecProvider。
2. awiki-cli normal command path 改为调用 im-core group E2EE send/status/repair runtime。
3. MlsExecProvider 保留为 compat fallback、legacy command wrapper 或 tests helper。
4. fallback 必须显式 feature/env gate，不作为默认路径。
5. 新增 contract tests 确认 CLI 默认路径不会绕过 im-core native provider。
```

Public API 不变：

```rust
client.messages().send(SendMessageRequest {
    target: MessageTarget::Group(group),
    security: MessageSecurityPolicy::E2eeRequired,
    ..
})

client.secure().group(group).status()
client.secure().group(group).repair()
client.secure().group(group).rotate_member_key()
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

### PR M0：storage / transaction / epoch spike

目标：

```text
先验证存储和一致性边界，不做大迁移。
```

验证项：

```text
1. anp library 能否打开 im-core local_state.sqlite。
2. openmls_sqlite_storage 是否能和 im-core 共享同一个 SQLite file。
3. 是否能共享同一个 connection / transaction。
4. OpenMLS migrations 是否影响 im-core PRAGMA user_version。
5. group add-member / group create 是否能改成 pending prepare，而不是 merge immediately。
6. crash after prepare / before finalize 的恢复行为。
```

输出：

```text
A. 可同库同事务。
B. 可同库不同 connection，但不能同事务。
C. 只能 internal sibling mls_state.sqlite。
```

### PR M1：anp extraction，不改变 binary 行为

目标：

```text
把 src/bin/anp-mls.rs 的 real OpenMLS operations 抽到 anp::group_e2ee::operations/storage/commands。
```

当前进展：

```text
../anp/rust local commit ce77ce5
  refactor: extract group mls command storage helpers

../anp/rust local commit ee77c58
  refactor: extract group mls real operations

已完成：
1. 新增 anp::group_e2ee::commands：
   - anp-mls/v1 API/version/command metadata；
   - ok/error JSON envelope helper；
   - operation-log response redaction helper。
2. 新增 anp::group_e2ee::storage：
   - StateLock；
   - JsonCodec；
   - SqliteMlsProvider；
   - sqlite_mls_provider；
   - init_app_schema。
3. src/bin/anp-mls.rs 复用 commands/storage helper，binary JSON 行为保持兼容。
4. 新增 anp::group_e2ee::operations：
   - real_key_package；
   - real_group_create；
   - real_group_add_member；
   - real_group_update_member_prepare；
   - real_group_recover_member_prepare；
   - real_group_remove_member；
   - real_group_leave；
   - real_welcome_process；
   - real_message_encrypt / real_message_decrypt；
   - real_group_commit_finalize / real_group_commit_abort；
   - real_commit_process；
   - real_group_status。
5. src/bin/anp-mls.rs 已降为约 750 行，只保留 CLI args、stdin/stdout JSON envelope、operation-id 记录、--data-dir compat wiring 和 contract-test mode。

仍未完成：
1. commands.rs 还不是完整 anp-mls/v1 command dispatcher。
2. typed operation input/output/error 尚未完成。
3. group create 已先在 JSON compatibility operation 中改成 prepare/finalize/abort 语义；后续仍要收敛成 typed API。
```

验证：

```bash
cd ../anp/rust
cargo check --features mls --bin anp-mls
cargo test --test group_e2ee_contract_tests --features mls
cargo test --test group_e2ee_storage_spike_tests --features mls
cargo test --test group_e2ee_real_mls_tests --features mls
```

完成标准：

```text
1. anp-mls binary 输出兼容现有 contract tests。
2. binary 只做 CLI/JSON envelope。
3. real_key_package / real_group_create / real_message_encrypt / real_message_decrypt 等逻辑不再只存在于 bin target。
4. library API 有 typed errors。
```

### PR M2：epoch-changing operations 统一 prepare/finalize/abort

目标：

```text
把所有会推进 MLS epoch 的 operations 统一到 prepare/finalize/abort。
```

范围：

```text
group create prepare/finalize/abort
group add-member prepare/finalize/abort
group remove-member prepare/finalize/abort
group leave prepare/finalize/abort
update-member prepare/finalize/abort
recover-member prepare/finalize/abort
generic commit-finalize / commit-abort typed API
```

完成标准：

```text
1. prepare 不更新 active binding epoch 为新 epoch。
2. finalize 只在 service accepted 后 merge pending commit 并更新 binding epoch。
3. abort / pending retry 不推进 local binding epoch。
4. 当前 add-member one-shot merge 行为不进入 NativeAnpMlsProvider。
5. crash after prepare / before finalize 可通过 pending_commits 恢复或显式 abort。
```

当前进展：

```text
../anp/rust local commit 54982b6
  fix: make group add member use pending commit

../anp/rust local commit 834bea6
  group create metadata prepare/finalize/abort implemented; pending create binding is not active until finalize.

../anp/rust local commit 55cfb31
  anp::group_e2ee::operations typed wrappers implemented for status, key package, create/add/remove/leave/update/recover prepare, finalize/abort, welcome/notice process, encrypt/decrypt.

已完成：
1. group add-member 不再 merge pending commit。
2. group add-member prepare 返回 pending_commit_id / commit / welcome / ratchet_tree。
3. group add-member prepare 不更新 binding epoch；group status local_epoch 保持旧 epoch。
4. group commit-finalize 对 add-member pending commit 执行 merge 并推进 epoch。
5. group commit-abort 对 add-member pending commit 清理 OpenMLS pending commit，不推进 local binding epoch。
6. group create prepare 写入 pending_create binding 和 pending_commits 记录，但 active binding 不可用。
7. group create finalize 验证 OpenMLS group state 仍存在，然后把 binding 激活。
8. group create abort 清理 pending binding 和对应 openmls group private state；随后可用新的 operation_id 重新 create。
9. typed operation entry points 已覆盖主要 command matrix；typed output 不暴露 provider path、SQLite path、OpenMLS StorageProvider、raw openmls_group_id 字段。

仍未完成：
1. crash after prepare / before finalize 的恢复/重试策略需要进一步收敛到 typed operations。
2. typed API 仍复用内部 JSON compatibility implementation；后续可逐步把内部实现改为原生 typed core。
```

### PR M3：storage abstraction

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

当前进展：

```text
../anp/rust local commit 8aeb96f
  feat: add im-core scoped group mls store

已完成：
1. 新增 GroupMlsStore trait。
2. 新增 CompatDataDirStore，兼容 --data-dir/state.db 的 state.lock、app schema、OpenMLS provider lifecycle。
3. typed operations 通过 GroupMlsStore 打开 one-shot operation scope；调用面不需要 anp-mls binary path。
4. 新增 ImCoreSqliteGroupMlsStore，可从 im-core local_state.sqlite path 推导 internal sibling scoped mls_state.sqlite。
5. ImCoreSqliteGroupMlsStore 创建 group_mls_operations / group_mls_agents / group_mls_key_packages / group_mls_bindings / group_mls_pending_commits。
6. typed operations 在进入 OpenMLS 写入前校验 owner_did/device_id 是否匹配 store owner scope。
7. ImCoreSqliteGroupMlsStore 不创建 legacy agents/key_packages/group_bindings/pending_commits 业务表；兼容 JSON core 通过 operation-scope TEMP view/trigger 写入 group_mls_* tables。

仍未完成：
1. group_mls_operations operation log 目前只定义 schema；native im-core path 的 operation log redaction/diagnostics 读取策略还需要在 provider 接入时完成。
2. im-core group secure send/decrypt/status/repair runtime 尚未接入 provider。
```

### PR M4：im-core provider trait + fake / exec / native skeleton

目标：

```text
im-core group-e2ee feature 下新增 GroupMlsProvider trait、FakeGroupMlsProvider、ExecAnpMlsProvider compat adapter 和 NativeAnpMlsProvider skeleton。
```

完成标准：

```text
1. provider path 不进入 public API。
2. fake provider 可覆盖 status/prepare/repair/send/decrypt tests。
3. ExecAnpMlsProvider 只作为 compat adapter。
4. NativeAnpMlsProvider skeleton 不接 public SDK route。
```

当前进展：

```text
awiki-cli-rs2 local commit d056020
  feat: add im-core group mls provider skeleton

已完成：
1. crates/im-core/Cargo.toml group-e2ee feature 改为 ["sqlite", "anp/mls"]。
2. 新增 internal/group_e2ee/provider.rs：
   - GroupMlsProvider trait；
   - typed operation matrix 覆盖 key package、create/add/remove/update/recover prepare、finalize/abort、welcome/notice、encrypt/decrypt/status。
3. 新增 NativeAnpMlsProvider：
   - 直接调用 anp::group_e2ee::operations typed one-shot functions；
   - 不 spawn anp-mls；
   - 不暴露 provider path / OpenMLS StorageProvider。
4. 新增 FakeGroupMlsProvider skeleton，供后续 im-core runtime tests 注入。
5. 新增 internal/group_e2ee/storage.rs：
   - 从 ImClient.current_identity().id/did/device_id 和 ImCorePaths.local_state.sqlite_path 构造 ImCoreSqliteGroupMlsStore；
   - device_id 为空时使用 anp 默认 device id；
   - sibling scoped mls_state.sqlite 仍是 internal detail。
6. 新增 module test，验证 NativeAnpMlsProvider 可通过 client identity scope 创建/finalize group 并读取 status。
7. group-e2ee 仍未接 public SDK route；MessageSecurityMode::GroupE2ee 仍保持 reserved/unsupported。

验证：
1. cargo check -p im-core --no-default-features
2. cargo check -p im-core --features group-e2ee
3. cargo test -p im-core --features group-e2ee
4. cargo check -p im-core-dart
5. cargo check --workspace --all-features

说明：
ExecAnpMlsProvider 未在 d056020 中实现；当前默认方向是 native provider。exec fallback 只有在 awiki-cli legacy migration 需要显式 compat feature 时再补，不能成为默认路径。
```

### PR M5：NativeAnpMlsProvider + storage adapter

目标：

```text
按 PR M0 结论接入 native provider storage。
```

选择：

```text
优先：same local_state.sqlite if proven safe。
否则：internal mls_state.sqlite sibling。
```

完成标准：

```text
1. im-core 直接调用 anp library。
2. 不 spawn anp-mls。
3. 不在 SDK public API 暴露 MLS storage path。
4. group_mls_* metadata owner_identity_id 隔离测试通过。
```

### PR M6：group send/decrypt/status/repair 接入 im-core

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
5. 不在 SQLite transaction 内执行网络 RPC。
6. service accepted 后 finalize，service rejected/network failed 后 abort 或 pending retry。
```

当前进展：

```text
awiki-cli-rs2 local commit 6bf9897
  feat: add internal group e2ee send runtime

已完成：
1. 新增 internal/group_e2ee/runtime.rs：
   - GroupE2eeTextSender；
   - GroupE2eeTextSend；
   - GroupE2eeTextSendResult。
2. 新增 internal/group_e2ee/wire.rs：
   - 构造 group.e2ee.send RPC params；
   - 复用现有 origin proof；
   - body 只包含 service delivery 需要的 cipher fields。
3. GroupE2eeTextSender send flow：
   - 校验 target 必须是 group；
   - 校验 MessageSecurityMode 必须是 GroupE2ee；
   - 确认 AuthScope::GroupMessaging session；
   - 构造 GroupApplicationPlaintext；
   - 调用 GroupMlsProvider::encrypt；
   - 调用 message service group.e2ee.send；
   - 复用普通 group send 的 SDK result mapping。
4. group.e2ee.send body 当前只写入：
   - crypto_group_id_b64u；
   - epoch；
   - private_message_b64u；
   - group_state_ref；
   - epoch_authenticator。
5. 新增 unit test 验证：
   - encrypt 先于 RPC send；
   - RPC method 是 group.e2ee.send；
   - request body 不包含 plaintext / application_plaintext；
   - origin proof 存在；
   - SDK result 可从 group service response 映射。

边界：
1. 这是 im-core internal runtime slice，不接 public SDK route。
2. MessageSecurityMode::GroupE2ee 对 public messages().send 仍保持 reserved/unsupported。
3. caller 仍必须提供 group_state_ref；在 group_state_ref lookup/repair 稳定前，不能把它作为默认 public send path。
4. encrypt 不推进 MLS epoch，因此这一段不需要 prepare/finalize/abort。
5. create/add/remove/update/recover/leave 后续接入时仍必须遵守 prepare -> service RPC -> finalize/abort。

验证：
1. cargo fmt --check
2. cargo check -p im-core --features group-e2ee
3. cargo test -p im-core --features group-e2ee group_e2ee_text_sender_encrypts_then_sends_cipher_without_plaintext
```

### PR M7：awiki-cli 旧路径迁移

目标：

```text
awiki-cli group_e2ee_* 不再默认直接 new MlsExecProvider。
```

完成标准：

```text
1. awiki-cli group send/status/repair/decrypt 默认调用 im-core group E2EE runtime。
2. MlsExecProvider 只作为 fallback feature / test helper / legacy wrapper。
3. CLI contract tests 覆盖默认路径和 fallback 路径。
4. doctor/report 更新，不再要求普通 SDK 用户理解 anp-mls binary path。
```

### PR M8：legacy data import / cleanup

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
client.secure().group(group).repair()
client.secure().group(group).rotate_member_key()
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
