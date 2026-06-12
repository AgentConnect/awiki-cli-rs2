# awiki-cli 发布与回滚手册

本文档描述当前有效的发布方式：在本地或服务器构建 release 产物，然后由维护者把产物放到实际文件服务目录。

## 1. 版本号约定

- 公开版本真相是仓库根目录的 `package.json.version`。
- `package.json.awikiCli.minSupportedVersion` 当前必须与 `package.json.version` 一致。
- `crates/awiki-cli/Cargo.toml` 的 `[package].version` 必须与 `package.json.version` 一致。
- `Cargo.lock` 中的 `awiki-cli` package 版本必须同步。
- Daemon 版本来自 `crates/awiki-deamon/Cargo.toml`。

修改版本后运行：

```bash
cargo run -p xtask -- check-version
```

构建 CLI release artifact 时，`scripts/release/build-release-artifact.sh` 还会运行：

```bash
cargo run -p xtask -- check-version --expect <version>
```

## 2. 前置环境

在构建机器上准备：

- Rust toolchain：仓库根目录 `rust-toolchain.toml` 固定版本，当前发布脚本默认使用 `1.88.0`。
- Node.js 18+：用于读取 `package.json` 和生成 daemon manifest。
- 同级 ANP Rust SDK：`../anp/anp/rust/Cargo.toml` 必须存在。
- Linux release 建议在 Ubuntu 或兼容 Linux build 机上构建，避免 macOS 交叉编译 Linux 目标带来的 linker 和 libc 差异。

检查：

```bash
rustc --version
cargo --version
node --version
ls ../anp/anp/rust/Cargo.toml
```

## 3. 构建 awiki-cli 产物

在 `awiki-cli-rs2` 仓库根目录执行：

```bash
scripts/release/build-release-artifact.sh \
  --version <version> \
  --os linux \
  --arch amd64 \
  --target x86_64-unknown-linux-gnu
```

常用目标：

```bash
# Linux x86_64
scripts/release/build-release-artifact.sh \
  --version <version> \
  --os linux \
  --arch amd64 \
  --target x86_64-unknown-linux-gnu

# macOS arm64
scripts/release/build-release-artifact.sh \
  --version <version> \
  --os darwin \
  --arch arm64 \
  --target aarch64-apple-darwin
```

产物默认写入 `dist/`，命名格式：

```text
awiki-cli-<version>-<os>-<arch>.tar.gz
awiki-cli-<version>-windows-<arch>.zip
```

脚本会注入构建信息：

- `AWIKI_CLI_VERSION`
- `AWIKI_CLI_COMMIT`
- `AWIKI_CLI_BUILD_DATE`

Linux/macOS 构建还会检查 E2EE feature graph，确认 `awiki-cli -> im-core/group-e2ee -> anp/mls` 已启用。

## 4. 发布 awiki-deamon Linux 包

Daemon release 包用于客户端安装/升级。当前推荐在目标 Ubuntu 服务器上执行高层发布脚本：

```bash
scripts/release/daemon/publish-linux.sh --base-url https://example.com
```

其中 `--base-url` 是目标环境的后端服务根地址。标准路由下，Daemon 下载根地址固定派生为 `<base-url>/daemon`。

脚本行为：

- 从 `crates/awiki-deamon/Cargo.toml` 读取发布版本。
- 校验 `Cargo.lock` 中的 `awiki-deamon` 版本一致。
- 校验本次版本高于 Nginx daemon 静态目录中 `releases/manifest.json` 的 `latest`。
- 构建 Linux amd64 release 包。
- 生成 `install.sh`、`releases/manifest.json` 和版本化 release 目录。
- 发布到 Nginx daemon 静态目录，默认 `/var/www/awiki-web/daemon`，可用 `AWIKI_DAEMON_NGINX_DIR` 覆盖。
- 通过 HTTP 校验 manifest、安装脚本和 tar 包可访问。

脚本不会修改版本号、提交代码、推送代码或执行测试。发布前需要先在 `crates/awiki-deamon/Cargo.toml` 中更新版本，并确保 `Cargo.lock` 已同步。只检查发布计划时使用：

