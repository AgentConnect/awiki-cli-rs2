# CLI 自托管发布

本目录只负责 `awiki-cli` 与配套 Skill 的自托管发布，不修改 daemon 发布流程。

## 配置边界

- `release-config.json`：受 Git 管理，记录 beta/stable 版本、最低支持版本、ANP commit、平台集合和归档数量。
- `publish-server.example.toml`：服务器配置模板。
- `publish-server.toml`：服务器真实配置，必须位于同目录、权限为 `0600`，且被 Git 忽略。

编译产物与站点无关。域名、默认 tenant endpoint、公开路径、归档路径、Nginx 路径、gateway 路径和 GitHub token 只能来自 `publish-server.toml`。

## 发布顺序

1. 在干净且已推送的发布分支运行 `prepare-cli-tag.sh beta`。
2. 在目标服务器运行 `publish-cli-release.sh beta`。
3. 完成 beta 的 Linux、macOS、Skill、Onboarding 和更新检查。
4. beta 全部通过后，在同一提交运行 `prepare-cli-tag.sh stable`。
5. 在目标服务器运行 `publish-cli-release.sh stable`。
6. stable 发布会让 beta channel 同时指向 stable，使 beta 安装自动毕业。

服务器脚本不会创建 commit 或 tag。任何失败都必须先修复并发布更高版本，不能移动 tag 或把旧归档重新提升为 latest。

## Nginx

运行以下命令生成不含域名常量的 location snippet：

```bash
node scripts/release/cli/render-nginx-snippet.js \
  scripts/release/cli/publish-server.toml
```

将输出写入配置中的 `nginx_snippet`，并在目标 HTTPS `server` 块中 include 一次。修改主配置前必须备份，随后执行 `nginx -t`，成功后才能 reload。

## 公开接口

每个 channel 暴露 `manifest.json`、`awiki-cli.tgz`、`artifacts/`、`awiki-cli-skill.tar.gz` 和 `.well-known/agent-skills/index.json`。服务器只公开当前 channel 指针，历史版本保存在 `archive_root`。

Onboarding 只跟随 stable。protocol-gateway 应读取 `archive_root/channels/stable-onboarding.md`，而不是读取服务器上的活动 Git checkout。
