# awiki-cli 发布与回滚手册

本文档描述当前有效的发布方式：在本地或服务器构建 release 产物，然后由维护者把产物放到实际文件服务目录。

发布边界：

- `awiki-cli` 是 npm 包和单二进制命令行产品；`awiki-cli upgrade` 只检查/升级 CLI 自身。
- `awiki-deamon` 是 awiki-me 客户端安装到宿主机的 daemon 包；它使用 daemon manifest、install.sh 和客户端/daemon 升级路径，不通过 `awiki-cli runtime listener` 管理。
- `awiki-cli runtime listener` 是 CLI 的本机 WebSocket receiving helper/service，属于 CLI runtime UX，不是 daemon release 包。

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
- Linux release 使用 musl 静态目标；构建机必须提供 `musl-tools`。归档脚本会拒绝仍包含 GLIBC 版本符号的产物，避免下载机器受 GitHub runner 的 glibc 版本约束。

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
  --target x86_64-unknown-linux-musl
```

常用目标：

```bash
# Linux x86_64
scripts/release/build-release-artifact.sh \
  --version <version> \
  --os linux \
  --arch amd64 \
  --target x86_64-unknown-linux-musl

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

Linux/macOS 构建还会检查 E2EE feature graph，确认 `awiki-cli -> im-core/group-e2ee -> anp/mls` 已启用。Linux 构建还会检查最终二进制不包含 GLIBC 版本符号。

## 4. 发布 awiki-deamon 包

Daemon release 包用于 awiki-me 客户端安装/升级宿主机 daemon。当前统一在目标服务器上执行高层发布脚本，
由 GitHub Actions 构建 Linux amd64、macOS arm64 和 macOS amd64 三个平台包，
再发布到本机 Nginx daemon 静态目录：

```bash
scripts/release/daemon/publish-multi-platform.sh
```

脚本不接受参数，也不依赖外部环境变量。发布前复制并填写同级配置文件：

```bash
cp scripts/release/daemon/publish-multi-platform.toml.template \
  scripts/release/daemon/publish-multi-platform.toml
```

配置文件只保存发布环境和触发构建所需的信息：`base_url`、`download_base_url`、
`download_mirror_urls`、`source_ref` 和 `github_token`。
它不配置当前版本号或最低可用版本号。当前发布版本固定来自
`crates/awiki-deamon/Cargo.toml`，并由 `Cargo.lock` 做一致性校验；第一版发布流程中，
manifest 的 `min_supported` 自动等于当前 Daemon 版本。`base_url` 是后端服务/API 根地址；
`download_base_url` 是当前发布机器提供的 daemon 静态下载根地址，省略时默认使用
`<base_url>/daemon`；`download_mirror_urls` 是可选镜像下载源列表，只写入安装脚本，
发布脚本不会主动推送或校验这些镜像。
其中 `source_ref` 是实际要构建的源码 ref，可以是分支、tag 或 commit SHA。GitHub
`workflow_dispatch` 入口本身需要存在于仓库默认分支；发布脚本固定从默认分支触发 workflow，
再把 `source_ref` 传给 workflow checkout。

脚本行为：

- 从 `crates/awiki-deamon/Cargo.toml` 读取发布版本。
- 校验 `Cargo.lock` 中的 `awiki-deamon` 版本一致。
- 校验本次版本高于 Nginx daemon 静态目录中 `releases/manifest.json` 的 `latest`。
- 使用配置中的 GitHub token 触发 GitHub Actions，从配置中的 `source_ref` 构建三平台 release 包。
- 生成 `install.sh`、`releases/manifest.json` 和版本化 release 目录。
- 发布到本机 Nginx daemon 静态目录 `/var/www/awiki-web/daemon`。
- 通过 HTTP 校验 manifest、安装脚本和三个平台 tar 包可访问。

manifest 中的包条目只保存相对 `path` 和 `sha256`，不保存完整 URL。安装脚本会从
`download_base_url` 和 `download_mirror_urls` 中选择可用且较快的下载源，下载包后用
manifest 中的 `sha256` 校验；校验失败或下载失败会继续尝试下一个源。Daemon 自升级也按
持久化的 `download_base_url + package.path` 下载并校验。

