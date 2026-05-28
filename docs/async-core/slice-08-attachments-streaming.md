# 切片 08：Attachments 异步流式传输

## 目标

将附件上传和下载改为真正 async streaming，避免大文件完整读入单个 `Vec<u8>`。

本切片保留附件 manifest、digest、message projection 和 public DTO 语义；如需扩展 progress/cancellation options，必须保持向后兼容。

## 依赖

依赖切片：

```text
slice-02-async-http-transport.md
slice-03-identity-bootstrap-auth.md
slice-04-local-state-db-actor.md
slice-05-messages-async.md
slice-07-groups-async.md
```

## 当前代码锚点

重点改造：

```text
crates/im-core/src/attachments/**
crates/im-core/src/internal/attachment_runtime/**
crates/im-core/src/internal/blob/**
crates/im-core/src/internal/transport.rs
crates/im-core/src/internal/wire/attachment.rs
crates/im-core/src/internal/message_runtime/local_projection.rs
```

当前风险：

```text
LocalFile input 可通过 std::fs::read 读入完整 Vec<u8>
AttachmentObjectTransport 使用 Vec<u8> body/response
download 如果一次性 buffer 大文件会造成内存压力
```

## 设计要求

1. Local file upload 使用 streaming body。

   推荐：

   ```text
   tokio::fs::File
   tokio_util::io::ReaderStream
   reqwest async Body stream
   ```

2. Bytes input 可以继续使用内存 buffer，但不能影响 LocalFile 大文件路径。

3. Download 使用 streaming response 写入目标文件。

4. Atomic download 行为：

   ```text
   - 先写入 temp/.partial
   - 完成 digest/manifest validation 后 rename
   - cancel/失败时 cleanup 或保留明确 partial marker
   - overwrite false/true 语义保持
   ```

5. Progress event 设计不破坏现有 API。

   可以先作为内部 operation model；Dart/CLI 暴露由后续同步切片决定。

6. Digest/manifest validation 保持或增强。

7. Local message projection 使用 DB actor。

## 执行步骤

1. 重构 `BlobSource`。

   从：

   ```text
   path + bytes
   ```

   改为能表达：

   ```text
   in-memory bytes
   local file path + metadata
   async stream factory
   known content length
   digest calculator
   ```

2. 将 `AttachmentObjectTransport` 改为 async streaming transport。

   避免 public API 暴露 reqwest body 类型。内部可以使用 concrete stream/body wrapper。

3. 将 upload runtime async 化：

   ```text
   create_slot.await
   stream upload.await
   commit_object.await
   send manifest message.await
   local projection.await
   ```

4. 将 download runtime async 化：

   ```text
   resolve object URI/ticket.await
   stream download.await
   validate digest/manifest
   atomic rename
   ```

5. 添加 cancellation 支持。

   明确：

   ```text
   cancellation before upload commit: stop local work
   cancellation after object commit but before message send: report explicit partial state if needed
   cancellation after message submit: 不声称撤回
   ```

6. 添加大文件测试，确认不会完整分配到 `Vec<u8>`。

   测试可以使用 fake stream 统计 read chunk，而不是依赖进程内存检测。

## 上层同步

如果 `AttachmentService` public methods 改为 async，必须同步：

```text
crates/awiki-cli/src/m_core_cli_adapter/messages.rs
crates/awiki-cli/src/cli_shell/msg_handlers.rs
crates/im-core-dart/src/api/attachments.rs
packages/awiki_im_core/lib/src/**
```

如果新增 progress API，必须同步 Dart Stream model 和 CLI 输出策略，或明确保持内部能力暂不暴露。

## 测试

本切片必须运行：

```bash
cargo test -p im-core attachments --locked
cargo test -p im-core attachment_streaming --locked
cargo check -p im-core --locked
```

稳定性测试：

```text
- small file upload
- large file upload streaming without full Vec allocation
- bytes input still works
- download overwrite false/true
- cancellation during download
- progress event order if exposed
- manifest/digest validation
- local attachment message projection
```

Grep 检查：

```bash
rg "std::fs::read|read_to_end|Vec<u8>" crates/im-core/src/internal/blob crates/im-core/src/internal/attachment_runtime crates/im-core/src/attachments
```

`Vec<u8>` 可以存在于 Bytes input 和 small manifest/metadata，但不能是 LocalFile 大文件 transfer 主路径。

## 验收

```text
1. LocalFile upload/download 是 async streaming。
2. 大文件路径不完整读入单个 Vec<u8>。
3. manifest/digest 行为不变。
4. local projection 使用 DB actor。
5. 上层调用者已同步或登记到切片 11/12。
```

## 完成报告

报告必须包含：

```text
- streaming transport 设计
- Bytes input 兼容策略
- atomic download 策略
- cancellation 语义
- 大文件测试结果
- CLI/Dart 同步状态
```
