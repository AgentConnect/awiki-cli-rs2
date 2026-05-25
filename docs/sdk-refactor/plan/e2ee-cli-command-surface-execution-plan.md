# E2EE CLI 高层命令面设计与执行计划

**来源文档**：`docs/sdk-refactor/plan/cli-shell-final-cutover-execution-plan2.md`
**适用范围**：整理 E2EE 相关 CLI 命令面、CLI 到 `im-core` 的执行边界、以及 E2EE 命令所依赖的最小 runtime/listener cutover 验收；不覆盖 mail、people、page/site、runtime provider 形态等非 E2EE 产品迁移。
**核心目标**：`awiki-cli` 只表达用户意图、解析参数、处理 workspace/config/dry-run/输出；所有受支持的 E2EE 产品能力都必须通过 `im-core` 高层 public API 执行。

---

## 1. 范围定义与设计原则

### 1.1 “默认打开”的定义

本计划中的“默认打开 E2EE”只表示 supported E2EE 产品能力在默认发布构建和默认 CLI surface 中可用，不表示所有消息默认加密，也不表示跳过 capability / identity / server-side gate。

落地定义：

```text
1. 默认发布构建中，awiki-cli 依赖 im-core 时必须启用 supported E2EE 所需 feature；当前尤其包括 group-e2ee。
2. 默认 schema/help/completion 展示高层 E2EE 产品命令和 canonical `--secure required`。
3. `msg send` 未指定 `--secure` 时仍保持当前默认语义；只有用户显式传 `--secure required` 才要求 E2EE。
4. direct/group E2EE 可用性仍由 im-core 根据 identity readiness、local state、server capability、transport 和 runtime 状态判断。
5. 如果 im-core 或服务端能力未就绪，必须 fail closed，返回 stable unsupported/unavailable/identity_not_ready，不 fallback 到旧 CLI 业务实现。
6. 本阶段编译和 release 验收范围先限定为 Linux 与 macOS；Windows E2EE package/release 可暂缓，不阻塞 Linux/macOS 开发推进。
```

### 1.2 依赖边界

最终 E2EE CLI 不是底层 MLS / secure outbox / KeyPackage / provider 调试工具，而是面向用户的高层产品接口。

强约束：

```text
1. supported E2EE app handler / im_core_adapter 不得调用旧 awiki-cli::message::* E2EE 业务实现。
2. supported E2EE app handler / im_core_adapter 不得直接调用旧 secure_* / group_e2ee_* CLI 实现。
3. supported E2EE CLI 不暴露 raw KeyPackage、prekey payload、MLS notice、provider binary、wire RPC method/params、ratchet counter、session counter、raw outbox row。
4. direct/group E2EE send、secure session、prekey、secure outbox、incoming decrypt、local ACK、group MLS state、MLS notice processing 都由 im-core 负责。
5. 如果某项能力还没有 im-core 高层 public API，先补 im-core API，再开放 CLI；不能为了开放 CLI 恢复旧 awiki-cli 业务路径。
6. 本版本不新增 supported E2EE diagnostic CLI；高层诊断和恢复由 status / repair 覆盖。
7. awiki-cli 可以继续负责 parser、workspace/config、identity selector、dry-run、错误码、输出渲染、schema/help/completion 这些 CLI shell 职责。
```

目标依赖方向：

```text
awiki-cli command
  -> app handler / im_core_adapter
  -> im-core public service
  -> im-core internal runtime/store/transport/secure implementation
```

禁止的默认路径：

```text
awiki-cli command
  -> crate::message::secure_*
  -> crate::message::group_e2ee_*
  -> crate::message::group_e2ee_provider::MlsExecProvider
  -> AWIKI_ANP_MLS_BINARY as default runtime
  -> anpsdk::DirectE2eeSession / FileSessionStore
```

---

## 2. 最终保留的高层接口

这些是未来默认 schema/help/completion 中应展示的 E2EE 产品命令。

### 2.1 Direct E2EE 发送

最终接口：

```bash
awiki-cli msg send --to <peer> --text <text> --secure required
```

执行目标：

```rust
client.messages().send(SendMessageRequest {
    security: MessageSecurityMode::E2eeRequired,
    ..
})
```

要求：

```text
1. CLI 不在 app 层把 E2eeRequired 统一拦成 Phase 6 / cutover unsupported。
2. direct secure send 是否可用由 im-core 判断。
3. CLI 可以做参数级校验，例如 secure-direct alias 只能用于 direct target。
4. secure attachment 如果 im-core 尚不支持，必须 fail closed，返回 stable unsupported，不 fallback 到旧 message/attachment 路径。
```

### 2.2 Direct E2EE 状态与修复

最终接口：

```bash
awiki-cli msg secure status --with <peer>
awiki-cli msg secure repair --with <peer>
```

执行目标：

```rust
client.secure().direct(peer).status()
client.secure().direct(peer).repair()
```

要求：

