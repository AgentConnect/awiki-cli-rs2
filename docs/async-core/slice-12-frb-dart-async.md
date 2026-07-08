# 切片 12：FRB / Dart 异步桥接

## 目标

将 `im-core-dart` Rust bridge 改为 async，并让 Dart/Flutter public API 继续保持 Future/Stream 语义。

本切片必须同步 Rust bridge、FRB generated bindings、Dart models 和 Flutter package facade。

## 依赖

依赖切片：

```text
slice-03-identity-bootstrap-auth.md
slice-05-messages-async.md
slice-06-directory-profile-content.md
slice-07-groups-async.md
slice-08-attachments-streaming.md
slice-09-e2ee-secure-async.md
slice-10-realtime-runner-async.md
```

## 当前代码锚点

重点改造：

```text
crates/im-core-dart/src/api/**
crates/im-core-dart/src/dto/**
crates/im-core-dart/src/mapping/**
packages/awiki_im_core/lib/src/**
packages/awiki_im_core/test/**
```

## 设计要求

1. Rust bridge functions 调用 `im-core` async methods 时必须自身 async。

   示例：

   ```rust
   pub async fn send_text(
       client: &Arc<DartImClient>,
       request: DartSendTextRequest,
   ) -> Result<DartSendMessageResult, DartImError> {
       let inner = client.clone_inner()?;
       inner.messages().send(request.try_into()?).await.map(Into::into).map_err(DartImError::from)
   }
   ```

2. 不跨 `.await` 持有 `RwLock` / `Mutex` guard。

   使用：

   ```text
   clone_inner before await
   closed/object state check before/after await as needed
   ```

3. Dart public API 保持 Future/Stream。

4. Dart model 字段、错误 code 和 enum 语义不漂移。

5. Realtime event stream 映射到新的 `RealtimeSession`。

6. Dispose semantics 明确：

   ```text
   - object closed 后新操作返回 object_closed
   - dispose cancel/stop realtime session
   - running operation 不跨 await 持锁
   - 不等待无关 stale long-running operation，除非资源安全需要
   ```

## 执行步骤

1. 为 `DartImCore` / `DartImClient` 增加安全的 `clone_inner` 或等价 handle clone API。

2. 按 domain 将 bridge API 改为 async：

   ```text
   core/open/client
   auth/identity
   messages
   groups
   attachments
   secure
   realtime
   directory/profile/content/site/email
   ```

3. 更新 DTO mapping。

   只在 im-core DTO 真实变化时改 Dart DTO；async signature 本身不应改变 model 字段。

4. 更新 FRB generated bindings。

5. 更新 Dart facade。

6. 更新 tests：

   ```text
   open core
   create client
   current identity
   send text
   inbox/history
   groups basic read
   attachment send/download if available
   realtime start/stop event stream
   running operation during dispose
   object_closed after dispose
   ```

## 上层同步

本切片必须同步 Flutter package。

同步范围：

```text
packages/awiki_im_core/lib/src/models/**
packages/awiki_im_core/lib/src/awiki_im_core_native.dart
packages/awiki_im_core/lib/src/**
packages/awiki_im_core/test/**
```

如果 FRB generated 文件变化，必须用项目脚本生成，不手写 generated 文件。

## 测试

本切片必须运行：

```bash
cargo check -p im-core-dart --locked
cargo test -p im-core-dart --locked
scripts/flutter/codegen-check.sh
cd packages/awiki_im_core && dart analyze
cd packages/awiki_im_core && dart test
```

Lock guard review：

```bash
rg "RwLock|Mutex|read\\(|write\\(|lock\\(" crates/im-core-dart/src
rg "\\.await" crates/im-core-dart/src
```

这需要人工 review，不能只依赖 grep。

## 验收

```text
1. Bridge functions 直接 await im-core async API。
2. 不跨 await 持有 lock guard。
3. Dart public API 保持 Future/Stream 语义。
4. DTO/model/error mapping 无非必要漂移。
5. Realtime stream 和 dispose 语义可测试。
```

## 完成报告

报告必须包含：

```text
- async bridge API 列表
- clone_inner/dispose 策略
- generated bindings 更新命令
- Dart analyze/test 结果
- lock across await review 结果
```
