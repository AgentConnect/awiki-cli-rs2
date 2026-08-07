# 参与贡献 AWiki Client Workspace

[English](CONTRIBUTING.md) | [简体中文](CONTRIBUTING.zh-CN.md)

本仓库包含 CLI、共享 Rust SDK、AWiki Daemon、Dart/Flutter SDK 与 Agent Skills。提交应明确影响哪一个产品面，并保持共享边界一致。

## 开始之前

- 搜索现有 Issue/PR；
- 外部贡献仅在事先获得许可方书面批准后接受；经批准的贡献者接受许可方指定的
  贡献者协议之前，贡献不得合并；
- 大型 API、命令树、SecretVault、E2EE、Runtime plugin 或发布流程变化先开 Issue；
- 不在同一 PR 混入不相关 crate 重构、文档搬迁和发布配置；
- 跨 `awiki-me`、`awiki-open-server` 或 ANP 的变化应记录匹配 commit 和兼容性验证。

## Rust Gate

```bash
cargo fmt --all --check
cargo check --workspace --locked
cargo test --workspace --locked
cargo run -p xtask -- check-structure
cargo run -p xtask -- check-version
```

## CLI

```bash
cargo run -p awiki-cli -- --help
cargo run -p awiki-cli -- version
```

修改命令、flag 或输出时，同步：

- schema；
- docs；
- output contract；
- tests；
- Skill references；
- onboarding。

不要让 Skill 猜测不存在的命令。

## AWiki Daemon

```bash
cargo test -p awiki-deamon --locked
```

Runtime plugin、controller scope、local RPC、token、session/audit 和 service management 变化需要专项安全评审。

## Flutter/Dart SDK

```bash
scripts/flutter/codegen-check.sh
scripts/flutter/build-sdk-native.sh --linux-only
cd packages/awiki_im_core
flutter test
```

按实际平台补充 native build。SDK DTO 不应包含 AWiki Me 的 UI/cache/presentation 字段。

## 架构规则

- `awiki-im-core` 是共享产品 SDK；
- CLI 是薄壳；
- AWiki Daemon 管理 Runtime Host 边界；
- Flutter SDK 暴露 core-owned DTO 和高层 API；
- Skill 只做任务路由、安全与按需加载；
- 不在 host 层重建 raw RPC、WebSocket、DID proof、本地投影和 E2EE internals。

## 安全

禁止提交或输出：

- DID private key、root key、JWT、bearer token；
- Direct/Group E2EE 私有状态、KeyPackage、prekey、ciphertext；
- Runtime RPC token、registration token；
- `publish-server.toml`、GitHub token、真实服务器路径；
- 用户 workspace、SQLite、身份目录、日志和真实消息；
- 未脱敏测试 artifact。

详见 [SECURITY.md](SECURITY.zh-CN.md)。

## PR 描述

至少说明：

```text
Affected component(s)
User / Agent impact
Command or API contract changes
Security boundary changes
Compatibility impact
Tests run
Release or migration implications
```
