# attachments 模块接口设计

**阅读顺序**：09 / 11  
**所属 crate**：`crates/im-core`  
**模块职责**：附件上传、manifest、发送和下载。

## 1. 目标

`attachments` 负责附件业务流程。Phase A 使用显式本地路径；CLI 负责把 `--file`、`--text-file`、`--output` 转成 core DTO。

## 2. 主要职责

- `send(target, AttachmentSendRequest)`：完成读取、digest、slot、上传、commit、manifest 和消息发送。
- `select_for_download(messages, message_id, attachment_id)`。
- `download(DownloadAttachmentRequest)`：完成附件定位、ticket 获取、下载和写入目标。

## 3. Phase A 路径需求

- `AttachmentInput::LocalFile(PathBuf)`：CLI 从 `--file`、`--text-file`、stdin 临时文件等输入转换而来；App 可使用自己的 sandbox path。
- `AttachmentDestination::LocalFile(PathBuf)`：CLI 从 `--output` 转换而来；App 可使用自己的 sandbox path。
- `AttachmentTempPaths`：可选，用于大文件 digest、分片上传、下载临时文件和原子 rename。
- `ImCoreConfig`：attachment service endpoint。

## 4. 接口草案

```rust
pub struct AttachmentService<'a> {
    client: &'a ImClient,
}

impl AttachmentService<'_> {
    pub async fn send(
        &self,
        target: MessageTarget,
        request: AttachmentSendRequest,
    ) -> ImResult<SendMessageResult>;

    pub async fn download(
        &self,
        request: DownloadAttachmentRequest,
    ) -> ImResult<DownloadedAttachment>;
}
```

`prepare_upload`、`create_slot`、`commit_object`、`build_manifest` 是内部步骤，不应作为 App/CLI 主接口暴露。测试需要时可以放在 internal module 或 feature-gated helper 中。

## 5. CLI 边界

- `im-core` 不接收 CLI 的 `--output` 字符串作为业务概念；它接收 `AttachmentDestination::LocalFile` 这类领域化目的地。
- 本地路径合法性、覆盖策略、文件权限由 CLI 在调用前决定。
- Phase B 可把 path source/sink 扩展为 memory、app storage 或外部 blob 能力。
