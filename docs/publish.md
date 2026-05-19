# awiki-cli 发布与回滚手册

本文档描述 awiki-cli 的发布主链路、预发布/回滚脚本，以及在出现坏版本时的处理建议。目标是让日常发版在几条标准命令内完成，并且可以安全地撤回。

当前发布只使用以下脚本：

- `scripts/release/release-one-click.sh`
- `scripts/release/build-release-artifact.sh`
- `scripts/release/release-tag-stable.sh`
- `scripts/release/release-tag-prerelease.sh`
- `scripts/release/publish-gitee-release.sh`

## 1. 版本号与 Tag 约定

- 单一公开版本真相：仓库根目录的 `package.json.version`，npm 包名为 `@awiki/cli`。
- Rust crate 版本：`crates/awiki-cli/Cargo.toml` 的 `[package].version` 必须与 `package.json.version` 一致，`Cargo.lock` 中的 `awiki-cli` package 版本也必须同步。
- npm 支持策略：`package.json.awikiCli.minSupportedVersion` 当前必须与 `package.json.version` 一致；如果未来引入兼容矩阵，需要先调整 `xtask check-version` 规则。
- Git Tag 规则：
  - 正式版：`vX.Y.Z`（例如 `v0.1.0`）。
  - 预发布版：`vX.Y.Z-<pre>`（例如 `v0.2.0-beta.1` / `v0.2.0-rc.1`）。
- Rust buildinfo 注入：release/build 脚本通过 `AWIKI_CLI_VERSION`、`AWIKI_CLI_COMMIT`、`AWIKI_CLI_BUILD_DATE` 注入构建信息；未注入 `AWIKI_CLI_VERSION` 的本地测试/开发构建显示为 `dev`，以保留 dev-build update 策略。
- 一致性检查：`cargo run -p xtask -- check-version` 必须通过，确认 package version、minSupportedVersion、crate version 三者一致。release artifact 构建还会运行 `cargo run -p xtask -- check-version --expect "${VERSION}"`，确认 tag/build 参数与版本真相一致。

**注意**：推荐用 `scripts/release/release-one-click.sh <version>` 修改版本号；它会同步 `package.json.version`、`package.json.awikiCli.minSupportedVersion`、`crates/awiki-cli/Cargo.toml` 和 `Cargo.lock`。如果手动修改版本号，也必须同步这四处并提交到当前分支。任何 Tag 都必须是 `v${package.json.version}`。

## 2. 正式发布（stable）

### 2.1 前置检查

1. 确保当前分支已经包含所有要发布的改动，并推送到远端：

   ```bash
   git status
   git push
   ```

2. 确认目标版本为标准 semver（不带 `-beta` / `-rc` 等）。推荐直接使用一键脚本：

   ```bash
   scripts/release/release-one-click.sh X.Y.Z --channel stable
   ```

   如果只想预览本地版本文件修改，可以先运行：

   ```bash
   scripts/release/release-one-click.sh X.Y.Z --channel stable --no-commit
   ```

3. 如果走手动版本修改和 tag 路径，必须先运行：

   ```bash
   cargo run -p xtask -- check-version
   ```

4. 在 GitHub 仓库的 workflow secrets 中配置 npm 凭据：

   - `NPM_TOKEN`：具有发布 `@awiki/cli` 的权限。

5. 添加 secrets 的位置：

   - 打开 GitHub 仓库页面。
   - 进入 `Settings`。
   - 进入 `Secrets and variables` -> `Actions`。
   - 点击 `New repository secret`，创建 `NPM_TOKEN`。

### 2.2 创建并推送 Tag

推荐使用 `scripts/release/release-one-click.sh` 完成版本更新、测试、提交、推送、tag、GitHub Release 等待、Gitee 同步和 npm 发布/校验。它会在提交前运行 `xtask check-version --expect`，并在 commit 中同时包含 npm 与 Cargo 版本文件。

低层 tag 脚本仍可用于手动流程。在 awiki-cli 仓库根目录执行：

```bash
scripts/release/release-tag-stable.sh
```

该脚本会：

- 从 `package.json.version` 读取版本，生成 `vX.Y.Z`；
- 要求工作区干净、当前分支已设置 upstream 且完全 push；
- 检查本地和远端是否已有同名 Tag；
- 创建 `vX.Y.Z` 的 annotated tag 并 push 到 origin。