```text
1. status 输出 DirectSecureStatus 这类高层状态字段。
2. repair 输出高层修复结果或修复计划。
3. 输出中不得包含 raw prekey、session id、ratchet counter、ciphertext、DB path。
4. dry-run 只展示将执行的高层 im-core action 和目标 peer。
```

说明：`msg secure init` 不保留为最终 CLI。当前代码中如已有 `msg.secure.init`，本计划要求从 default schema/help/completion 和 dispatch 中移除，或保留为 stable unsupported/removed 兼容入口；不得作为 supported/advanced/deprecated alias 暴露。`client.secure().direct(peer).prepare()` 只允许作为 im-core 内部或后续单独计划的能力，不进入本版本 E2EE CLI surface。

### 2.3 Group E2EE 发送

最终接口：

```bash
awiki-cli msg send --group <group_did> --text <text> --secure required
```

执行目标：

```rust
client.messages().send(SendMessageRequest {
    security: MessageSecurityMode::E2eeRequired,
    ..
})
```

要求：

```text
1. group secure send 走 im-core group E2EE 实现。
2. awiki-cli 必须启用 im-core 支持 group E2EE 所需 feature，或等价的 supported feature set。
3. group secure send 不依赖 CLI exec provider / AWIKI_ANP_MLS_BINARY。
4. CLI 不拼 MLS payload，不处理 KeyPackage，不暴露 group E2EE wire params。
```

### 2.4 Group E2EE 生命周期

最终接口：

```bash
awiki-cli group create --name <name> --secure required
awiki-cli group add --group <group_did> --member <member> --secure required
awiki-cli group remove --group <group_did> --member <member> --secure required
awiki-cli group leave --group <group_did> --secure required
```

执行目标：

```text
client.groups().create(...)
client.groups().add_member(...)
client.groups().remove_member(...)
client.groups().leave(...)
```

这些 im-core group API 必须是 secure-aware 的高层 API。CLI 只传达 “secure required” 这个用户意图，具体的 group MLS 编排、notice、member state、owner state、local projection 都由 im-core 完成。

要求：

```text
1. 如果 im-core 当前没有 secure-aware group lifecycle API，先补 im-core。
2. 不允许 CLI 直接恢复 awiki-cli::message::group_e2ee_create/add/remove/leave/recover/update 作为执行路径。
3. add/remove/leave 的 E2EE 成员状态变化由 im-core 处理。
4. CLI 输出只展示 group lifecycle 的高层结果，不输出 MLS artifact。
5. `--secure required` 是新 canonical flag；旧 `--e2ee` / `--message-security-profile group-e2ee` 只能作为 deprecated alias。
```

Group lifecycle 是本计划的阻塞前置项，而不是普通 CLI 改名任务。当前 public API 如果只能表达 group create 的 `message_security_profile` / `e2ee`，但不能表达 add/remove/leave 的 secure requirement，则必须先扩展 im-core DTO 或 service 方法，再接 CLI。

### 2.5 Group E2EE 状态与修复

最终接口：

```bash
awiki-cli group secure status --group <group_did>
awiki-cli group secure repair --group <group_did>
```

执行目标：

```rust
client.secure().group(group).status()
client.secure().group(group).repair()
```

要求：

```text
1. status 输出 GroupSecureStatus 这类高层状态。
2. repair 覆盖 pending notices、local MLS state repair、service head comparison 等恢复语义。
3. CLI 不输出 raw MLS state、KeyPackage、notice body、provider stdout/stderr。
4. `group secure status` / `group secure repair` 是本版本 group E2EE 的高层诊断和恢复入口。
```

---

## 3. 需要修改的接口

这些接口不一定立刻删除，但必须改成 canonical 命令的 alias、warning 或 stable unsupported。

### 3.1 `msg send --secure on`

当前/旧接口：

```bash
awiki-cli msg send --to <peer> --text <text> --secure on
awiki-cli msg send --group <group_did> --text <text> --secure on
```

目标：

```text
1. deprecated alias 到 `--secure required`。
2. 仍可执行，但输出 deprecation warning。
3. default schema/help/completion 不展示 `on`。
4. schema --all 可以展示，并标记 deprecated=true、canonical=`--secure required`。
```

### 3.2 其他 `msg send --secure` 旧值

旧接口：

```text
--secure e2ee
--secure secure-direct
--secure group-e2ee
```

目标：

```text
1. 全部收敛到 `--secure required`。
2. `secure-direct` 只能用于 direct target；用于 group 时在 RPC 前返回 invalid_argument 或 unsupported_mode。
3. `group-e2ee` 只能用于 group target；用于 direct 时在 RPC 前返回 invalid_argument 或 unsupported_mode。
4. 都输出 deprecation warning。
5. 不进入 default completion。
```

### 3.3 `group e2ee status/repair`

旧接口：

```bash
awiki-cli group e2ee status --group <group_did>
awiki-cli group e2ee repair --group <group_did>
```

目标：

```bash
awiki-cli group secure status --group <group_did>
awiki-cli group secure repair --group <group_did>
```

