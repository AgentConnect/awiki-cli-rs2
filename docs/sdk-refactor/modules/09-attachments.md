# attachments 模块接口设计

**阅读顺序**：09 / 11  
**所属 crate**：`crates/im-core`  
**模块职责**：附件上传、manifest、发送和下载。

## 1. 目标

`attachments` 负责附件业务流程。Phase A 使用显式本地路径；CLI 负责把 `--file`、`--text-file`、`--output` 转成 core DTO。

## 2. 主要职责

- `prepare_upload(source)`：Phase A 从显式 `AttachmentSourceRef::Path` 读取 bytes、识别 mime、计算 digest。
- `create_slot(actor, target, prepared)`。
- `commit_object(actor, slot, object)`。
- `build_manifest(object, caption)`。
- `send(actor, target, source, caption)`。
- `select_for_download(messages, message_id, attachment_id)`。
- `download(actor, target, message_id, attachment_id, sink)`。

## 3. Phase A 路径需求

- `AttachmentSourceRef::Path(PathBuf)`：CLI 从 `--file`、`--text-file`、stdin 临时文件等输入转换而来。
- `AttachmentSinkRef::Path(PathBuf)`：CLI 从 `--output` 转换而来。
- `AttachmentTempPaths`：可选，用于大文件 digest、分片上传、下载临时文件和原子 rename。
- `ImCoreConfig`：attachment service endpoint。

## 4. 接口草案

```rust
pub struct AttachmentService<'a> {
    core: &'a ImCore,
}

impl AttachmentService<'_> {
    pub fn prepare_upload(
        &self,
        source: AttachmentSourceRef,
    ) -> ImResult<PreparedAttachment>;

    pub async fn create_slot(
        &self,
        actor: ActorContext,
        target: MessageTarget,
        prepared: PreparedAttachment,
    ) -> ImResult<AttachmentSlot>;

    pub async fn commit_object(
        &self,
        actor: ActorContext,
        slot: AttachmentSlot,
        object: PreparedAttachment,
    ) -> ImResult<AttachmentObject>;

    pub fn build_manifest(
        &self,
        object: AttachmentObject,
        caption: Option<String>,
    ) -> ImResult<AttachmentManifest>;

    pub async fn send(
        &self,
        actor: ActorContext,
        target: MessageTarget,
        source: AttachmentSourceRef,
        caption: Option<String>,
    ) -> ImResult<SendMessageResult>;

    pub async fn download(
        &self,
        actor: ActorContext,
        request: DownloadAttachmentRequest,
        sink: AttachmentSinkRef,
    ) -> ImResult<DownloadedAttachment>;
}
```

## 5. CLI 边界

- `im-core` 不接收 CLI 的 `--output` 字符串作为业务概念；它接收 `AttachmentSinkRef::Path`。
- 本地路径合法性、覆盖策略、文件权限由 CLI 在调用前决定。
- Phase B 可把 path source/sink 扩展为 memory、app storage 或外部 blob 能力。
