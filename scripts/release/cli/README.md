# CLI 自托管发布

本目录只负责 `awiki-cli` 与配套 Skill 的自托管发布，不修改 daemon 发布流程。

## 配置边界

- `release-config.json`：受 Git 管理，记录 beta/stable 版本、最低支持版本、ANP commit、平台集合和归档数量。
- `publish-server.example.toml`：服务器配置模板。
- `publish-server.toml`：服务器真实配置，必须位于同目录、权限为 `0600`，且被 Git 忽略。

编译产物与站点无关。域名、默认 tenant endpoint、公开路径、归档路径、Nginx 路径、Nginx 备份路径、下载并发/限速参数、显式 document_tenants 配置路径 和 GitHub token 只能来自 `publish-server.toml`。

CLI 产品包中的两个内置租户则由构建参数决定：

```bash
scripts/release/build-release-artifact.sh \
  --tenant-config config/builtin-tenants.default.json \
  <其余发布参数>
```

配置必须一次提供 `primary`、`secondary`、中英文名称、backend Origin、DID Host 和 `default_slot`。省略参数时使用仓库默认文件；传入时完整替换，不与默认官方端点合并。每个平台归档都包含同一份 `BUILTIN-TENANTS.json`，`stage-release.js` 会在组装 NPM 引导包前验证所有归档的内容与摘要完全一致。

Linux AMD64 使用静态 musl 目标构建，并在归档前拒绝包含 GLIBC 版本符号的二进制，避免产物依赖 GitHub runner 的 glibc 版本。

当前 Windows 发布产物是 `windows-amd64`。Windows 11 ARM64 安装器在 manifest 未声明原生 ARM64 条目时选择该 x64 兼容产物；已经声明但无效的 ARM64 条目会直接失败。Manifest、归档名称和安装日志始终保留实际的 `windows-amd64` 架构，不创建伪 ARM64 条目。

所有平台的 `awiki-cli version` 都嵌入不可变 tag 对应的完整 40 位 Git commit；发布验收和系统测试使用它确认运行中的二进制与 manifest 来源一致。

## 发布方式

Beta 和 Stable 是相互独立的发布通道，按实际需要选择其中一个发布，不要求成对执行，发布任一通道也不会改写另一个通道。

1. 在干净且已推送的发布分支运行 `prepare-cli-tag.sh beta` 或 `prepare-cli-tag.sh stable`。
2. 需要应用新 Nginx 规则时，在目标服务器先运行 `deploy-nginx-config.sh`并完成回归。
3. 在目标服务器运行对应的 `publish-cli-release.sh beta` 或 `publish-cli-release.sh stable`。
4. 验证本次所选通道的 Linux、macOS、Skill、Onboarding 和更新检查。

需要并行构建、随后统一上线时，可先对不可变 CLI tag 调度原有 Actions，再使用
`publish-cli-release.sh --run-id <ID> stable` 复用该次构建。入口检查 workflow、事件、
tag commit 与通道名称，随后执行原有下载、产物校验和发布流程，不重复构建。
Daemon 的独立入口也支持 `--run-id <ID>`，要求配置中的 `source_ref` 为精确 SHA，
且 workflow controller 与产品源码属于该次相同提交；平台包和版本检查保持原样。

Stable 发布必须生成、安装并校验文档后才完成；Beta 不改变 Stable 文档指针。

服务器脚本不会创建 commit 或 tag。任何失败都必须先修复并发布更高版本，不能移动 tag 或把旧归档重新提升为 latest。

## Nginx

下载保护需要两份不同 Nginx 作用域的生成配置：

- `nginx_http_snippet`：放在 `http` 作用域的 CLI 下载连接状态区，建议路径为 `/etc/nginx/conf.d/00-awiki-cli-download-zones.conf`。
- `nginx_snippet`：在 AWiki HTTPS `server` 中 include 的 CLI location 配置。

可以分别预览两份生成结果：

```bash
node scripts/release/cli/render-nginx-download-zones.js \
  scripts/release/cli/publish-server.toml

node scripts/release/cli/render-nginx-snippet.js \
  scripts/release/cli/publish-server.toml
```

正式部署不手工拷贝输出，而是使用独立脚本：

```bash
scripts/release/cli/deploy-nginx-config.sh \
  --config scripts/release/cli/publish-server.toml
```

脚本会确认站点配置已 include `nginx_snippet`，生成两份候选文件，有变化时先备份再安装。只有 `nginx -t` 成功才 reload；验证或 reload 失败时自动恢复原文件。两份生成文件与线上完全一致时，只执行 `nginx -t`，不 reload。

`publish-cli-release.sh` 不会隐式调用该脚本。Nginx 配置部署和 CLI 版本发布是两个独立失败边界；需要在下一次启用新保护时先显式运行 `deploy-nginx-config.sh`，验证通过后再运行发布脚本。

### 下载与缓存边界

- 只有 `stable/beta/artifacts/` 下文件名已包含版本号的平台包使用并发限制、速度限制和 `immutable` 长期缓存。
- `manifest.json`、`awiki-cli.tgz`、Skill 包、Skill 发现文件、通道根路径和 CLI 站点文件继续使用 `no-cache, no-store, must-revalidate`。
- 保留 Nginx 静态文件的 HTTP Range 能力，不对小型 npm 引导包应用大型二进制下载限制。
- CLI 站点目前同时包含带哈希和不带哈希的资源，不允许将整个 `/cli/assets/` 设为 `immutable`。

