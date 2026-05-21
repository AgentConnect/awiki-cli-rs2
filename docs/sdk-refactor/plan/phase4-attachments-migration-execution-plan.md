# Phase 4：附件迁入 im-core 执行方案

**适用仓库**：`AgentConnect/awiki-cli-rs2`  
**适用阶段**：`docs/sdk-refactor/implementation-playbook.md` 中的 `18. Phase 4：附件`  
**目标**：在普通 direct/group text message、message/group/local_state 主链路稳定后，把附件上传、manifest、发送、下载能力按 leaf-file / 小子模块级切片迁入 `crates/im-core`，同时保持 `awiki-cli` 的 CLI 参数、路径校验、文件权限、输出行为和 legacy fallback 稳定。

---

## 1. 总体结论

Phase 4 不采用整体模块迁移，也不把现有 `message/attachment*` 一次性整体搬进 `im-core`。

推荐迁移粒度：

```text
主策略：leaf-file / 小子模块级迁移
辅策略：2-5 个强相关文件组成一个垂直业务切片
例外：函数级抽取只用于拆掉少量 CLI 依赖，不作为长期迁移单位
禁止：整体迁移 message、store、runtime、secure、group_e2ee、app handlers
```

Phase 4 的核心目标：

```text
client.attachments().send(target, request)
client.attachments().download(request)
AttachmentInput::LocalFile
AttachmentInput::Bytes
AttachmentDestination::LocalFile
AttachmentDestination::Memory
manifest / digest / slot / upload / commit / ticket / temp file / controlled atomic write
```

CLI 仍然保留：

```text
--file
--text-file
--output
overwrite policy
path validation
file permission strategy
stdout/stderr / pretty/json/table output
dry-run plan rendering
```

Phase 4 不做：

```text
secure attachment encryption
group E2EE attachment flow
realtime attachment notification runner
external BlobStore provider
platform-specific app storage provider
```

这些属于 Phase 6 secure 或 Phase 7 provider 抽象。

---

## 2. 与主方案的关系

`docs/sdk-refactor/modules/09-attachments.md` 已经定义附件模块职责：

```text
send(target, AttachmentSendRequest)：读取、digest、slot、上传、commit、manifest 和消息发送
download(DownloadAttachmentRequest)：附件定位、ticket 获取、下载和写入目标
select_for_download(messages, message_id, attachment_id)
```

Phase 4 执行计划只把这些职责拆成可落地的 PR 切片。

重要边界：

```text
AttachmentService 默认 public API 只暴露 send() 和 download()。
select_for_download 是 attachments::selection internal helper 或 compat/diagnostics-only helper。
除非同步更新 public-api.md、modules/09-attachments.md 和 architecture.md，否则不要把 select_for_download 作为默认 public API。
```

执行原则沿用 P1-beta：

```text
1. im-core 不依赖 awiki-cli。
2. awiki-cli 可以通过 compat wrapper 调 im-core。
3. awiki-cli 原文件路径和函数签名先保留。
4. 新 im-core 测试覆盖新实现。
5. 旧 awiki-cli 测试继续覆盖 wrapper 兼容。
6. compat API 不进 prelude，不承诺 semver。
```

---

## 3. 进入条件

开始 Phase 4 前，建议满足：

```text
1. P1 direct/group text send 可走 im-core 或稳定 adapter。
2. Phase 3 message/local_state 基础能力稳定，至少 SendMessageResult / Message DTO 已可复用。
3. Auth ensure/refresh 可由 im-core 或 adapter 完成。
4. MessageTarget::Direct / Group 路由稳定。
5. Attachment 调用仍有 legacy fallback。
6. im-core boundary 测试确认不引用 CLI 类型。
```

建议进入前检查：

```bash
cargo test -p im-core
cargo test -p awiki-cli --test msg_contract
cargo test -p awiki-cli --test message_contract
rg "ParsedCommand|ExitError|GlobalOptions|config::Resolved|identity::Manager|awiki_cli" crates/im-core/src crates/im-core/tests
```

如果本地没有 attachment-specific contract test，应在 Phase 4A 或 4B 增加 `im-core` attachment unit/contract tests，并保留现有 `awiki-cli` message tests。

---

