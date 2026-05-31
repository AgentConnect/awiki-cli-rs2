# 切片 06：Directory、Profile、Relationships、Content/Site/Email 异步化

## 目标

将 user/profile/directory/relationships 相关 service 以及 content/site/email 中触达 I/O 的方法转换为 async。

本切片保持 handle、DID、profile、relationship、content/site/email DTO 语义不变。

## 依赖

依赖切片：

```text
slice-02-async-http-transport.md
slice-03-identity-bootstrap-auth.md
slice-04-local-state-db-actor.md
```

## 当前代码锚点

重点改造：

```text
crates/im-core/src/directory/**
crates/im-core/src/identity/**
crates/im-core/src/content/**
crates/im-core/src/site/**
crates/im-core/src/email/**
crates/im-core/src/internal/directory_runtime.rs
crates/im-core/src/internal/profile_runtime.rs
crates/im-core/src/internal/relationship_runtime.rs
crates/im-core/src/internal/contact_store/**
crates/im-core/src/internal/identity_wire/**
crates/im-core/src/internal/email_runtime/**
```

## 设计要求

1. DirectoryService async methods：

   ```text
   resolve_peer
   lookup_handle
   public_profile
   save_contact
   contacts
   relation_status
   follow
   unfollow
   relationship_status
   followers
   following
   ```

2. IdentityService async methods：

   ```text
   profile
   update_profile
   bind_contact
   bind_email_status
   replace_did_plan（如保留）
   ```

3. Content/Site/Email methods 触达 network/file/DB 的路径 async 化。

4. Contact local projection 使用 DB actor。

5. 默认不使用本地陈旧缓存替代 server resolve，除非 API 明确是 local cache 查询。

## 执行步骤

1. 将 directory/profile/relationship runtime 中的 transport 调用改为 async。

2. 将 DirectoryService 和 IdentityService 中触达 I/O 的 public methods 改为 async。

3. 将 contact projection 从直接 SQLite 改为 DB actor。

4. 将 content/site/email runtime 中的 network 和 local cache path async 化。

5. 保留 wire builder。

   不更改：

   ```text
   handle lookup params
   profile update payload
   follow/unfollow payload
   site/content endpoint path
   email DTO 和 mailbox semantics
   ```

6. 增加 fake transport tests 和 payload golden tests。

7. 确认错误映射不漂移。

## 上层同步

如果 public methods 改为 async，必须同步：

```text
crates/awiki-cli/src/m_core_cli_adapter/people.rs
crates/awiki-cli/src/m_core_cli_adapter/identity.rs
crates/awiki-cli/src/m_core_cli_adapter/content.rs
crates/awiki-cli/src/m_core_cli_adapter/site.rs
crates/awiki-cli/src/m_core_cli_adapter/email.rs
crates/im-core-dart/src/api/*
packages/awiki_im_core/lib/src/**
```

Email 上层同步不能破坏非 email 系统测试门禁。Email 测试可以单独作为可选门禁。

## 测试

本切片必须运行：

```bash
cargo test -p im-core directory --locked
cargo test -p im-core profile --locked
cargo test -p im-core relationships --locked
cargo check -p im-core --locked
```

如修改 content/site/email：

```bash
cargo test -p im-core content --locked
cargo test -p im-core site --locked
cargo test -p im-core email --locked
```

## 验收

```text
1. Directory/Profile/Relationships I/O methods 是 async。
2. Contact projection 使用 DB actor。
3. handle/DID/profile/relationship DTO 语义不变。
4. Content/Site/Email 若被修改，上层同步或登记到切片 11/12。
5. 非 email 功能不依赖 email 测试通过。
```

## 完成报告

报告必须包含：

```text
- async 化的方法列表
- content/site/email 是否修改
- contact projection 迁移状态
- payload/contract tests 结果
- CLI/Dart 同步状态
```
