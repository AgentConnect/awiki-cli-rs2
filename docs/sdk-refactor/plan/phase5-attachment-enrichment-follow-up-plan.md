# Phase 5'：attachment realtime enrichment follow-up 执行方案

**适用仓库**：`AgentConnect/awiki-cli-rs2`  
**适用阶段**：Phase 5 core realtime runner 与 Phase 4 attachments 之后的回补阶段  
**推荐执行顺序**：`Phase 5 core -> Phase 4 attachments -> Phase 5' attachment enrichment follow-up`  
**目标**：在 realtime runner 和 AttachmentService 都稳定后，把附件类 notification 从 generic projection 升级为 attachment-aware projection，同时不把附件 upload/download runtime、CLI 文件路径策略或 host notification delivery 重新塞回 realtime runner。

---

## 1. 总体结论

Phase 5' 是 Phase 5 core 的后续增强，不是 Phase 4 的一部分。

Phase 5 core 先于 Phase 4 执行时，附件类 notification 只能做：

```text
generic MessageReceived
Unsupported body
metadata content_type
UnknownNotification
```

Phase 4 完成后，Phase 5' 才允许使用 canonical attachment DTO / manifest / selection 逻辑来增强 realtime projection：

```text
attachment notification classify
attachment metadata extraction
attachment-aware MessageReceivedEvent
attachment-aware local projection metadata
attachment host notification summary
download action metadata bridge
```

Phase 5' 不做：

```text
自动下载附件
自动上传附件
secure attachment decrypt
group E2EE attachment decrypt
OpenClaw / Hermes delivery implementation
CLI output/path policy
raw WebSocket frame public API
```

---

## 2. 进入条件

开始 Phase 5' 前必须满足：

```text
1. Phase 5 core realtime runner 已稳定，CLI listener run/service-run 可调用 SDK runner。
2. Phase 4 AttachmentService public API 已稳定：send() / download()。
3. canonical attachments::AttachmentInput / AttachmentDestination / DownloadedAttachment 已落地。
4. manifest / selection / ticket metadata normalize 逻辑已经在 im-core 有测试覆盖。
5. Phase 5 core 对附件类 notification 的 generic projection 仍可 fallback。
6. im-core 仍不依赖 awiki-cli runtime / CLI path / host notification sink 类型。
```

建议进入前检查：

```bash
cargo test -p im-core realtime
cargo test -p im-core attachments
cargo test -p awiki-cli --test host_runtime_listener_bridge_dispatch_contract
cargo test -p awiki-cli --test msg_contract
rg "ParsedCommand|ExitError|GlobalOptions|config::Resolved|awiki_cli" crates/im-core/src crates/im-core/tests
```

---

## 3. 与 Phase 4 / Phase 5 的关系

Phase 5 core 负责：

```text
WebSocket connect
frame classify
pending dispatch
heartbeat / reconnect
generic notification -> ImEvent
runner / handle / shutdown
CLI listener host bridge
```

Phase 4 负责：

```text
AttachmentService::send
AttachmentService::download
canonical attachment DTO
manifest / digest / selection
slot / commit / ticket
controlled LocalFile write / Memory destination
```

Phase 5' 只负责两者之间的 enrich bridge：

```text
notification payload -> attachment metadata
attachment metadata -> MessageReceivedEvent enrichment
attachment metadata -> local projection metadata
attachment metadata -> host notification domain event summary
attachment metadata -> CLI download action hint
```

边界规则：

```text
1. Phase 5' 可以读取 attachment manifest/metadata，但不执行 download。
2. Phase 5' 可以生成 download action metadata，但不决定 output path。
3. Phase 5' 可以让 CLI host notification 展示附件摘要，但不投递 OpenClaw/Hermes。
4. Phase 5' 不新增 AttachmentService public method。
5. Phase 5' 不把 select_for_download 暴露成 default public service method。
```

---

## 4. 目标目录和 API 形态

优先复用 Phase 4 / Phase 5 已有目录：

```text
crates/im-core/src/internal/realtime/projection.rs
crates/im-core/src/internal/realtime/attachment_projection.rs
crates/im-core/src/realtime/events.rs
crates/im-core/src/attachments/manifest.rs
crates/im-core/src/attachments/selection.rs
crates/im-core/src/compat/realtime.rs
crates/awiki-cli/src/im_core_adapter/realtime.rs
```

public API 原则：

```text
1. RealtimeService public shape 不变。
2. AttachmentService public shape 不变。
3. 可以扩展 ImEvent / MessageReceivedEvent 的 attachment metadata 字段，但必须是领域 DTO，不是 raw payload。
4. 需要兼容旧 generic projection；无法识别的附件 payload 仍返回 UnknownNotification 或 Unsupported body。
```

建议 DTO：

```rust
pub struct AttachmentMessageSummary {
    pub attachment_id: Option<String>,
    pub filename: Option<String>,
    pub mime_type: Option<String>,
    pub size_bytes: Option<u64>,
    pub content_type: Option<String>,
}

pub struct AttachmentDownloadAction {
    pub thread: ThreadRef,
    pub message_id: MessageId,
    pub attachment_id: Option<String>,
}
```

这些 DTO 只表达可展示 / 可路由的业务信息，不包含：

```text
download ticket
upload slot
raw manifest JSON
local output path
CLI flag name
OpenClaw/Hermes route config
```

---

## 5. PR 5'A：attachment notification classifier

### 5.1 目标

在 realtime projection 中识别附件类 notification，但不改变外部行为。

