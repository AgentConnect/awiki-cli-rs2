# 切片 07：Groups 异步化

## 目标

将 group lifecycle、read、member、message 和 local cache flows 转换为 async。

本切片保持 group DTO、group policy、service DID 注入和 group message wire format 不变。

## 依赖

依赖切片：

```text
slice-02-async-http-transport.md
slice-03-identity-bootstrap-auth.md
slice-04-local-state-db-actor.md
slice-05-messages-async.md
slice-06-directory-profile-content.md
```

## 当前代码锚点

重点改造：

```text
crates/im-core/src/groups/**
crates/im-core/src/internal/group_runtime/**
crates/im-core/src/internal/wire/group.rs
crates/im-core/src/internal/message_runtime/group.rs
crates/im-core/src/internal/message_runtime/read.rs
crates/im-core/src/internal/group_e2ee/**
```

## 设计要求

1. GroupService async methods：

   ```text
   create
   join
   leave
   add_member
   remove_member
   update_profile
   update_policy
   update
   get
   list
   members
   messages
   ```

2. Group member handle resolution async。

3. Group local projection 使用 DB actor。

4. Group E2EE lifecycle paths async-compatible。

5. 保留 service DID 注入。

6. 保留 group policy DTO semantics。

## 执行步骤

1. 将 `GroupService` public I/O methods 改为 async。

2. 将 `internal/group_runtime/lifecycle.rs`、`read.rs`、`cache.rs`、`projection.rs` 中触达 session/transport/DB 的方法改为 async。

3. 将 group create/join/update/member operations 改走 async transport。

4. 将 group list/get/members/messages projection 改走 DB actor。

5. 将 group member handle resolution 改走 async DirectoryService。

6. 对 group E2EE lifecycle 保持 async-compatible：

   ```text
   - 不新增阻塞 DB 访问
   - 不跨 await 持锁
   - 不改变 unsupported/required 行为
   - 允许完整 E2EE session transaction 在切片 09 完成
   ```

7. 增加 group payload golden tests。

8. 增加 group projection tests。

## 上层同步

如果 `GroupService` public methods 改为 async，必须同步：

```text
crates/awiki-cli/src/m_core_cli_adapter/groups.rs
crates/awiki-cli/src/cli_shell/group_handlers.rs
crates/awiki-cli/src/cli_shell/group_e2ee_handlers.rs
crates/im-core-dart/src/api/groups.rs
packages/awiki_im_core/lib/src/**
```

如果 CLI/Dart 暂留到切片 11/12，必须记录 workspace 暂时失败原因。

## 测试

本切片必须运行：

```bash
cargo test -p im-core groups --locked
cargo test -p im-core group_lifecycle --locked
cargo test -p im-core group_contract --locked
cargo check -p im-core --locked
```

稳定性测试：

```text
- create request payload 保持不变
- service DID 缺失时返回 invalid_input，并带正确 field
- group-e2ee unsupported/required 行为保持不变
- group list/get/members/messages projection 保持不变
- owner/member role cache 保持不变
- incremental group message cursor 保持不变
```

## 验收

```text
1. GroupService I/O methods 是 async。
2. group wire payload 和 DTO 语义不变。
3. group local projection 使用 DB actor。
4. service DID 注入行为不变。
5. 上层调用者已同步或登记到切片 11/12。
```

## 完成报告

报告必须包含：

```text
- async 化的 group methods
- group payload golden tests 结果
- group projection 迁移状态
- group E2EE async-compatible 状态
- CLI/Dart 同步状态
```
