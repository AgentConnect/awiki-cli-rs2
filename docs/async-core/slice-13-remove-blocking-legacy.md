# 切片 13：移除阻塞 Legacy 和最终门禁

## 目标

移除生产阻塞实现、临时 legacy oracle 和过渡 compat 路径，执行最终架构 review、依赖兼容性检查和系统测试门禁。

本切片是异步切换完成的收口切片。不能用“局部测试通过”替代最终全量验收。

## 依赖

依赖切片：

```text
slice-11-cli-async-host.md
slice-12-frb-dart-async.md
```

并要求切片 01-10 的功能迁移均已完成。

## 修改范围

允许修改：

```text
crates/im-core/**
crates/awiki-cli/**
crates/im-core-dart/**
packages/awiki_im_core/**
scripts/**
docs/**
Cargo.toml / Cargo.lock
CI 配置
```

但只允许做最终清理、门禁接入、文档更新和必要修复，不做新的协议或 DTO 重设计。

## 执行步骤

1. 移除 production blocking feature。

   检查：

   ```text
   crates/im-core/Cargo.toml default features
   cfg(feature = "blocking")
   legacy sync transport traits
   legacy sync HTTP client
   legacy thread/mpsc realtime runner
   legacy sync service method variants
   ```

2. 保留 test-only compat 时必须有明确 cfg。

   可接受：

   ```text
   #[cfg(test)]
   #[cfg(feature = "internal-test-helpers")]
   ```

   不可接受：

   ```text
   production default path 静默调用 blocking implementation
   ```

3. 清理 docs/examples。

   所有示例应使用 async API。

4. 更新 CI gates。

   至少加入：

   ```text
   workspace check/test
   grep fences
   dependency compatibility checks
   Flutter codegen check
   non-email system test gate（可在 release gate 或 manual gate）
   ```

5. 执行架构兼容性 review。

6. 执行 Rust 依赖兼容性检查。

7. 执行全量 Rust/Dart/Flutter tests。

8. 执行 `../awiki-system-test` 非 email 系统测试。

## 架构兼容性 review

必须逐项确认：

```text
im-core:
  - 仍是 IM 核心 SDK
  - 不包含 CLI 专属输出 DTO
  - public DTO 语义稳定
  - wire builder 保持协议语义
  - public API 不暴露低层 async traits
  - service getter 保持纯内存同步
  - I/O service methods 是 async

awiki-cli:
  - CLI parser/render/JSON output 留在 CLI crate
  - CLI 通过 im-core public API 访问 IM core
  - CLI JSON output shape 兼容

im-core-dart / packages/awiki_im_core:
  - FRB DTO mapping 与 im-core DTO 对齐
  - Dart public API 保持 Future/Stream
  - dispose/object_closed 语义明确

local state:
  - rusqlite 只在 LocalStateDbActor 内部直接使用
  - schema/migration/projection 语义未被重写
  - owner_identity_id / owner_did isolation 保持

runtime:
  - HTTP/WebSocket/attachment transfer 是真正 async
  - blocking work 被隔离到 DB actor 或 worker
  - 不跨 await 持有锁
```

## 依赖兼容性检查

执行：

```bash
cargo tree --workspace --locked | rg -i "openssl|openssl-sys|native-tls"
cargo tree --workspace --locked | rg -i "security-framework|schannel"
cargo tree --workspace --locked | rg -i "rusqlite|libsqlite3-sys"
```

预期：

```text
- 不出现 openssl / openssl-sys / native-tls。
- 如出现 security-framework / schannel，必须确认不是默认 TLS 分发路径。
- rusqlite 继续使用 bundled SQLite。
```

## 最终测试

必须运行并通过：

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo check --workspace --locked
cargo test --workspace --locked
cargo test -p im-core --locked
cargo test -p awiki-cli --locked
cargo test -p im-core-dart --locked
scripts/flutter/codegen-check.sh
cd packages/awiki_im_core && dart analyze && dart test
```

Grep 围栏：

```bash
rg "std::net::TcpStream|std::thread::spawn|std::sync::mpsc" crates/im-core/src
rg "StreamOwned|std::io::Read|std::io::Write" crates/im-core/src/internal
rg "std::fs::read|std::fs::write|std::fs::File" crates/im-core/src
rg "rusqlite::Connection|Connection::open|open_writable" crates/im-core/src
rg "pub trait .*async|async fn" crates/im-core/src
rg "diagnostic_raw|raw_response|compat::|crate::internal" crates/im-core/src/prelude.rs crates/im-core/src/lib.rs packages/awiki_im_core/lib
```

Grep 可以有例外，但每个例外必须记录：

```text
- path
- 为什么存在
- 是否 production path
- 是否 test-only
- 是否需要后续 issue
```

## 系统测试门禁

最终必须通过 `../awiki-system-test` 的非 email 系统测试。

推荐命令：

```bash
cd ../awiki-system-test
uv run awiki-system-test tests tests_v2 --ignore=tests_v2/mail
```

报告必须遵守 `../awiki-system-test/AGENTS.md` 的系统测试报告规则：

```text
- 总体结果：通过、失败、跳过、耗时、实际命令
- 失败用例：文件/用例名、功能域、数量、原因
- 跳过用例：文件/用例名或 pytest summary、功能域、数量、原因
- 配置上下文：AWIKI_SYSTEM_TEST_MODE、user-service URL、message-service URL、WebSocket URL、DID domain
- 失败 0 或明确失败列表
- 跳过 0 或明确跳过列表
```

Email 用例不属于本计划最终强制门禁，但不得破坏非 email 用例。

## 验收

```text
1. production blocking implementation 已移除或 test-only。
2. 所有 public business APIs async-first。
3. CLI async host 通过测试。
4. FRB/Dart async bridge 通过测试。
5. HTTP/WebSocket/attachment transfer 是真正 async。
6. SQLite 访问隔离到 DB actor。
7. E2EE 磁盘优先且事务安全。
8. 架构兼容性 review 通过。
9. 没有 OpenSSL/native-tls 等不期望系统库依赖。
10. Rust/Dart/Flutter 全量测试通过。
11. awiki-system-test 非 email 用例全部通过。
```

## 完成报告

报告必须包含：

```text
- legacy blocking 清理清单
- test-only 例外清单
- 架构兼容性 review 结论
- 依赖兼容性检查输出摘要
- Rust/Dart/Flutter 全量测试结果
- Grep 围栏结果和例外说明
- awiki-system-test 非 email 测试详细报告
```