## 4. Phase 4 canonical AttachmentInput

P1 的 `MessageBody::Attachment` 只是 reserved shape。若 P1 中存在 reserved `AttachmentInput`，它不是稳定 canonical DTO。

Phase 4 正式引入 canonical 类型：

```rust
pub enum AttachmentInput {
    LocalFile(PathBuf),
    Bytes {
        filename: Option<String>,
        mime_type: Option<String>,
        bytes: Vec<u8>,
    },
}
```

Phase 4 后必须满足：

```text
1. MessageBody::Attachment 复用 attachments::AttachmentInput。
2. 不再维护独立的 messages::AttachmentInput。
3. P1 reserved LocalFile(String) / Bytes { bytes_len } 若存在，需通过 type alias / re-export / migration adapter 收敛。
4. canonical AttachmentInput 不携带 CLI flag 名，不携带 workspace 发现逻辑。
```

---

## 5. 目标目录和 API 形态

建议新增：

```text
crates/im-core/src/attachments/
  mod.rs
  dto.rs
  service.rs
  manifest.rs
  selection.rs

crates/im-core/src/internal/attachment_runtime/
  mod.rs
  digest.rs
  upload.rs
  download.rs
  temp_file.rs
  atomic_write.rs

crates/im-core/src/internal/wire/attachment.rs

crates/im-core/src/internal/blob/
  mod.rs
  source.rs
  sink.rs

crates/im-core/src/compat/
  attachments.rs
```

`lib.rs` / `prelude.rs` 在 Phase 4 才加入默认 public service：

```rust
impl ImClient {
    pub fn attachments(&self) -> crate::attachments::AttachmentService<'_>;
}
```

Public API 形态：

```rust
pub struct AttachmentService<'a> {
    client: &'a ImClient,
}

impl AttachmentService<'_> {
    pub fn send(
        &self,
        target: MessageTarget,
        request: AttachmentSendRequest,
    ) -> ImResult<SendMessageResult>;

    pub fn download(
        &self,
        request: DownloadAttachmentRequest,
    ) -> ImResult<DownloadedAttachment>;
}
```

`select_for_download` 不进入默认 public service。它可以存在于：

```text
attachments::selection internal helper
compat/diagnostics-only helper
AttachmentService::download() 内部步骤
```

建议 DTO：

```rust
pub struct AttachmentSendRequest {
    pub input: AttachmentInput,
    pub caption: Option<String>,
    pub mime_type: Option<String>,
    pub filename: Option<String>,
    pub delivery: MessageDeliveryOptions,
}

pub enum AttachmentInput {
    LocalFile(PathBuf),
    Bytes {
        filename: Option<String>,
        mime_type: Option<String>,
        bytes: Vec<u8>,
    },
}

pub struct DownloadAttachmentRequest {
    pub thread: ThreadRef,
    pub message_id: MessageId,
    pub attachment_id: Option<String>,
    pub destination: AttachmentDestination,
    pub overwrite: bool,
}

pub enum AttachmentDestination {
    LocalFile(PathBuf),
    Memory,
}

pub struct DownloadedAttachment {
    pub attachment_id: String,
    pub filename: Option<String>,
    pub mime_type: Option<String>,
    pub size_bytes: Option<u64>,
    pub destination: DownloadedAttachmentDestination,
    pub warnings: Vec<String>,
}

pub enum DownloadedAttachmentDestination {
    LocalFile(PathBuf),
    Memory(Vec<u8>),
}
```

不进入 public API：

```text
prepare_upload
create_slot
commit_object
download_ticket_rpc_params
upload chunk/request internals
raw response JSON
temporary file path internals
SQLite connection
select_for_download as default service method
```

---

## 6. 文件写入职责合同

Phase 4 的 `AttachmentDestination::LocalFile(PathBuf)` 允许 SDK 执行受控写入，但写入策略由 CLI/App 决定。

CLI / App 决定：

```text
output path 来自哪里
是否允许 overwrite
权限策略
目标路径是否允许写
用户确认和错误提示
是否展示 path
```

im-core 负责：

```text
只对显式传入的 AttachmentDestination::LocalFile 写入
使用 CLI/App/ImCorePaths 明确提供的 temp 策略
下载失败时清理 temp
atomic rename 到目标文件
不自行发现 workspace
不自行改变权限策略
不绕过 overwrite=false
```