这是正式版的低层 tag 入口；使用它之前必须已经提交并推送同步后的 `package.json`、`crates/awiki-cli/Cargo.toml` 和 `Cargo.lock`。

### 2.3 CI 行为

推送 `vX.Y.Z` Tag 后，`.github/workflows/release.yml` 会自动执行：

1. 使用 Rust release 构建脚本构建多平台二进制，并创建 GitHub Release；
2. 对稳定 Tag（`vX.Y.Z` 且不包含 `-`）执行一次 npm 发布：

   ```bash
   npm publish --access public
   ```

发布完成后可以做一个最小自检：

```bash
npm view @awiki/cli version
```

确认 registry 上的版本号与刚刚发布的一致。

### 2.4 在本地同步 Gitee Release

> Gitee Release 产物同步不再放在 GitHub hosted runner 上执行，避免跨境上传导致的长时间阻塞。
> 推荐在你自己的 Mac 或国内网络环境更稳定的机器上执行以下脚本。

先准备本地环境变量：

```bash
export GITEE_USERNAME=<你的 Gitee 登录用户名>
export GITEE_TOKEN=<你的 Gitee 个人访问令牌>
```

然后执行：

```bash
scripts/release/publish-gitee-release.sh vX.Y.Z
```

示例：

```bash
scripts/release/publish-gitee-release.sh v0.1.0
scripts/release/publish-gitee-release.sh v0.2.0-beta.1
```

脚本会：

- 从 GitHub Release 按 tag 拉取 release 元数据和已构建好的附件；
- 确保同名 tag 已推送到 Gitee；
- 在 Gitee 上创建或复用同名 Release；
- 将 GitHub Release 附件上传到 Gitee Release。

脚本路径：`scripts/release/publish-gitee-release.sh`

支持的可选环境变量：

- `GITEE_OWNER`：默认 `agentconnect`
- `GITEE_REPO`：默认 `awiki-cli`
- `GITHUB_OWNER`：默认 `AgentConnect`
- `GITHUB_REPO`：默认 `awiki-cli`
- `GITHUB_TOKEN`：可选；公开仓库通常不需要，遇到 GitHub API rate limit 时可配置

正式版的一键操作顺序就是：

1. 配置本地 release 环境和必要 token。
2. 运行 `scripts/release/release-one-click.sh X.Y.Z --channel stable`。
3. 等脚本完成 GitHub Release、Gitee mirror 和 npm 发布/校验。

正式版的手动操作顺序是：

1. 同步修改 `package.json.version`、`package.json.awikiCli.minSupportedVersion`、`crates/awiki-cli/Cargo.toml` 和 `Cargo.lock` 并提交。
2. 运行 `cargo run -p xtask -- check-version`。
3. 运行 `scripts/release/release-tag-stable.sh`。
4. 等 GitHub Actions 完成 GitHub Release 和 npm 发布。
5. 在本地运行 `scripts/release/publish-gitee-release.sh vX.Y.Z`。

## 3. 预发布版本（beta/rc）

> 预发布用于内测 / 灰度，不会自动覆盖 npm 的 `latest`，而是挂在指定 dist-tag（例如 `beta`）。

### 3.1 调整版本号

推荐直接使用一键脚本，它会同步 npm 与 Cargo 版本文件：

```bash
scripts/release/release-one-click.sh 0.2.0-beta.1 --channel beta --dist-tag beta
```

如果走手动流程，将版本同步修改为带预发布后缀的版本，例如：

- `0.2.0-beta.1`
- `0.2.0-rc.1`

提交并推送修改。

### 3.2 使用预发布脚本创建 Tag

运行：

```bash
scripts/release/release-tag-prerelease.sh <dist-tag>
```

示例：

```bash
scripts/release/release-tag-prerelease.sh beta
```

脚本行为：

- 读取 `package.json.version`，要求版本中包含 `-`（预发布后缀）；
- 检查工作区干净、当前分支已 push 且没有同名 Tag；
- 创建并推送 Tag：`v<package.json.version>`（例如 `v0.2.0-beta.1`）；
- 打印后续建议，包括如何发布带 dist-tag 的 npm 预发布包。

