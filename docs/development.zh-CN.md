# AWiki Client Workspace 开发指南

[English](development.md) | [简体中文](development.zh-CN.md)

## 1. 环境

- Rust：根 `rust-toolchain.toml`，最低 1.88；
- Node.js 18+；
- sibling ANP Rust SDK：`../anp/anp/rust`；
- Flutter/Dart：仅修改 FFI 或 Flutter SDK 时需要；
- bundled SQLite：CLI/Core 通常不依赖系统 SQLite。

```bash
rustc --version
cargo --version
node --version
ls ../anp/anp/rust/Cargo.toml
```

## 2. Rust Gate

```bash
cargo fmt --all --check
cargo check --workspace --locked
cargo test --workspace --locked
cargo run -p xtask -- check-structure
cargo run -p xtask -- check-version
```

CLI 快速运行：

```bash
cargo run -p awiki-cli -- --help
cargo run -p awiki-cli -- version
```

Daemon：

```bash
cargo test -p awiki-deamon --locked
cargo run -p awiki-deamon -- status --state-root /tmp/awiki-deamon-state
```

## 3. Flutter SDK

```bash
scripts/flutter/codegen-check.sh
scripts/flutter/build-sdk-native.sh --linux-only
cd packages/awiki_im_core
flutter test
```

按平台选择：

```bash
scripts/flutter/build-sdk-native.sh --macos-only
scripts/flutter/build-sdk-native.sh --ios-only
scripts/flutter/build-sdk-native.sh --android-only
```

以上命令默认仍生成对应平台的完整产物。明确只发布单一架构时，可以追加
`--macos-arch arm64|x86_64` 或 `--android-abi arm64-v8a`，避免编译不会进入
最终安装包的原生架构。

生成 native artifact 不应在没有明确发布策略时随意提交。

## 4. Workspace 本地状态

默认：

```text
~/.awiki-cli/
```

隔离测试：

```bash
export AWIKI_CLI_WORKSPACE_HOME_DIR=/tmp/awiki-cli-test
cargo run -p awiki-cli -- init
```

CLI 以租户隔离 backend、DID host、identity、SQLite、runtime 和 logs。不要直接拼 owner query，也不要跨租户复用私有状态。

## 5. 租户

```bash
awiki-cli tenant list
awiki-cli tenant current
awiki-cli tenant setup acme \
  --backend-base-url https://api.acme.example \
  --did-host acme.example
awiki-cli tenant use acme
```

`tenant setup` 是幂等 onboarding 入口；配置冲突时拒绝覆盖。已有身份或数据库数据的租户不能随意 reconfigure，应创建新租户。

## 6. CLI 输出契约

- Canonical output：JSON；
- `pretty/table/ndjson` 是视图；
- 错误按稳定 `error.code`、`hint`、`retryable`；
- exit code 与 envelope 业务结果一致；
- 写操作应支持 dry-run；
- 输出中不得包含 secret material。

更改 envelope 时必须同步：

- contract tests；
- Skill reference；
- docs；
- Agent integration；
- examples。

## 7. 发布

CLI artifact：

```bash
scripts/release/build-release-artifact.sh --os linux --arch amd64
scripts/release/build-release-artifact.sh --os darwin --arch arm64
```

自托管 channel：

```text
scripts/release/cli/
```

发布顺序：

1. 干净、已推送的发布分支准备 beta tag；
2. 服务器发布 beta；
3. 验证平台、Skill、Onboarding 与更新；
4. 同一 commit 准备 stable；
5. 发布 stable；
6. 不移动既有 tag，不把旧 artifact 重新提升为 latest。

Daemon 发布脚本：

```bash
scripts/release/daemon/publish-multi-platform.sh
```

真实服务器配置、GitHub token 和路径只能放在 ignored 配置中。

## 8. 开发规则

- 保持 `awiki-im-core` 为共享产品 SDK；
- 保持 CLI 为薄壳；
- 保持 Daemon 负责 Runtime plugin、RPC token、Agent DID 与 audit state；
- 保持 Dart SDK DTO core-owned，不加入 AWiki Me presentation fields；
- 高风险输出保留 DID/handle 事实，不用 display name 代替路由或授权身份；
- 不记录 root key、private key、JWT、E2EE state、registration/runtime token。

## 9. PR 前检查

- [ ] Rust Gate 通过；
- [ ] 结构与版本一致；
- [ ] 行为变化有测试；
- [ ] CLI schema/docs 与实现一致；
- [ ] Skill 未描述不存在的 command/flag；
- [ ] 无真实 workspace、token、private key 或发布配置；
- [ ] 影响 README 的命令、状态和兼容性已同步；
- [ ] 跨仓库变更记录了匹配 commit。