行为：

```text
1. `group e2ee status` 作为 deprecated alias 转发到 `group secure status`。
2. `group e2ee repair` 作为 deprecated alias 转发到 `group secure repair`。
3. 旧命令不进入 default schema/help/completion。
4. 转发后仍执行目标命令的 policy gate 和 im-core capability check。
```

### 3.4 Group 创建和成员变更的旧 E2EE flags

旧接口：

```bash
awiki-cli group create --e2ee
awiki-cli group create --message-security-profile group-e2ee
awiki-cli group add --group <group_did> --member <member> --e2ee
awiki-cli group remove --group <group_did> --member <member> --e2ee
awiki-cli group leave --group <group_did> --e2ee
```

目标：

```bash
awiki-cli group create --secure required
awiki-cli group add --group <group_did> --member <member> --secure required
awiki-cli group remove --group <group_did> --member <member> --secure required
awiki-cli group leave --group <group_did> --secure required
```

行为：

```text
1. 旧 flags 作为 deprecated alias 保留一段时间。
2. 输出 deprecation warning。
3. default schema/help/completion 只展示 `--secure required`。
4. alias 不得绕过 im-core secure-aware lifecycle API。
```

### 3.5 `msg secure failed/retry/drop`

旧接口：

```bash
awiki-cli msg secure failed
awiki-cli msg secure retry
awiki-cli msg secure drop
```

目标：

```text
1. 不作为本版本 supported diagnostic CLI。
2. 从 default schema/help/completion 移除。
3. 直接调用时返回 stable unsupported，或要求 InternalService/TestOnly gate。
4. 不转发到 supported outbox 命令，因为本版本没有 supported outbox diagnostic surface。
```

原因：

```text
secure outbox retry/flush/drop 属于 im-core runtime 和 repair flow 内部职责。
用户级恢复入口是 `msg secure repair`。
```

---

## 4. 需要删除、隐藏或内部化的接口

这些命令不是未来默认 E2EE CLI，不应作为 supported contract 暴露。

### 4.1 Direct secure outbox 明细命令

```bash
awiki-cli msg secure outbox list
awiki-cli msg secure outbox retry
awiki-cli msg secure outbox drop
```

处理策略：

```text
1. 不进入 default schema/help/completion。
2. 直接调用返回 stable unsupported，或需要 InternalService/TestOnly gate。
3. 不要求 im-core 暴露同名 public API。
4. 如果未来需要 support-grade diagnostics，另起 diagnostics plan，并使用抽象 DTO，例如 SecureDiagnosticReport / SecureProblem / SecureOperationId。
```

### 4.2 Group secure diagnostics

```bash
awiki-cli group secure diagnostics --group <group_did>
awiki-cli group secure repair --group <group_did> --explain
```

处理策略：

```text
1. 本版本 unsupported。
2. 用 `group secure status` / `group secure repair` 覆盖高层诊断和恢复。
3. 不因为 diagnostics 需求暴露 MLS notice body、provider binary path、raw DB row。
```

### 4.3 低层 `group e2ee *` 命令

```bash
awiki-cli group e2ee publish-key-package --group <group_did>
awiki-cli group e2ee pending --group <group_did>
awiki-cli group e2ee process-leave-request --group <group_did>
awiki-cli group e2ee recover-member --group <group_did> --member <member>
awiki-cli group e2ee update-key --group <group_did> --member <member>
awiki-cli group e2ee rejoin --group <group_did> --member <member>
```

处理策略：

```text
1. Hidden / Internal / TestOnly，不进入 default surface。
2. 没有 internal/test-only gate 时，直接调用返回 stable unsupported。
3. 不承诺 supported diagnostic contract。
4. 不要求 im-core 提供这些同名 public functions。
5. 不允许这些旧命令反向驱动 im-core public API 变成 wire/store/MLS helper 集合。
```

高层替代方向：

```text
publish-key-package -> 不提供 supported CLI；由 im-core runtime 管理
pending -> group secure status
process-leave-request -> group remove --secure required，由 im-core 编排
recover-member -> group secure repair
update-key -> group secure repair
rejoin -> group add --secure required，由 im-core 编排
```

---

## 5. im-core 必须承接的能力

执行 CLI 命令面优化前，需要确认或补齐这些 im-core 能力。

### 5.1 Direct E2EE

```text
1. client.messages().send(... E2eeRequired ...) 支持 direct secure send。
2. client.secure().direct(peer).status() 可返回高层 direct secure 状态。
3. client.secure().direct(peer).repair() 可完成或计划 direct secure 修复。
4. secure prekey retry、secure outbox flush、incoming decrypt、local ACK 进入 im-core runtime。
```

### 5.2 Group E2EE

```text
1. awiki-cli 对 im-core 启用 group E2EE 所需 feature，或 im-core default feature set 已支持。
2. client.messages().send(... E2eeRequired ...) 支持 group secure send。
3. client.secure().group(group).status() 返回高层 group secure 状态。
4. client.secure().group(group).repair() 覆盖高层 group secure 修复。
5. group MLS provider 是 im-core native provider，不依赖 CLI exec provider。
6. incoming group decrypt、MLS notice processing、secure realtime projection 进入 im-core。
```