因此不要把“路径合法性、权限策略、overwrite UX”迁到 SDK；也不要让 SDK 通过 CLI config 推导输出路径。

---

## 7. 通用边界规则

`im-core` 不能直接使用：

```text
ParsedCommand
ExitError
GlobalOptions
config::Resolved
identity::Manager
message::AttachmentDownloadRequest
message::SendRequest
message::MessageError
store::MessageRecord
CLI output envelope
```

允许的迁移方式：

```text
1. awiki-cli wrapper 把 legacy CLI flags / request 转成 im-core DTO。
2. im-core internal 使用自己的 AttachmentRecord / Manifest / Slot / Ticket DTO。
3. im-core compat 暂时为 awiki-cli 暴露迁移期函数。
4. legacy wrapper 稳定两个阶段后再删除。
```

CLI 负责：

```text
--file / --text-file / --output 参数解析
本地输入路径存在性检查
输出路径 overwrite policy
输出文件权限策略
dry-run 文案
pretty/json/table 渲染
```

SDK 负责：

```text
digest
manifest
slot
upload
commit object
download ticket
download bytes
destination controlled write / memory return
SendMessageResult / DownloadedAttachment normalize
```

---

## 8. Compat 与 internal trait 规则

Phase 4 可能需要 internal trait：

```text
AttachmentBlobSource
AttachmentBlobSink
AttachmentUploader
AttachmentDownloader
AttachmentTempFileFactory
```

这些必须是：

```text
internal-only
compat-only
不是 Phase 7 public BlobStore provider
不进入 prelude
不承诺 semver
```

如果 `awiki-cli` 需要调用 `im_core::compat::attachments`，规则是：

```text
1. compat API 不进入 prelude。
2. compat API 使用 #[doc(hidden)]。
3. compat API 不作为 SDK semver 稳定 API。
4. 发布独立 crate 前应放到 non-default feature 或清理。
```

---

## 9. 测试分层规则

### 9.1 Required：Codex Goal / 单 PR 必跑

```text
cargo test -p im-core attachments
cargo test -p awiki-cli --test msg_contract
rg import fence
```

待新增 attachment contract tests：

```text
cargo test -p awiki-cli --test attachment_contract
cargo test -p awiki-cli --test msg_attachment_contract
```

如果这些 test target 尚不存在，不要在 Codex Goal 中把它们当作当前已存在测试执行。

### 9.2 Optional integration：合并前或本地补跑

```text
cargo test -p awiki-cli --test message_contract
cargo test -p awiki-cli --test group_contract
cargo test -p awiki-cli --test store_messages_contract
```

### 9.3 Manual / live / system：不由默认 Codex Goal 执行

当前仓库已有 live/system target：

```text
cargo test -p awiki-cli --test attachment_live_contract
```

真实 CLI 验证：

```text
awiki-cli msg send --file ...
awiki-cli msg attachment download ...
真实网络上传/下载
真实大文件写入
真实 workspace 操作
```

只有当某个 PR 明确声明进入系统验证时，才运行 Manual / live / system 测试。

---

## 10. PR 4A：Attachment DTO / Service skeleton

### 10.1 目标

建立 Phase 4 public API 形态，不迁真实上传/下载。

### 10.2 改动范围

```text
crates/im-core/src/attachments/mod.rs
crates/im-core/src/attachments/dto.rs
crates/im-core/src/attachments/service.rs
crates/im-core/src/core/client.rs
crates/im-core/src/lib.rs
crates/im-core/src/prelude.rs
crates/im-core/tests/attachment_api.rs
```

### 10.3 执行步骤

```text
1. 新增 AttachmentService。
2. 在 ImClient 上新增 attachments()。
3. 新增 canonical AttachmentInput / AttachmentSendRequest / DownloadAttachmentRequest / DownloadedAttachment DTO。
4. MessageBody::Attachment 复用 canonical AttachmentInput。
5. send/download 先返回 UnsupportedCapability 或明确 stub。
6. 不改 awiki-cli handler。
7. 增加 DTO 构造、UnsupportedCapability、public API boundary 测试。
```