```bash
scripts/release/daemon/publish-linux.sh --base-url https://example.com --dry-run
```

Daemon 发布脚本、内部 helper 和 Nginx 配置要求见 `scripts/release/daemon/README.md`。

## 5. 手工准备 daemon 下载目录

一般发布不需要手工执行底层脚本。只有在本地调试 release 包或下载目录结构时，才直接使用下面两个脚本。

先构建 Linux 包：

```bash
scripts/release/daemon/_build-artifact.sh \
  --os linux \
  --arch amd64 \
  --target x86_64-unknown-linux-gnu \
  --dist dist/daemon
```

再生成安装脚本、manifest 和版本化下载目录：

```bash
scripts/release/daemon/_stage-downloads.sh \
  --version <version> \
  --source-dir dist/daemon \
  --output-dir dist/daemon-downloads \
  --download-base-url https://example.com/daemon
```

参数含义：

- `--download-base-url`：安装脚本、manifest 和 release tar 包的静态下载根地址。脚本会从这里读取 `releases/manifest.json`，manifest 中的包 URL 也会从这里派生。标准线上路由必须使用 `<后端服务根地址>/daemon`，发布脚本会据此推导 daemon 持久配置中的后端服务根地址。
- `--base-url`：daemon 持久配置中的后端服务根地址，默认派生 user-service、message-service、mail-service、DID domain 和 ANP service。标准线上路由下可省略；如果下载域名和后端 API 域名不同，或者使用 `file://` / 本地路径测试，则必须显式传入。

标准域名手工 staging 使用：

```bash
scripts/release/daemon/_stage-downloads.sh \
  --version <version> \
  --source-dir dist/daemon \
  --output-dir dist/daemon-downloads \
  --download-base-url https://example.com/daemon
```

如果后续采用 CDN/API 分离部署，则同时传两个 URL，避免发布脚本按 `/daemon` 路由推导：

```bash
scripts/release/daemon/_stage-downloads.sh \
  --version <version> \
  --source-dir dist/daemon \
  --output-dir dist/daemon-downloads \
  --base-url https://api.example.com \
  --download-base-url https://cdn.example.com/daemon
```

输出目录结构：

```text
dist/daemon-downloads/
  install.sh
  releases/
    manifest.json
    <version>/
      awiki-deamon-linux-amd64.tar.gz
      checksums.txt
```

把 `dist/daemon-downloads/` 发布到文件服务的 `/daemon` 路径后，安装入口就是：

```bash
curl -fsSL https://example.com/daemon/install.sh | sh -s -- --token <install-token>
```

如果当前阶段只做本地联调，也可以直接使用 tar 包或 `file://` 方式验证安装脚本，不需要公网 CDN。

## 6. 建议发布检查

发布前至少执行：

```bash
cargo fmt --all --check
cargo test -p awiki-cli --locked
cargo test -p awiki-deamon --locked
python3 scripts/test_daemon_release_contract.py
```

如果修改了 Flutter SDK：

```bash
scripts/flutter/build-sdk-native.sh
```

该脚本依次执行 bridge 生成一致性检查、Apple XCFramework 构建和 Android jniLibs 构建。只需要检查执行计划时可使用 `--dry-run`。

如果只发布 daemon，可以至少执行：

```bash
cargo fmt --all --check
cargo test -p awiki-deamon --locked
python3 scripts/test_daemon_release_contract.py
```

## 7. 回滚

文件服务发布采用目录和 manifest 管理。回滚时按实际发布目录操作：

1. 保留旧版本 tar 包和 `checksums.txt`。
2. 将 `releases/manifest.json` 指回上一个可用版本。
3. 如已下发坏版本，删除或隔离坏版本目录，避免新客户端继续下载。
4. 如版本策略依赖 `package.json.awikiCli.minSupportedVersion` 或服务端配置，同步回调到可用版本范围。

回滚后重新执行安装命令，确认新安装拿到的是预期版本：

```bash
curl -fsSL https://example.com/daemon/install.sh | sh -s -- --token <install-token>
awiki-deamon --version
```
