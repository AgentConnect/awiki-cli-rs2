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

测试批次可显式向 `build-cli-release.yml` 传完整 40 位 `source_ref` SHA 和已提交的 `tenant_config` 路径，复用相同平台矩阵、registry 来源和制品检查。默认租户文件与正式 tag 入口不变；此方式只产生 Actions 制品，不创建正式 tag、GitHub Release 或激活下载渠道。正式发布器的 tag 合同仍保留，测试批次由独立目录交付。

需要并行构建、随后统一上线时，可先对不可变 CLI tag 调度原有 Actions，再使用
`publish-cli-release.sh --run-id <ID> stable` 复用该次构建。入口检查 workflow、事件、
tag commit 与通道名称，随后执行原有下载、产物校验和发布流程，不重复构建。
Daemon 的独立入口也支持 `--run-id <ID>`，要求配置中的 `source_ref` 为精确 SHA，
且 workflow controller 与产品源码属于该次相同提交；平台包和版本检查保持原样。

Stable 发布必须生成、安装并校验文档后才完成；Beta 不改变 Stable 文档指针。

服务器脚本不会创建 commit 或 tag。产品内容变化必须发布更高版本，不能移动 tag 或覆盖旧归档。
若仅因政策尚未协调或网络验证失败，可以重新激活同一份已安装、已核对摘要的制品，不重新打包或替换内容。

### Stable 版本化安装地址与两阶段发布

Stable 的包内元数据、manifest 和引导文档统一使用
`<public_origin><public_base_path>/stable/releases/<版本>/awiki-cli.tgz`；Beta 保留通道别名。
新增必填 `installer_root`：实际安装到 `installer_root/<版本>/awiki-cli.tgz`。
已有同版本文件仅在摘要相同时复用，内容不同时拒绝覆盖。安装不会更改 User Service 政策。

**先核对现有下载目录再部署 Nginx**。已有生产版本化包时，`installer_root` 必须指向其当前版本化安装包目录，
保留旧版本文件和符号链接；不得用一个空目录替换旧版本下载树。缺此配置时发布和 Nginx 生成均失败关闭。
站点生成器增加更精确的版本化路径及通道安装包、Skill 下载/发现文件路由，保留 User Service 的 manifest 精确代理。
若站点已有版本化路径（例如硅谷），只有其 alias 与 `installer_root` 一致时复用；不重复生成同名 location，冲突则拒绝生成。

政策尚未提升时，通过同一个发布入口分两步执行：

```bash
# 只安装完整制品及文档；不切换通道或文档，不修改政策。
scripts/release/cli/publish-cli-release.sh --config SERVER_CONFIG --run-id RUN_ID --stage-only stable
# 按既有 User Service 流程协调推荐版本、版本化 URL 和制品摘要；不由本脚本另建政策。
# 使用上一步 installed_release 输出，避免重建同版本安装包。
sudo scripts/release/cli/publish-cli-release.sh --release activate SERVER_CONFIG INSTALLED_RELEASE
```

政策已就绪时仍可直接运行原 `publish-cli-release.sh stable`。验证同时兼容通道静态 manifest
与生产 User Service 投影；版本、URL、摘要、公开文档均通过才报告完成。

### 发布中断恢复

常规发布使用同一份事务记录保存旧/新通道及 Stable 文档目标；Beta 只保存 Beta 通道。
记录在第一次公开指针变化前原子写入并同步文件与目录。Stable 使用 `web_root/docs.transaction.json`，
Beta 使用 `web_root/beta.transaction.json`；网站部署保护两份记录。
出现 `pending` 或 `restoring` 时，禁止继续发布，先恢复：

```bash
sudo scripts/release/cli/publish-cli-release.sh --release restore SERVER_CONFIG stable
# Beta 对应：--release restore SERVER_CONFIG beta
```

恢复可在新进程中执行，也可重复执行；中途再次被强制结束后继续同一恢复。
检测到与记录无关的人工指针变化时拒绝覆盖。普通验证异常自动恢复；SIGKILL/重启后按上述入口显式恢复。
旧 `--documents restore <web_root>/docs` 入口也能识别常规发布记录并恢复完整组合；纯文档迁移的旧记录保持兼容。
同环境发布、文档迁移、恢复和网站部署须串行。没有后台恢复服务，也不承诺多个符号链接在请求层同时可见；
保证的是发布完成前验证配套关系，以及中断后能够恢复完整旧组合。保留恢复记录引用的制品。

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