### 10.4 Required 验收

```bash
cargo test -p im-core attachments
rg "ParsedCommand|ExitError|config::Resolved|identity::Manager|awiki_cli" crates/im-core/src crates/im-core/tests
```

### 10.5 完成标准

```text
1. AttachmentService public API 可编译。
2. P1/P3 message API 不受影响。
3. 没有真实上传/下载行为。
4. 未暴露 CLI 类型或 raw wire payload。
5. select_for_download 未进入 default public service。
```

---

## 11. PR 4B：manifest / digest / selection 纯逻辑迁移

### 11.1 目标

把附件 manifest、digest、选择逻辑迁入 `im-core`，不接远端上传/下载。

### 11.2 源和目标

源：

```text
crates/awiki-cli/src/message/attachment.rs
```

目标：

```text
crates/im-core/src/attachments/manifest.rs
crates/im-core/src/attachments/selection.rs
crates/im-core/src/internal/attachment_runtime/digest.rs
crates/im-core/src/compat/attachments.rs
```

### 11.3 迁移范围

可迁移：

```text
attachment_manifest_content_type
build_attachment_manifest
manifest_content_string
find_attachment_selection
find_attachment_selection_with_paging 的纯选择逻辑
digest helper
filename / mime-type normalize helper
```

暂不迁移：

```text
load_attachment_file 具体 CLI path 读取
upload slot / commit
download ticket
atomic output write
MessageError 构造
CLI warning / summary 文案
```

### 11.4 执行方式

```text
1. im-core 内定义 Manifest / AttachmentDescriptor / AttachmentSelection DTO。
2. awiki-cli 原 attachment.rs 保留旧函数名和旧签名。
3. awiki-cli 原函数内部可调用 im_core::compat::attachments。
4. MessageError 映射仍在 awiki-cli wrapper 中完成。
5. 复制 manifest/selection 相关测试到 im-core。
```

### 11.5 Required 验收

```bash
cargo test -p im-core attachments
cargo test -p awiki-cli --test msg_contract
```

### 11.6 完成标准

```text
1. manifest/digest/selection 逻辑由 im-core 覆盖测试。
2. awiki-cli 旧函数仍兼容。
3. select_for_download 只作为 internal/compat helper。
4. 不触发远端上传/下载。
```

---

## 12. PR 4C：attachment service discovery / endpoint selection

### 12.1 目标

把 attachment service endpoint discovery 和选择规则迁入 `im-core`。

### 12.2 源和目标

源：

```text
crates/awiki-cli/src/message/service_discovery.rs
```

目标：

```text
crates/im-core/src/internal/discovery/attachment.rs
crates/im-core/src/compat/attachments.rs
```

### 12.3 范围

支持：

```text
select attachment service from DID document
fallback to ImCoreConfig attachment/message service endpoint
capability requirement check
invalid endpoint error mapping
```

暂不支持：

```text
full DID document cache
provider-based service discovery
secure attachment service capability
```

### 12.4 Required 验收

```bash
cargo test -p im-core attachment_discovery
cargo test -p awiki-cli --test msg_contract
```

### 12.5 完成标准

```text
1. attachment endpoint selection 可由 im-core 完成。
2. awiki-cli 原 service_discovery wrapper 兼容。
3. discovery 不读取 CLI config。
```

---

## 13. PR 4D：upload slot / commit / attachment send wire builder

### 13.1 目标

迁移附件上传相关 wire builder，但不接真实文件读取和远端上传。

### 13.2 源和目标

源：

```text
crates/awiki-cli/src/message/attachment.rs
```

目标：

```text
crates/im-core/src/internal/wire/attachment.rs
crates/im-core/src/compat/attachments.rs
```

### 13.3 迁移范围

可迁移：

```text
build_attachment_create_slot_rpc_params
build_attachment_commit_object_rpc_params
build_direct_attachment_send_rpc_params
build_group_attachment_send_rpc_params
build_attachment_download_ticket_rpc_params
AttachmentCreateSlotResult normalize
AttachmentCommitObjectResult normalize
AttachmentDownloadTicketResult normalize
```

暂不迁移：

```text
actual HTTP upload/download transport
file read/write
direct/group text send general flow
secure attachment
```