### 5.3 Group secure lifecycle

需要 im-core 暴露 secure-aware group lifecycle API。形态可以是：

```rust
client.groups().create(GroupCreateRequest {
    secure: GroupSecurityRequirement::Required,
    ..
})
```

也可以是：

```rust
client.groups().create(GroupCreateRequest {
    message_security_profile: Some(GroupMessageSecurityProfile::GroupE2ee),
    ..
})
```

但无论 DTO 形态如何，service implementation 必须由 im-core 完成完整 secure 编排，CLI 不得直接调用旧 group_e2ee builders。

最低能力要求：

```text
1. create secure group：
   - 提交普通 group.create 高层请求。
   - 初始化本地 MLS group state。
   - 提交服务端 secure head / group E2EE 初始化所需请求。
   - 本地 projection 只保存高层 group secure summary，不保存 raw MLS artifact。
2. add secure member：
   - im-core 负责获取/租约成员 KeyPackage、生成 commit/welcome、提交成员变更和 notice。
   - CLI 只传 group、member、role、secure required。
3. remove secure member：
   - im-core 负责成员移除、commit、service head 更新、notice 和 local projection。
4. leave secure group：
   - im-core 负责 leave request / owner processing / local state cleanup 的高层语义。
   - 如果当前服务端协议仍需要 owner/admin 处理，应由 repair/status 暴露高层 pending work，不暴露 process-leave-request CLI。
5. 所有 lifecycle API 必须有幂等键或 operation id 策略；CLI 不生成 MLS-level request id。
6. 失败语义必须稳定：remote mutation 成功但 local MLS/projection 失败时，返回 high-level warning/problem，并可由 `group secure repair` 收敛。
```

### 5.4 CLI shell 和 metadata 必须承接的能力

CLI 改造不能只改 handler；必须同时更新 parser、metadata、schema、completion 和 policy gate。

```text
1. `cmdmeta` 增加或更新：
   - `msg send --secure required` choices。
   - `group create/add/remove/leave --secure required` flag。
   - `group secure status/repair` parent/spec。
   - deprecated alias metadata：old surface、canonical command、warning text。
   - internal/test-only/unsupported policy metadata。
2. dispatcher 增加：
   - `group.secure.status`
   - `group.secure.repair`
   - 如果保留 `group.e2ee.status/repair`，它们只作为 alias dispatch 到 canonical handler。
3. handler 行为：
   - dry-run 也必须经过 policy gate。
   - deprecated alias 允许执行但必须输出 warning。
   - hidden/internal/test-only command 直接调用时必须返回 stable unsupported，除非显式 internal/test-only gate 打开。
4. parser 行为：
   - `required` 是 canonical secure mode。
   - `on/e2ee/secure-direct/group-e2ee` 解析为 deprecated alias，并做 direct/group target validation。
   - invalid target-specific alias 在 RPC 前返回 invalid_argument 或 unsupported_mode。
5. default schema/help/completion 只展示 supported 高层命令；`schema --all` 展示 deprecated/internal/unsupported 条目和 policy。
```

### 5.5 Runtime/listener 最小验收

本计划不重做 runtime provider 架构，但 E2EE 默认可用需要 listener/runtime 的 E2EE 行为不再依赖旧 CLI 业务路径。

最低验收：

```text
1. direct incoming decrypt、secure init/ack、local ACK、secure prekey retry、secure outbox flush 由 im-core runtime 执行。
2. group incoming decrypt、MLS notice processing、secure realtime projection 由 im-core runtime 执行。
3. runtime/listener handler 不调用 awiki-cli legacy secure_direct / group_e2ee implementation 作为 default path。
4. 如果某项 runtime 能力尚未 cutover，本计划必须将对应 CLI surface 标为 unsupported 或 partial，不能声明 full default-on。
```

### 5.6 Public API 暴露边界

`im-core` 可以保留抽象 diagnostic DTO，但 supported CLI 不暴露低层调试命令。

```text
1. 如果保留 `client.secure().outbox()` public API，它只能返回 redacted/high-level DTO，不得包含 plaintext、raw row、DB path、session id 或 ciphertext。
2. CLI 本版本不支持 outbox list/retry/drop；未来如需支持，必须另起 diagnostics plan。
3. im-core public API 不应为了兼容旧 CLI 命令而暴露 KeyPackage、MLS notice body、provider binary path 或 raw RPC params。
```

---

## 6. 命令策略表

