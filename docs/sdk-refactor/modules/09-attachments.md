# attachments 模块接口设计

**所属 crate**：`crates/im-core`  
**阶段**：P4  
**职责**：附件上传、manifest、发送和下载。

## 1. 目标

`attachments` 负责附件业务流程。Phase A 使用显式本地路径；CLI 负责把 `--file`、`--text-file`、`--output` 转成 core DTO。

P1 不实现完整附件能力。若 P1 调用 `MessageBody::Attachment`，返回 `UnsupportedCapability`。

## 2. 职责

- `send(target, AttachmentSendRequest)`：完成读取、digest、slot、上传、commit、manifest 和消息发送。
- `download(DownloadAttachmentRequest)`：完成附件定位、ticket 获取、下载和写入目标。
- `select_for_download(messages, message_id, attachment_id)`。

## 3. DTO 草案

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

pub enum AttachmentInput {
    LocalFile(PathBuf),
    Bytes {
        filename: Option<String>,
        mime_type: Option<String>,
        bytes: Vec<u8>,
    },
}

pub enum AttachmentDestination {
    LocalFile(PathBuf),
    Memory,
}
```

## 4. 不暴露的内部步骤

`prepare_upload`、`create_slot`、`commit_object`、`build_manifest`、`download_ticket_rpc_params` 是内部步骤，不应作为 App/CLI 主接口暴露。测试需要时可以放在 internal module 或 feature-gated helper 中。

## 5. CLI 边界

- `im-core` 不接收 CLI 的 `--output` 字符串作为业务概念；它接收 `AttachmentDestination::LocalFile` 这类领域化目的地。
- 本地路径合法性、覆盖策略、文件权限由 CLI 在调用前决定。
- Phase 7 可把 path source/sink 扩展为 memory、app storage 或外部 blob 能力。