### 13.4 Required 验收

```bash
cargo test -p im-core attachment_wire
cargo test -p awiki-cli --test msg_contract
```

### 13.5 完成标准

```text
1. slot/commit/ticket/send wire shape 由 im-core 覆盖测试。
2. awiki-cli 旧函数可 wrapper 到 im-core。
3. group attachment wire 不触碰 group lifecycle。
```

---

## 14. PR 4E：Attachment upload runtime

### 14.1 目标

实现 `AttachmentService::send()` 的上传主链路，但先保留 legacy fallback。

### 14.2 调用链

```text
AttachmentService::send
  -> validate AttachmentInput
  -> resolve target MessageTarget
  -> ensure_session(AuthScope::Messaging 或 GroupMessaging)
  -> resolve attachment endpoint
  -> read bytes / stream source
  -> digest
  -> create slot
  -> upload object
  -> commit object
  -> build manifest
  -> send direct/group attachment message
  -> map SendMessageResult
  -> best-effort local persist
```

### 14.3 目标文件

```text
crates/im-core/src/attachments/service.rs
crates/im-core/src/internal/attachment_runtime/upload.rs
crates/im-core/src/internal/blob/source.rs
crates/im-core/src/internal/transport.rs
crates/im-core/src/compat/attachments.rs
crates/awiki-cli/src/im_core_adapter/messages.rs
```

### 14.4 风险控制

```text
1. LocalFile path 已由 CLI adapter 校验。
2. Bytes input 可在 im-core unit test 中使用，避免真实文件系统。
3. LocalFile read 只能读取显式传入 path。
4. 上传失败返回 ImError::Service / TransportUnavailable。
5. local persist 失败只产生 warning，不阻断发送。
6. attachment secure mode 返回 UnsupportedCapability。
```

### 14.5 Required 验收

```bash
cargo test -p im-core attachments
cargo test -p awiki-cli --test msg_contract
```

### 14.6 Manual / live / system

```bash
cargo test -p awiki-cli --test attachment_live_contract
awiki-cli msg send --to <peer> --file <path> --text "caption"
awiki-cli msg send --group <group> --file <path> --text "caption"
```

### 14.7 完成标准

```text
1. AttachmentInput::Bytes 可完整跑 upload unit/contract test。
2. LocalFile 路径由 CLI adapter 传入，im-core 不发现 workspace。
3. direct/group attachment message 可经 im-core 主链路发送。
4. legacy fallback 可回退。
```

---

## 15. PR 4F：Attachment download selection / ticket / memory destination

### 15.1 目标

实现附件下载定位、ticket 获取和 memory destination，不先接 CLI 文件写入。

### 15.2 调用链

```text
AttachmentService::download
  -> locate message by thread/message_id
  -> select attachment by attachment_id or first attachment
  -> ensure_session
  -> build download ticket request
  -> fetch bytes
  -> if Memory: return Vec<u8>
```

### 15.3 目标文件

```text
crates/im-core/src/attachments/service.rs
crates/im-core/src/attachments/selection.rs
crates/im-core/src/internal/attachment_runtime/download.rs
crates/im-core/src/internal/blob/sink.rs
```

### 15.4 Required 验收

```bash
cargo test -p im-core attachments
cargo test -p awiki-cli --test msg_contract
```

### 15.5 完成标准

```text
1. AttachmentDestination::Memory 可通过 unit test 验证。
2. ticket wire shape 与 legacy 一致。
3. multiple attachments 选择规则与 legacy 一致。
4. select_for_download 仍不是 public service method。
5. 未做 LocalFile atomic write 时不改 CLI download handler 默认路径。
```

---

## 16. PR 4G：LocalFile destination / temp file / controlled atomic write

### 16.1 目标

实现 `AttachmentDestination::LocalFile`，支持 temp file 和 atomic rename。

### 16.2 职责合同

CLI 决定：

```text
output path
overwrite policy
permission strategy
用户确认
错误提示
```

SDK 执行：

```text
explicit LocalFile destination write
temp file creation according to explicit strategy
download bytes -> temp
atomic rename temp -> destination
failure cleanup
overwrite=false enforcement
```

### 16.3 范围

支持：