## 下一次发布顺序

1. 在 Mac 开发分支完成测试、提交和推送，准备不可变的 CLI tag。
2. 在发布窗口内将服务器仓库切换到本次发布使用的代码，并为被 Git 忽略的 `publish-server.toml` 补充新配置项。
3. 先运行 `deploy-nginx-config.sh`，确认备份路径、`nginx -t` 和 reload 结果。
4. 验证当前通道、网站、API、WebSocket、Range、缓存头和下载限制。
5. 再运行 `publish-cli-release.sh beta|stable`。
6. 验证 manifest 已指向新版本，平台包保持 `immutable`，manifest 和 npm 引导包仍不长期缓存。

## 公开接口

`/cli/onboarding.md` 和 `/cli/skill.md` 由 Nginx 提供同一份完成租户渲染的静态 Markdown。每个 channel 暴露 `manifest.json`、`awiki-cli.tgz`、`artifacts/`、`awiki-cli-skill.tar.gz` 和 `.well-known/agent-skills/index.json`；channel 根路径直接返回同一份 `manifest.json`，未发布的 channel 返回 `404`。服务器只公开当前 channel 指针，历史版本保存在 `archive_root`。

Onboarding 只跟随 stable。文档位于 `archive_root/document-releases/<release.json SHA-256>/`，
活动指针为 `web_root/docs`；目录内按显式租户域名隔离。模板和渲染由本仓拥有，网站不复制正文。
`release.json` 记录 CLI 版本/来源、installer 摘要、模板摘要、文档来源提交、完整租户配置及摘要和每个文件 SHA-256。
Nginx 返回 `text/markdown; charset=utf-8`、`Cache-Control: no-store`，未知租户不自动推断。

## 配套现有 CLI 迁移文档

`document-tenants.shanghai.json` 默认中国租户；`silicon-valley` 显式选择硅谷；
`singapore` 为主站及三个隔离域名分别生成目标租户。隔离域名下载政策明确绑定新加坡主站。
先核对现有 CLI 的 `tenant setup --help` 及正文命令兼容性，准备包含来源和 installer 摘要的原 manifest；
不要使用无 source 字段的政策投影伪造构建来源。

```bash
CLI=scripts/release/cli/publish-cli-release.sh
bash "$CLI" --documents prepare onboarding.md document-tenants.json existing-manifest.json /tmp/docs-stage FULL_DOCUMENT_SOURCE_SHA
sudo bash "$CLI" --documents install /tmp/docs-stage /var/lib/awiki-cli
# install 输出不可变路径 DOC_RELEASE；准备/安装均不改变 Stable。
bash "$CLI" --documents verify "$DOC_RELEASE" --before-activation
sudo bash "$CLI" --documents activate "$DOC_RELEASE" /var/www/awiki-web/cli/docs
# 失败自动恢复；进程中断可按持久 journal 显式恢复：
sudo bash "$CLI" --documents restore /var/www/awiki-web/cli/docs
```

激活必须校验 User Service 有效政策、公开 manifest、实际 installer 摘要和全部租户文档的
Content-Type/缓存/SHA-256。生产政策仍由 User Service 拥有；准备阶段不更改政策。
若正常新版本发布时政策尚未指向对应 CLI，发布会失败并恢复通道/文档指针；按既有政策发布流程协调后重试。
同一环境的发布/迁移必须串行，不在切换期间运行网站发布或第二个 CLI 发布。
回滚记录为 `web_root/docs.transaction.json`。保留当前和恢复指针引用的旧制品，不自动清理。
首次静态切换使用 System Test 的专用退役入口协调 Nginx、文档指针和旧服务恢复；
后续只使用本发布流程，不另设手工同步目录。

## 发布构建使用线上 SDK

CLI 和 daemon 的 release builder 使用 [registry-build.py](../registry-build.py)，在临时 Git worktree
中将 ANP、Identity 和 IM Core dependency 切换到 crates.io。版本由
[registry-dependencies.json](../registry-dependencies.json) 固定为本次已发布版本；
[registry-Cargo.lock](../registry-Cargo.lock) 锁定完整依赖图。底层 Cargo 开发工作区保留 path dependency；团队默认 Debug 的 registry/local/source 选择使用
[统一依赖入口](../../dependencies/README.md)，不依靠修改原工作区 manifest 切换。

Flutter App 的 release worker 设置 `AWIKI_RELEASE_REGISTRY=1`，使原生构建走同一入口；
普通 Debug/本地 SDK 开发不设置此变量，继续使用工作区源码。IM Core Node artifact workflow
也在独立的 registry 工作区构建 addon，原始源码 checkout 仍用于 provenance 和打包审核。

每次 SDK 发布后，更新三个版本，再在干净且已提交的源码工作区生成 registry lock：

```bash
python3 scripts/release/registry-build.py --refresh-lock
# 检查并提交 registry-Cargo.lock 后，验证 registry 消费者可以构建。
python3 scripts/release/registry-build.py -- cargo check -p awiki-cli -p awiki-deamon -p im-core-dart -p awiki-im-core-node
```

脚本核对 Cargo 实际 metadata，拒绝 path/git SDK 或不同版本，构建失败不回退到本地 SDK。
临时 worktree 退出即删除；产物仍进入原工作区 `target/`（或显式 `CARGO_TARGET_DIR`）。
`--prepare <new-directory>` 用于 CI 容器构建，生成的 worktree 保留到 CI 工作区清理。