| 接口 | 最终状态 | 默认展示 | 执行 owner | 备注 |
| --- | --- | --- | --- | --- |
| `msg send --secure required` | Allow | 是 | ImCoreMessages + ImCoreSecure | direct/group 都走 `client.messages().send` |
| `msg send --secure on` | DeprecatedAlias | 否 | ImCoreMessages + ImCoreSecure | alias 到 `--secure required` |
| `msg send --secure e2ee` | DeprecatedAlias | 否 | ImCoreMessages + ImCoreSecure | alias 到 `--secure required` |
| `msg send --secure secure-direct` | DeprecatedAlias | 否 | ImCoreMessages + ImCoreSecure | direct only |
| `msg send --secure group-e2ee` | DeprecatedAlias | 否 | ImCoreMessages + ImCoreSecure | group only |
| `msg secure status` | Allow | 是 | ImCoreSecure | direct status |
| `msg secure repair` | AllowWithWarning | 是 | ImCoreSecure | direct repair |
| `msg secure init` | Removed 或 StableUnsupported | 否 | ImCoreSecure | 不保留；不映射为 supported prepare |
| `msg secure failed/retry/drop` | StableUnsupported 或 Internal/TestOnly | 否 | ImCoreSecure | 不作为 supported diagnostic |
| `msg secure outbox *` | StableUnsupported 或 Internal/TestOnly | 否 | ImCoreSecure | 不暴露 outbox row |
| `group create --secure required` | AllowWithWarning | 是 | ImCoreGroups + ImCoreSecure | secure-aware lifecycle |
| `group add/remove/leave --secure required` | AllowWithWarning | 是 | ImCoreGroups + ImCoreSecure | secure-aware lifecycle |
| `group create --e2ee` | DeprecatedAlias | 否 | ImCoreGroups + ImCoreSecure | alias 到 `--secure required` |
| `group create --message-security-profile group-e2ee` | DeprecatedAlias | 否 | ImCoreGroups + ImCoreSecure | alias 到 `--secure required` |
| `group add/remove/leave --e2ee` | DeprecatedAlias | 否 | ImCoreGroups + ImCoreSecure | alias 到 `--secure required` |
| `group secure status` | Allow | 是 | ImCoreSecure | group status |
| `group secure repair` | AllowWithWarning | 是 | ImCoreSecure | group repair |
| `group secure diagnostics` | StableUnsupported | 否 | ImCoreSecure | 本版本不支持 |
| `group secure repair --explain` | StableUnsupported | 否 | ImCoreSecure | 本版本不支持 |
| `group e2ee status/repair` | DeprecatedAlias | 否 | ImCoreSecure | alias 到 `group secure *` |
| `group e2ee publish-key-package` | Internal/TestOnly 或 StableUnsupported | 否 | ImCoreSecure | 低层 MLS 命令 |
| `group e2ee pending/process-leave-request/recover-member/update-key/rejoin` | Internal/TestOnly 或 StableUnsupported | 否 | ImCoreSecure | 低层 MLS 命令 |

---

## 7. 实施顺序

### T0. 明确构建 feature 与能力 gate

修改点：

```text
1. awiki-cli 默认发布构建启用 im-core supported E2EE feature，至少包括 group-e2ee。
2. 本阶段 E2EE feature/release 验收先限定 Linux 与 macOS。
3. Windows package/release 暂缓；不得因为 Windows 暂未验证而阻塞 Linux/macOS E2EE CLI 打开。
4. 确认 group-e2ee 引入的 anp/mls 依赖、包体积、Linux/macOS release artifact 和 npm install 流程可接受。
5. 对 Windows 保留 stable unsupported 或 build/package disabled 说明，避免用户误以为 Windows E2EE 已支持。
6. 保留 runtime/server capability gate：feature 打开不等于服务端能力一定可用。
7. unsupported/unavailable/identity_not_ready 错误码稳定，且不触发 legacy fallback。
```

验收：

```text
1. `cargo tree -p awiki-cli -e features` 可看到 im-core group-e2ee 相关 feature。
2. Linux/macOS 上 awiki-cli 默认构建能启用 group-e2ee feature 并通过 E2EE CLI contract tests。
3. group E2EE disabled/unavailable 场景返回 stable error，而不是编译期缺功能、generic internal error 或旧 CLI fallback。
4. Windows 不作为本阶段 E2EE release blocker；如果构建矩阵仍包含 Windows，必须显式跳过 E2EE package/release 验收或返回稳定 unsupported。
```

### T1. 建立命令 metadata / policy enforcement

为 E2EE 命令补齐或更新 metadata：

```text
CommandAudience
CommandOwner
CliShellRole
DirectInvocationPolicy
canonical_name
deprecated alias metadata
```

行为要求：

```text
1. default schema/help/completion 只展示高层 E2EE 产品命令。
2. schema --all 可以展示 deprecated/internal/unsupported 条目和 policy。
3. completion 默认不提示 low-level group e2ee commands。
4. dispatch 在 handler 执行前统一 enforce policy，dry-run 也不绕过 policy。
5. deprecated alias 不得绕过目标命令 gate。
6. `msg.secure.status/repair` 从 cutover unsupported 改为 ImCore supported。
7. `group.secure.status/repair` 新增 canonical specs 和 dispatch。
8. `group.e2ee.status/repair` 明确为 DeprecatedAlias，执行时 rewrite 到 `group.secure.status/repair` 并输出 warning。
9. `msg.secure.init` 从 default surface 和 dispatch 中移除；如保留兼容入口，标记为 Removed 或 StableUnsupported，不执行 prepare。
10. `msg.secure.failed/retry/drop`、低层 `group.e2ee.*`、`group secure diagnostics` 标记为 internal/test-only 或 stable unsupported。
```