脚本不会修改版本号、提交代码或推送代码。发布前需要先在
`crates/awiki-deamon/Cargo.toml` 中更新版本，并确保 `Cargo.lock` 已同步。

注意：daemon 发布和 CLI 发布是两条发布线。daemon manifest 的 `latest` / `min_supported`
只约束 awiki-me daemon 安装和升级；不会改变 `@awiki/cli` 的 npm 版本，也不会影响
`awiki-cli upgrade` 的行为。

## 5. 手工准备 daemon 下载目录

一般发布不需要手工执行底层脚本。只有在本地调试 release 包或下载目录结构时，才直接使用下面两个脚本。

先构建三个平台包：

```bash
scripts/release/daemon/_build-artifact.sh \
  --os linux \
  --arch amd64 \
  --target x86_64-unknown-linux-musl \
  --dist dist/daemon

scripts/release/daemon/_build-artifact.sh \
  --os darwin \
  --arch arm64 \
  --target aarch64-apple-darwin \
  --dist dist/daemon

scripts/release/daemon/_build-artifact.sh \
  --os darwin \
  --arch amd64 \
  --target x86_64-apple-darwin \
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

- `--download-base-url`：主静态下载根地址。安装脚本会优先从这里读取 `releases/manifest.json` 和 release tar 包。
- `--download-mirror-url`：可重复传入的镜像静态下载根地址。生成的安装脚本会把主源和镜像源一起作为候选下载源。
- `--base-url`：daemon 持久配置中的后端服务根地址，默认派生 user-service、message-service、mail-service、DID domain 和 ANP service。标准线上路由下可省略；如果下载域名和后端 API 域名不同，或者使用 `file://` / 本地路径测试，则必须显式传入。

manifest 的包条目保存相对路径：

```json
{
  "version": "1.2.3",
  "os": "darwin",
  "arch": "arm64",
  "path": "releases/1.2.3/awiki-deamon-darwin-arm64.tar.gz",
  "sha256": "..."
}
```

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

如果需要把多个下载源写入安装脚本：

```bash
scripts/release/daemon/_stage-downloads.sh \
  --version <version> \
  --source-dir dist/daemon \
  --output-dir dist/daemon-downloads \
  --base-url https://api.example.com \
  --download-base-url https://primary.example.com/daemon \
  --download-mirror-url https://mirror-a.example.com/daemon \
  --download-mirror-url https://mirror-b.example.com/daemon
```

输出目录结构：

```text
dist/daemon-downloads/
  install.sh
  releases/
    manifest.json
    <version>/
      awiki-deamon-linux-amd64.tar.gz
      awiki-deamon-darwin-arm64.tar.gz
      awiki-deamon-darwin-amd64.tar.gz
      checksums.txt
```

把 `dist/daemon-downloads/` 发布到文件服务的 `/daemon` 路径后，安装入口就是：

```bash
curl -fsSL https://example.com/daemon/install.sh | sh -s -- --token <install-token>
```

如果当前阶段只做本地联调，也可以直接使用 tar 包或 `file://` 方式验证安装脚本，不需要公网 CDN。

## 6. 同步 daemon 下载镜像

镜像服务器不需要主发布服务器的 SSH 权限。每个镜像节点只需要能通过 HTTP(S) 访问主下载源，
然后在镜像节点本机执行同步脚本：

```bash
cp scripts/release/daemon/sync-download-mirror.toml.template \
  scripts/release/daemon/sync-download-mirror.toml
```

配置示例：

```toml
source_base_url = "https://anpclaw.com/daemon"
target_dir = "/var/www/awiki-web/daemon"
keep_versions = "3"
```

执行：

```bash
scripts/release/daemon/sync-download-mirror.sh
```

同步脚本不接受命令行参数，所有配置都来自 `sync-download-mirror.toml`。它会拉取主源的
`install.sh`、`releases/manifest.json` 和 manifest 中列出的 release 包，逐个校验
`sha256`，校验通过后再写入目标静态目录。`manifest.json` 最后替换，避免用户读到半同步状态。
`keep_versions` 用于清理未被当前 manifest 引用的旧版本目录，当前 `latest`、`min_supported`
和 manifest 中的 package 版本总会保留。

## 7. 建议发布检查

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

## 8. 回滚

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
