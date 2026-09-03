# CLI 自托管发布

本目录只负责 `awiki-cli` 与配套 Skill 的自托管发布，不修改 daemon 发布流程。

## 配置边界

- `release-config.json`：受 Git 管理，记录 beta/stable 版本、最低支持版本、ANP commit、平台集合和归档数量。
- `publish-server.example.toml`：服务器配置模板。
- `publish-server.toml`：服务器真实配置，必须位于同目录、权限为 `0600`，且被 Git 忽略。

编译产物与站点无关。域名、默认 tenant endpoint、公开路径、归档路径、Nginx 路径、Nginx 备份路径、下载并发/限速参数、gateway checkout、gateway 内网 origin 和 GitHub token 只能来自 `publish-server.toml`。

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

只有 Stable 发布会更新 `/cli/onboarding.md` 和 protocol-gateway 使用的 stable onboarding 快照；Beta 发布不会改变线上 onboarding。

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

`/cli/onboarding.md` 和 `/cli/skill.md` 由生成的 Nginx snippet 代理到 protocol-gateway。每个 channel 暴露 `manifest.json`、`awiki-cli.tgz`、`artifacts/`、`awiki-cli-skill.tar.gz` 和 `.well-known/agent-skills/index.json`；channel 根路径直接返回同一份 `manifest.json`，未发布的 channel 返回 `404`。服务器只公开当前 channel 指针，历史版本保存在 `archive_root`。

Onboarding 只跟随 stable。protocol-gateway 应读取 `archive_root/channels/stable-onboarding.md`，而不是读取服务器上的活动 Git checkout。