当前 CLI policy 对 DeprecatedAlias 的处理不能只停留在 metadata：必须支持 alias rewrite、warning 注入和 canonical handler dispatch。`DeprecatedAlias` 直接返回 removed/unsupported 不满足本计划。

### T2. 打开 `msg send --secure required`

修改点：

```text
1. 移除 app 层对 E2eeRequired 的 blanket cutover unsupported gate。
2. `--secure required` 映射到 MessageSecurityMode::E2eeRequired。
3. `--secure on/e2ee/secure-direct/group-e2ee` 作为 deprecated alias。
4. secure attachments 继续 fail closed，直到 im-core 支持。
5. schema/help/completion 默认只展示 `required` / `off`，旧值只在 schema --all 或 deprecated metadata 中出现。
```

验收：

```text
1. direct `msg send --secure required` 进入 client.messages().send()。
2. group `msg send --secure required` 进入 client.messages().send()。
3. unsupported secure attachment 返回 stable unsupported。
4. target-specific alias 在 RPC 前完成 target validation。
5. `--secure required` 被 parser 接受；`--secure on` 输出 deprecation warning。
```

### T3. 接入 direct `msg secure status/repair`

修改点：

```text
msg secure status -> client.secure().direct(peer).status()
msg secure repair -> client.secure().direct(peer).repair()
```

同时处理：

```text
msg secure failed/retry/drop -> stable unsupported 或 Internal/TestOnly
msg secure outbox * -> stable unsupported 或 Internal/TestOnly
```

### T4. 补齐 group secure lifecycle

Group lifecycle 是打开 group secure send 的阻塞前置项。必须先补 im-core secure-aware lifecycle，再开放默认 group E2EE 发送和 status/repair surface。

修改点：

```text
im-core:
1. 为 group create/add/remove/leave 增加 secure-aware public request/API。
2. 实现完整 secure lifecycle 编排、local projection、repair convergence 和 high-level warnings/problems。

CLI:
group create --secure required
group add --secure required
group remove --secure required
group leave --secure required
```

如果 im-core 当前没有 secure-aware lifecycle API，本任务先补 im-core，再接 CLI。不得先打开 group secure send 后让用户无法创建或维护 secure group。

验收：

```text
1. group create --secure required 使用 im-core secure-aware create。
2. group add/remove/leave --secure required 使用 im-core secure-aware lifecycle。
3. group create/add/remove/leave --e2ee 是 deprecated alias，不能直接调用旧 group_e2ee builders。
4. remote mutation 成功但 local MLS/projection 失败时，返回 high-level warning/problem，并可由 group secure repair 收敛。
5. 当前 cached E2EE group 的 fail-closed gate 不再阻断 supported secure-aware lifecycle path。
```

### T5. 打开 group secure send

修改点：

```text
1. awiki-cli 启用 im-core group E2EE 所需 feature。
2. group `msg send --secure required` 使用 im-core group E2EE send。
3. 移除 CLI 对 group E2EE send 的 cutover unsupported gate。
4. 不恢复 MlsExecProvider / AWIKI_ANP_MLS_BINARY 默认执行路径。
5. im-core 返回 `unsupported("group-e2ee")` 时，CLI 映射为 stable unsupported/unavailable envelope，不落到 generic internal_error。
```

### T6. 新增 group secure status/repair

修改点：

```text
group secure status -> client.secure().group(group).status()
group secure repair -> client.secure().group(group).repair()
```

alias：

```text
group e2ee status -> group secure status
group e2ee repair -> group secure repair
```

### T7. 隐藏或删除低层 E2EE 命令

处理：

```text
1. `group e2ee publish-key-package/pending/process-leave-request/recover-member/update-key/rejoin`
   从 default schema/help/completion 移除。
2. `group secure diagnostics` / `group secure repair --explain` 返回 stable unsupported。
3. 旧 outbox/detail 命令返回 stable unsupported 或需要 InternalService/TestOnly gate。
4. 删除或重写依赖这些命令作为 supported contract 的测试。
```

### T8. Runtime/listener E2EE cutover 验收

处理：

```text
1. direct incoming decrypt、secure ACK、prekey retry、outbox flush 默认走 im-core runtime。
2. group incoming decrypt、MLS notice processing、secure realtime projection 默认走 im-core runtime。
3. 如果 listener 仍调用旧 awiki-cli secure/group_e2ee 业务代码，对应能力不能标记 full supported。
4. 更新 runtime/listener 相关测试，证明 default path 不依赖 legacy E2EE implementation。
```