```text
explicit output path
overwrite policy passed by CLI/App
temp file in RuntimePaths.temp_dir 或 output sibling temp
atomic rename
partial download cleanup
file permission handoff according to caller strategy
```

CLI 保留：

```text
--output 参数解析
overwrite confirmation / policy
human-readable path errors
permission UX
```

### 16.4 Required 验收

```bash
cargo test -p im-core attachments
cargo test -p awiki-cli --test msg_contract
```

### 16.5 Manual / live / system

```bash
awiki-cli msg attachment download --message-id <id> --output <path>
```

### 16.6 完成标准

```text
1. LocalFile write 使用 controlled atomic strategy。
2. download 失败不留下破损目标文件。
3. output overwrite 行为和 CLI policy 一致。
4. im-core 不自行发现 workspace，不自行改变权限策略。
```

---

## 17. PR 4H：CLI attachment handler 切换与 compat 清理

### 17.1 目标

让 `msg send --file` 和 `msg attachment download` 可以通过 `im-core` 路径执行，同时保留 fallback。

### 17.2 范围

```text
msg send --to --file
msg send --group --file
msg attachment download
dry-run plan 仍由 CLI 渲染
legacy fallback 保留一个阶段
```

### 17.3 Required 验收

```bash
cargo test -p im-core attachments
cargo test -p awiki-cli --test msg_contract
```

### 17.4 Manual / live / system

```bash
cargo test -p awiki-cli --test attachment_live_contract
awiki-cli msg send --to <peer> --file <path>
awiki-cli msg attachment download --message-id <id> --output <path>
```

### 17.5 完成标准

```text
1. CLI handler 不再直接拼 attachment wire params。
2. CLI 仍负责 input/output path policy。
3. legacy attachment_service wrapper 可逐步收敛。
4. secure/group E2EE attachment 未被误动。
```

---

## 18. 错误映射规则

`im-core` 返回：

```text
InvalidInput(field=file)
InvalidInput(field=destination)
AttachmentNotFound
UnsupportedCapability(attachments-secure)
TransportUnavailable
Service
PathUnavailable
Io
Internal
```

`awiki-cli` wrapper 映射：

```text
AttachmentNotFound -> MessageError::AttachmentNotFound
InvalidInput(field=output) -> MessageError::OutputPathRequired / invalid_argument
UnsupportedCapability -> MessageError::AttachmentNotSupported 或 CLI unsupported hint
PathUnavailable / Io -> ExitError path/permission hint
```

规则：

```text
1. im-core 不知道 CLI flag 名。
2. im-core 不生成 CLI help/hint 文案。
3. CLI 不把 raw RPC response 作为 attachment public DTO。
```

---

## 19. 回滚策略

```text
1. im-core 新实现先落地。
2. awiki-cli wrapper 再切过去。
3. feature flag / adapter fallback 保留一个阶段。
4. 出问题时只回滚 wrapper 调用点，im-core 新代码可暂时保留但不走默认路径。
5. compat API 稳定两个阶段后清理。
```

涉及文件写入的回滚规则：

```text
1. LocalFile download 必须先写 temp，再 atomic rename。
2. 失败时清理 temp。
3. 不覆盖目标文件，除非 CLI 明确允许 overwrite。
4. 回滚时旧 CLI download path 可继续工作。
```

---

## 20. 明确不做事项

Phase 4 不做：

```text
1. 不迁 secure direct attachment encryption。
2. 不迁 group E2EE attachment。
3. 不迁 realtime attachment notification runner。
4. 不迁 external BlobStore provider。
5. 不迁 OpenClaw / Hermes。
6. 不整体迁移 message/attachment_service.rs。
7. 不把 upload slot / commit / download ticket 暴露为 public SDK API。
8. 不让 im-core 读取 CLI config 或发现 workspace。
9. 不把 select_for_download 暴露为默认 public service method。
```

---

## 21. 方案核心

Phase 4 的核心是：

```text
先迁 manifest/digest/selection 纯逻辑，
再迁 slot/commit/ticket wire，
再接 upload/download runtime，
最后切 CLI handler。
```

这样既能保留旧测试和旧 CLI 路径，又能把附件能力逐步收敛到 `im-core` 的高层 `AttachmentService`。
