# 切片 00：基线和文档

## 目标

建立异步切换前的事实基线，补齐总计划和切片计划，并记录当前已知测试状态。

本切片不修改生产代码行为。它的作用是让后续每个切片可以清楚回答三个问题：

```text
当前行为是什么？
本切片改了什么？
哪些失败是既有问题，哪些失败是本切片引入的问题？
```

## 修改范围

允许修改：

```text
docs/async-core/**
docs/file-size-exceptions.md（仅当后续切片需要预先登记文档约定时）
```

不允许修改：

```text
crates/**
packages/**
scripts/**
Cargo.toml / Cargo.lock
```

## 执行步骤

1. 确认总计划存在：

   ```text
   docs/async-core/full-async-cutover-plan.md
   ```

2. 创建每个切片的独立落地计划：

   ```text
   docs/async-core/slice-00-baseline-and-docs.md
   docs/async-core/slice-01-runtime-foundation.md
   docs/async-core/slice-02-async-http-transport.md
   docs/async-core/slice-03-identity-bootstrap-auth.md
   docs/async-core/slice-04-local-state-db-actor.md
   docs/async-core/slice-05-messages-async.md
   docs/async-core/slice-06-directory-profile-content.md
   docs/async-core/slice-07-groups-async.md
   docs/async-core/slice-08-attachments-streaming.md
   docs/async-core/slice-09-e2ee-secure-async.md
   docs/async-core/slice-10-realtime-runner-async.md
   docs/async-core/slice-11-cli-async-host.md
   docs/async-core/slice-12-frb-dart-async.md
   docs/async-core/slice-13-remove-blocking-legacy.md
   ```

3. 记录当前 public API 和行为锚点：

   ```text
   - im-core public service methods
   - im-core public DTO exports
   - JSON-RPC method names and endpoint paths
   - CLI JSON output contracts
   - im-core-dart API functions and Dart model names
   - current local_state schema version and migration status
   - current realtime event DTO shape
   ```

4. 记录当前测试基线。命令可以失败，但必须记录失败范围和失败原因：

   ```bash
   cargo test -p im-core --locked
   cargo test -p awiki-cli --locked
   cargo check -p im-core-dart --locked
   cargo check --workspace --locked
   ```

5. 记录当前 grep 基线：

   ```bash
   rg "std::net::TcpStream|std::thread::spawn|std::sync::mpsc" crates/im-core/src
   rg "StreamOwned|std::io::Read|std::io::Write" crates/im-core/src/internal
   rg "std::fs::read|std::fs::write|std::fs::File" crates/im-core/src
   rg "rusqlite::Connection|Connection::open|open_writable" crates/im-core/src
   ```

## 验收

本切片完成时必须满足：

```text
1. 总计划引用所有切片文档。
2. 每个切片文档存在，并说明目标、执行步骤、测试策略和上层同步要求。
3. 基线测试结果已记录。
4. 没有生产代码行为变更。
5. 没有将“重写 SDK”作为实现目标。
```

## 测试策略

本切片只修改文档，不要求全量测试通过。

建议运行：

```bash
find docs/async-core -maxdepth 1 -type f | sort
perl -0ne 'print "$ARGV\n" if /重写\s+`im-core`\s+时|以重写为目[标]|作为重写计[划]|rewrite[-]first/' docs/async-core/*.md
rg "slice-[0-9][0-9].*\\.md" docs/async-core/full-async-cutover-plan.md
```

预期：

```text
- 文档文件完整。
- 不存在把重写 SDK 作为执行目标的措辞。
- 总计划中的切片链接覆盖全部切片。
```

## 完成报告

报告必须包含：

```text
- 新增/修改的文档列表
- 当前测试基线命令及结果
- 已知失败测试清单
- 下一切片入口
```