### T9. 文档、skills 与 release surface 同步

处理：

```text
1. 更新 docs/architecture/awiki-command-v2.md 的命令树和 secure flag choices。
2. 更新 docs/architecture/group-e2ee-operations.md，标明旧低层命令变为 internal/test-only/unsupported。
3. 更新 skills/ 中面向 agent 的 E2EE 命令说明。
4. 更新 release notes/package 检查，说明 group-e2ee feature、anp/mls 依赖和 blocked diagnostics。
```

---

## 8. 测试迁移

### 8.1 需要新增或保留的测试

```text
1. msg send --secure required direct path reaches im-core。
2. msg send --secure required group path reaches im-core。
3. msg send --secure on/e2ee/... emits deprecation warning and forwards to canonical mode。
4. msg secure status/repair uses client.secure().direct()。
5. group secure status/repair uses client.secure().group()。
6. group e2ee status/repair aliases to group secure status/repair。
7. group create/add/remove/leave --secure required uses im-core secure-aware group lifecycle。
8. secure attachment remains fail-closed until im-core supports it。
9. low-level group e2ee commands are hidden/internal/unsupported。
10. default schema/help/completion only shows high-level E2EE product commands。
11. dry-run cannot bypass hidden/internal/unsupported policy。
12. runtime/listener E2EE default path uses im-core where claimed supported。
13. Linux/macOS package/release build includes required im-core E2EE features。
14. Windows E2EE package/release is explicitly skipped, disabled, or documented as non-blocking for this stage。
```

### 8.2 需要删除或重写的旧测试

删除或重写断言以下行为的测试：

```text
msg send --secure -> cutover unsupported
group secure send -> cutover unsupported
group create/add/remove/leave --e2ee -> cutover unsupported
group e2ee status/repair -> live unsupported
AWIKI_ANP_MLS_BINARY execution shape
MlsExecProvider args/stdin shape
direct calls to awiki-cli group_e2ee_* builders
CLI-built raw group.e2ee.* RPC params
raw KeyPackage / MLS notice / provider stdout/stderr in CLI output
```

如果底层行为仍重要，应迁到 `im-core` 测试，用 native provider / test transport / high-level DTO 断言。

建议测试文件：

```text
msg_secure_send_im_cli_shell_core_contract.rs
msg_secure_command_surface_contract.rs
group_secure_send_im_cli_shell_core_contract.rs
group_secure_command_surface_contract.rs
group_secure_lifecycle_im_cli_shell_core_contract.rs
e2ee_deprecated_alias_contract.rs
e2ee_hidden_internal_command_contract.rs
```

---

## 9. 验证命令

实现后运行：

```bash
cargo +1.85.1 fmt --all -- --check
cargo +1.85.1 test -p im-core --locked secure
cargo +1.85.1 test -p im-core --locked realtime
cargo +1.85.1 test -p im-core --features group-e2ee --locked
cargo +1.85.1 test -p awiki-cli --locked msg_secure
cargo +1.85.1 test -p awiki-cli --locked group_secure
cargo +1.85.1 test -p awiki-cli --locked e2ee
cargo +1.85.1 test --workspace --locked
AWIKI_CLI_RUST_TOOLCHAIN=1.85.1 ./scripts/test-unit.sh
```

说明：仓库 workspace 当前声明 `rust-version = 1.78`，但 final cutover 当前验证口径使用 `cargo +1.85.1 ... --locked`。如果 release pipeline 需要继续保留 1.78/1.79 MSRV 检查，应作为额外兼容性检查保留；不要把 MSRV 检查和本阶段 E2EE feature/release 验收混成一个阻塞条件。

静态检查：

```bash
rg 'new_secure_e2ee_client_for_record|MessageServiceE2EEClient|flush_queued_secure_outbox|maybe_publish_secure_prekeys|DirectE2eeSession|FileSessionStore|AWIKI_ANP_MLS_BINARY' crates/awiki-cli/src/app crates/awiki-cli/src/im_core_adapter
rg 'crate::message::secure_|crate::message::group_e2ee_|MlsExecProvider|AWIKI_ANP_MLS_BINARY' crates/awiki-cli/src/runtime
rg 'group_e2ee_create|group_e2ee_add|group_e2ee_remove|group_e2ee_recover|group_e2ee_update|MlsExecProvider' crates/awiki-cli/src/app crates/awiki-cli/src/im_core_adapter
rg 'unsupported_cutover_command\("msg.send"|unsupported_group_e2ee_command' crates/awiki-cli/src/app crates/awiki-cli/src/im_core_adapter
rg 'group\.secure|secure required|CommandAudience|DirectInvocationPolicy|canonical' crates/awiki-cli/src/cmdmeta crates/awiki-cli/src/cli crates/awiki-cli/src/app
cargo tree -p awiki-cli -e features | rg 'im-core|group-e2ee|anp/mls'
```

期望：