### 5.2 范围

支持：

```text
识别 direct attachment message notification
识别 group attachment message notification
提取 content_type / manifest marker / attachment marker
无法识别时 fallback generic projection
```

暂不支持：

```text
download action
host notification 文案增强
local projection schema 变更
secure / group E2EE attachment
```

### 5.3 Required 验收

```bash
cargo test -p im-core realtime_attachment_projection
cargo test -p im-core attachments
```

### 5.4 完成标准

```text
1. classifier 不依赖 awiki-cli。
2. raw notification 不进入 public ImEvent 主字段。
3. generic fallback 行为保留。
```

---

## 6. PR 5'B：MessageReceivedEvent attachment metadata enrichment

### 6.1 目标

把附件类 notification enrich 成可消费的领域 metadata。

### 6.2 范围

支持：

```text
AttachmentMessageSummary
message metadata content_type
filename / mime_type / size_bytes normalize
attachment_id normalize
warnings for partial metadata
```

暂不支持：

```text
自动下载
download ticket 获取
LocalFile write
CLI --output 推导
```

### 6.3 Required 验收

```bash
cargo test -p im-core realtime_attachment_projection
cargo test -p awiki-cli --test host_runtime_listener_bridge_dispatch_contract
```

### 6.4 完成标准

```text
1. MessageReceivedEvent 可携带附件摘要。
2. 无附件摘要时仍保持 Phase 5 core 行为。
3. 不暴露 raw manifest 作为 default public DTO。
```

---

## 7. PR 5'C：local projection attachment metadata

### 7.1 目标

把附件摘要投影到本地 message metadata，方便 inbox/history/host notification 使用。

### 7.2 范围

支持：

```text
local message metadata 写入 attachment summary
content_type / filename / mime_type / size_bytes
重复 notification 去重
legacy generic record 兼容读取
```

暂不支持：

```text
附件文件缓存
下载状态持久化
外部 blob provider
secure attachment decrypt
```

### 7.3 Required 验收

```bash
cargo test -p im-core realtime_attachment_projection
cargo test -p awiki-cli --test store_messages_contract
cargo test -p awiki-cli --test store_groups_contract
```

### 7.4 完成标准

```text
1. local projection 不重复写入消息。
2. attachment metadata 可被 history/inbox 读取。
3. legacy generic projection 记录仍可兼容。
```

---

## 8. PR 5'D：host notification summary / download action bridge

### 8.1 目标

让 CLI host notification adapter 可以基于 SDK event 显示附件摘要，并生成下载 action hint。

### 8.2 边界

SDK 产出：

```text
AttachmentMessageSummary
AttachmentDownloadAction
warnings
```

CLI 决定：

```text
通知文案最终渲染
OpenClaw / Hermes delivery
平台权限
用户点击行为
download output path
是否实际调用 client.attachments().download()
```

### 8.3 Required 验收

```bash
cargo test -p im-core realtime_attachment_projection
cargo test -p awiki-cli --test host_runtime_notify_contract
cargo test -p awiki-cli --test host_runtime_listener_bridge_dispatch_contract
```

### 8.4 Manual / live / system

```bash
awiki-cli runtime listener run
awiki-cli msg send --to <peer> --file <path>
awiki-cli msg attachment download --message-id <id> --output <path>
```

### 8.5 完成标准

```text
1. host notification adapter 可展示附件摘要。
2. download action 不包含 output path。
3. SDK 不调用 OpenClaw/Hermes sink。
4. CLI 可继续 fallback 到 generic notification。
```

---

## 9. 错误和 fallback 规则

```text
1. metadata 缺失：产生 warning，fallback generic MessageReceived。
2. attachment_id 缺失：download action 使用 None，由 download selection 决定。
3. manifest parse 失败：不阻断 runner，投递 UnknownNotification 或 Unsupported body。
4. AttachmentService 不可用：不阻断 realtime runner。
5. secure/group E2EE attachment：返回 unsupported / unknown，不尝试 decrypt。
```

`im-core` 不返回：

```text
ExitError
MessageError
CLI hint
OpenClaw/Hermes delivery error
```

---

## 10. 回滚策略

```text
1. 保留 Phase 5 core generic projection fallback。
2. attachment enrichment 通过小切片逐步打开。
3. 出问题时只回滚 attachment_projection / adapter 使用点。
4. 不回滚 Phase 4 AttachmentService send/download。
5. 不回滚 Phase 5 core runner。
```

---

## 11. 明确不做事项

Phase 5' 不做：

```text
1. 不自动下载附件。
2. 不实现 attachment upload/download runtime。
3. 不迁 secure attachment decrypt。
4. 不迁 group E2EE attachment decrypt。
5. 不迁 OpenClaw/Hermes delivery implementation。
6. 不决定 CLI output path。
7. 不暴露 download ticket / upload slot / raw manifest JSON。
8. 不新增 service manager / daemon socket 行为。
9. 不把 select_for_download 暴露成 default public API。
```

---

## 12. 方案核心

Phase 5' 的核心是：

```text
在 Phase 5 core runner 已经能稳定产出 ImEvent、
Phase 4 AttachmentService 已经能表达 canonical attachment DTO 之后，
只补 attachment notification 的领域 metadata enrichment，
不改变 runner 宿主模型，也不接管附件下载动作。
```

这样可以支持 `5 -> 4 -> 5'` 的执行顺序：先把 realtime engine 收敛到 `im-core`，再补附件 send/download，最后再把附件 notification 投影从 generic 升级为 attachment-aware。