当前版本的 CI release workflow 只对稳定 Tag 自动执行 `npm publish`。预发布包的 npm 发布需要你在本地手动执行：

```bash
NODE_AUTH_TOKEN=... npm publish --access public --tag <dist-tag>
```

```bash
export GITEE_USERNAME=<你的 Gitee 登录用户名>
export GITEE_TOKEN=<你的 Gitee 个人访问令牌>
scripts/release/publish-gitee-release.sh vX.Y.Z-<pre>
```

预发布的一键操作顺序就是：

1. 配置本地 release 环境和必要 token。
2. 运行 `scripts/release/release-one-click.sh X.Y.Z-<pre> --channel beta --dist-tag <dist-tag>`。
3. 等脚本完成 GitHub pre-release、Gitee mirror 和 npm 发布/校验。

预发布的手动操作顺序是：

1. 同步修改 `package.json.version`、`package.json.awikiCli.minSupportedVersion`、`crates/awiki-cli/Cargo.toml` 和 `Cargo.lock` 并提交。
2. 运行 `cargo run -p xtask -- check-version`。
3. 运行 `scripts/release/release-tag-prerelease.sh <dist-tag>`。
4. 等 GitHub Actions 完成 GitHub pre-release。
5. 在本地运行 `NODE_AUTH_TOKEN=... npm publish --access public --tag <dist-tag>`。
6. 如需同步 Gitee Release，再运行：

```bash
scripts/release/publish-gitee-release.sh vX.Y.Z-<pre>
```

## 4. 回滚/撤回发布

> 回滚操作具有破坏性，仅在明确确认为 “坏版本” 时使用。脚本默认只打印推荐命令，只有在显式开启时才真正执行。

### 4.1 withdraw-release.sh 概览

脚本路径：`scripts/release/withdraw-release.sh`

用法：

```bash
scripts/release/withdraw-release.sh <version>
```

示例：

```bash
scripts/release/withdraw-release.sh 0.1.0
scripts/release/withdraw-release.sh 0.2.0-beta.1
```

脚本会：

- 计算 Tag 名：`v<version>`；
- 检查本地和远端是否存在该 Tag；
- 打印一组推荐的回滚命令，包括：
  - 删除 Git Tag（本地 + origin）；
  - 使用 GitHub CLI 草拟/删除 Release；
  - 使用 `npm deprecate` 和/或 `npm dist-tag` 调整 npm 状态；
- **只在设置环境变量 `AWIKI_CLI_WITHDRAW_EXECUTE=1` 时真正执行这些命令**，否则仅打印提示，方便人工审阅后复制执行。

典型撤回流程可以是：

1. 先在命令行预览脚本给出的建议：

   ```bash
   scripts/release/withdraw-release.sh 0.1.0
   ```

2. 确认无误后，显式开启执行开关：

   ```bash
   AWIKI_CLI_WITHDRAW_EXECUTE=1 scripts/release/withdraw-release.sh 0.1.0
   ```

3. 根据实际情况适当调整 `npm deprecate` 文案和保留的 dist-tag。

## 5. 与版本策略/强制升级的关系

awiki-cli 内部通过 Rust `update` 模块和配置项：

- `update.disable_strict_version`
- `update.metadata_cache_ttl_seconds`
- 环境变量 `AWIKI_CLI_DISABLE_STRICT_VERSION` / `AWIKI_CLI_UPDATE_CACHE_TTL` / `AWIKI_CLI_UPDATE_CACHE_ONLY`

来决定：

- 哪个版本是最新版本（latest）；
- 哪个版本是最小支持版本（minSupportedVersion）；
- 何时对过旧版本执行强制升级拦截。

一旦通过正式发布或预发布调整了 npm 上的 `version` 和 `awikiCli.minSupportedVersion`，客户端的版本策略会在缓存 TTL 过期或手动刷新后自动生效。坏版本被回滚或标记为 deprecated 后，也建议同步更新 `minSupportedVersion`，确保新版本的强制升级逻辑与发布状态一致。

在 CI、离线调试或 air-gapped 环境下，如果你希望 `awiki-cli upgrade` 只读取本地缓存、完全不访问 npm registry，可以临时设置 `AWIKI_CLI_UPDATE_CACHE_ONLY=1`。