```text
1. supported app handler / im_core_adapter 不调用旧 message secure/group_e2ee implementation。
2. 不再存在 msg send blanket E2eeRequired cutover gate。
3. unsupported_group_e2ee_command 只用于 low-level hidden/internal commands，不用于默认 secure 产品命令。
4. AWIKI_ANP_MLS_BINARY 不出现在 default execution paths。
5. runtime/listener claimed-supported E2EE paths 不调用 legacy secure/group_e2ee implementation；允许调用 `im_core::secure::*` wrapper。
6. awiki-cli 构建启用了 im-core group-e2ee 所需 feature。
7. Linux/macOS release/package 构建启用 E2EE；Windows 不作为本阶段阻塞项。
```

---

## 10. 冒烟命令

Supported surface：

```bash
awiki-cli msg send --to <peer> --text "secure hello" --secure required
awiki-cli msg secure status --with <peer>
awiki-cli msg secure repair --with <peer>

awiki-cli group create --name "secure-test" --secure required
awiki-cli msg send --group <group_did> --text "secure group hello" --secure required
awiki-cli group secure status --group <group_did>
awiki-cli group secure repair --group <group_did>
awiki-cli group add --group <group_did> --member <peer> --secure required
awiki-cli group remove --group <group_did> --member <peer> --secure required
awiki-cli group leave --group <group_did> --secure required
```

Deprecated alias：

```bash
awiki-cli msg send --to <peer> --text "secure hello" --secure on
awiki-cli group create --name "secure-test" --e2ee
awiki-cli group create --name "secure-test" --message-security-profile group-e2ee
awiki-cli group e2ee status --group <group_did>
awiki-cli group e2ee repair --group <group_did>
```

Blocked / internal：

```bash
awiki-cli msg secure outbox list
awiki-cli group secure diagnostics --group <group_did>
awiki-cli group secure repair --group <group_did> --explain
awiki-cli group e2ee publish-key-package --group <group_did>
awiki-cli group e2ee pending --group <group_did>
awiki-cli group e2ee recover-member --group <group_did> --member <member>
awiki-cli group e2ee update-key --group <group_did> --member <member>
awiki-cli group e2ee rejoin --group <group_did> --member <member>
```

期望：

```text
1. supported surface 全部通过 im-core 执行。
2. deprecated alias 能转发到 canonical command，并输出 warning。
3. blocked/internal commands 返回 stable unsupported，或要求 InternalService/TestOnly gate。
```

---

## 11. 完成检查清单

```text
[x] awiki-cli 启用 supported group E2EE 所需 im-core feature。
[x] 已明确“默认打开”不改变未指定 `--secure` 的默认发送语义。
[x] msg send --secure required direct path 进入 im-core。
[x] msg send --secure required group path 进入 im-core。
[x] msg send --secure on/e2ee/secure-direct/group-e2ee 是 deprecated alias。
[x] parser/schema/help/completion 默认展示 canonical `--secure required`，旧值不在 default completion。
[x] secure attachment 在 im-core 支持前保持 fail-closed。
[x] msg secure status/repair 使用 client.secure().direct()。
[x] msg secure failed/retry/drop 不在 default surface。
[x] msg secure outbox * 不作为 supported CLI。
[x] im-core 已补齐 secure-aware group lifecycle public API 和 runtime implementation。
[x] group create/add/remove/leave --secure required 使用 im-core secure-aware lifecycle。
[x] group create/add/remove/leave --e2ee 是 deprecated alias。
[x] group secure status/repair 使用 client.secure().group()。
[x] group e2ee status/repair 是 deprecated alias。
[x] group e2ee low-level commands 是 hidden/internal/unsupported。
[x] group secure diagnostics / repair --explain 不作为本版本 supported CLI。
[x] default schema/help/completion 只展示高层 E2EE 产品命令。
[x] schema --all 展示 deprecated/internal entries 和 policy metadata。
[x] dry-run 也经过 policy gate，不能绕过 hidden/internal/unsupported。
[x] app handlers 不调用 awiki-cli::message::secure_* 或 group_e2ee_* implementation。
[x] im_core_adapter 不调用 awiki-cli legacy E2EE implementation。
[x] runtime/listener 中声明 supported 的 E2EE default path 不调用 legacy E2EE implementation。
[x] im-core public API 不暴露 raw KeyPackage/prekey/MLS notice/provider binary/raw outbox row。
[x] secure outbox public API 如保留，仅返回 redacted/high-level DTO，CLI 不暴露 outbox diagnostics。
[x] docs/architecture 和 skills 中旧命令面已同步。
[x] Linux package/release 构建已确认 group-e2ee feature、anp/mls 依赖和 artifact；macOS 使用同一脚本 gate，需在 macOS runner 产出 artifact。
[x] Windows E2EE package/release 暂缓策略已记录，不阻塞本阶段 Linux/macOS 打开。
[x] cutover unsupported 旧测试改为 supported/alias/internal policy 测试。
[x] workspace `--lib` 单元测试已用 repository default stable toolchain 和 `--locked` 通过；未运行连接真实域名/服务的系统测试。
```