- `stable/beta/artifacts/` 下平台包使用并发限制、速度限制和 `immutable` 长期缓存；Stable 版本化安装包使用 `immutable`，不套用大型二进制限速。
- 通道别名 `awiki-cli.tgz`、Skill 包及发现文件返回 `no-store`；其余可变通道/站点文件保留 `no-cache, no-store, must-revalidate`。生产 manifest 仍由原 User Service 代理控制。
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

`/cli/onboarding.md` 和 `/cli/skill.md` 由 Nginx 提供同一份完成租户渲染的静态 Markdown。每个 channel 暴露 `manifest.json`、`awiki-cli.tgz`、`artifacts/`、`awiki-cli-skill.tar.gz` 和 `.well-known/agent-skills/index.json`；channel 根路径直接返回同一份 `manifest.json`，未发布的 channel 返回 `404`。通道入口读取当前 channel 指针，Stable 版本化安装包独立保留在 `installer_root`，完整历史制品保存在 `archive_root`。

Onboarding 只跟随 stable。文档位于 `archive_root/document-releases/<release.json SHA-256>/`，
活动指针为 `web_root/docs`；目录内按显式租户域名隔离。模板和渲染由本仓拥有，网站不复制正文。
`release.json` 记录 `public_base_path`、CLI 版本/来源、installer URL 与摘要、模板摘要、文档来源提交、完整租户配置及摘要和每个文件 SHA-256。
正常备包自动沿用 `publish-server.toml` 的 `public_base_path`，文档、Skill 地址和验证请求使用同一路径；未含此字段的旧文档记录按 `/cli` 读取。
Nginx 返回 `text/markdown; charset=utf-8`、`Cache-Control: no-store`，未知租户不自动推断。

## 配套现有 CLI 迁移文档

`document-tenants.shanghai.json` 默认中国租户；`silicon-valley` 显式选择硅谷；
`singapore` 为主站及三个隔离域名分别生成目标租户。隔离域名下载政策明确绑定新加坡主站。
先核对现有 CLI 的 `tenant setup --help` 及正文命令兼容性，准备包含来源和 installer 摘要的原 manifest；
不要使用无 source 字段的政策投影伪造构建来源。

```bash
CLI=scripts/release/cli/publish-cli-release.sh
bash "$CLI" --documents prepare onboarding.md document-tenants.json existing-manifest.json /tmp/docs-stage FULL_DOCUMENT_SOURCE_SHA /cli
sudo bash "$CLI" --documents install /tmp/docs-stage /var/lib/awiki-cli
# install 输出不可变路径 DOC_RELEASE；准备/安装均不改变 Stable。
bash "$CLI" --documents verify "$DOC_RELEASE" --before-activation
sudo bash "$CLI" --documents activate "$DOC_RELEASE" /var/www/awiki-web/cli/docs
# 失败自动恢复；进程中断可按持久 journal 显式恢复：
sudo bash "$CLI" --documents restore /var/www/awiki-web/cli/docs
```

迁移命令最后一个参数为可选 `PUBLIC_BASE_PATH`（默认 `/cli`）；自定义路径必须与目标服务器配置一致。
激活必须校验 User Service 有效政策、公开 manifest、文档实际 installer URL 与摘要和全部租户文档的
Content-Type/缓存/SHA-256。生产政策仍由 User Service 拥有；准备阶段不更改政策。正文安装命令使用 manifest 的精确 installer.url，必须与有效政策 URL 一致，并实际下载该 URL 核对摘要；旧 URL 失效或仅摘要一致也会拒绝激活。Skill 地址保持原有通道合同。
若正常新版本发布时政策尚未指向对应 CLI，发布会失败并恢复通道/文档指针；按既有政策发布流程协调后重试。
同一环境的发布/迁移必须串行，不在切换期间运行网站发布或第二个 CLI 发布。
纯文档迁移的回滚记录同样位于 `web_root/docs.transaction.json`；与常规 Stable 发布串行共用，恢复入口按记录类型处理。保留当前和恢复指针引用的旧制品，不自动清理。
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
