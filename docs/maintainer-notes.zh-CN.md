# awiki-cli-rs2 README 上线前维护说明

[English](maintainer-notes.md) | [简体中文](maintainer-notes.zh-CN.md)

本文不面向最终用户。

## 1. 仓库身份

长期建议评估将仓库重命名为：

```text
awiki-client
awiki-client-workspace
```

二进制继续使用 `awiki-cli`。如暂不改名，README 标题至少应使用 `AWiki Client Workspace`，并在首段解释历史仓库名。

## 2. 建议 GitHub About

**Description**

```text
AWiki client workspace: CLI, Rust IM SDK, Agent Runtime Daemon, Flutter SDK, and agent skills for ANP messaging.
```

**Topics**

```text
agent, cli, rust, sdk, anp, did, messaging, flutter
```

## 3. P0：修复公开安装入口

以下文件当前仍包含 `{{AWIKI_CLI_CHANNEL_BASE_URL}}`：

- `onboarding.md`；
- `skills/references/00-installation.md`；
- 可能由发布脚本生成的 onboarding/Skill 文档。

在 stable URL 确认前，不要在 README 展示伪可执行命令。发布完成后应验证：

```text
<public-origin>/<public-base-path>/<channel>/manifest.json
<public-origin>/<public-base-path>/<channel>/awiki-cli.tgz
<public-origin>/<public-base-path>/<channel>/awiki-cli-skill.tar.gz
<public-origin>/<public-base-path>/<channel>/.well-known/agent-skills/index.json
```

实际路径以 `publish-server.toml` 和 Nginx 输出为准。

## 4. P0：版本一致性

审阅基线中：

- `crates/awiki-cli/Cargo.toml` 为 `1.0.16`；
- `scripts/release/cli/release-config.json` stable 为 `1.0.18`；
- Skill metadata 另有独立版本。

发布前必须明确这是预期分层还是漂移，并保证 binary、wrapper、manifest、Skill、tag 和 embedded commit 可追溯。

## 5. 成熟度

Skill 当前将 Messaging、Runtime、Discovery、People 和 Debug 描述为 partially implemented。README 不应使用“完整支持所有 AWiki 能力”或“生产就绪 Client Stack”等措辞。

建议在每次 stable 发布生成 capability snapshot，而不是手工维护模糊列表。

## 6. 命名

公共文案统一：

- `AWiki`；
- `AWiki Daemon`；
- 仅在二进制、crate 和路径中写 `awiki-deamon`；
- 中文 README：`README.zh-CN.md`。

## 7. 默认分支

审阅基线为 `release/0710`，默认分支为 `main`。最终 README 和安装修复必须进入默认分支。

## 8. README 下沉内容

旧 README 中以下内容保留在专项文档，不在首页完整展开：

- 全量 command groups；
- Daemon 开发命令；
- Core 每个业务领域的完整枚举；
- Dart SDK 实现细节；
- 发布服务器操作；
- Development Rules；
- 完整 Documentation Map。

首页只保留入口和边界。
