# AWiki Client Workspace README 素材计划

[English](screenshot-plan.md) | [简体中文](screenshot-plan.zh-CN.md)

CLI 项目不需要大量图片，但需要一段能证明“Agent-friendly CLI”的真实终端演示。

## 1. Hero GIF：第一次消息

- 文件：`awiki-cli-first-message.gif`；
- 时长：25–40 秒；
- 推荐尺寸：1400×800；
- 流程：
  1. `awiki-cli status --format json`；
  2. `awiki-cli id status`；
  3. `awiki-cli msg send ... --dry-run`；
  4. 实际发送；
  5. `awiki-cli msg inbox --format table` 或 JSON；
- 目标：同时证明可读的人类视图与稳定机器输出；
- 不展示安装模板 URL、真实 identity、token、手机号或绝对路径。

## 2. JSON Envelope 静态图（可选）

- 文件：`awiki-cli-json-envelope.png`；
- 展示：`ok`、`command`、`data`、`warnings`、`meta`；
- 不需要展示完整 DID，使用 `example.com` demo identity；
- 可放在 Output Contract 章节。

## 3. Workspace 架构图

优先使用 README 内 Mermaid，避免静态图漂移。需要 Social Preview 时再导出：

- 文件：`awiki-client-workspace-architecture.png`；
- 展示：Skill/CLI/Daemon/Dart SDK → IM Core → Services；
- 不绘制所有 crate 内部模块。

## 4. Agent Host Notification 演示（可选）

- 文件：`awiki-cli-host-notification.gif`；
- 流程：远端消息到达 → listener → host notification → Agent 收到提示；
- 必须遮挡 token、session-key、channel 和真实平台账号；
- 仅在完整链路已稳定验证后放入主 README，否则放在专项文档。

## 5. Social Preview

- 文件：`awiki-client-workspace-social-preview.png`；
- 尺寸：1280×640；
- 文案：`CLI, IM SDKs, Agent Runtime and Skills for ANP messaging`；
- 视觉：终端片段 + 简化组件箭头，不使用密集 crate 列表。

## 6. 录制要求

- shell prompt 简洁；
- 字号至少 18px；
- 使用隔离 workspace；
- 输出使用 demo handle/DID；
- 清除环境变量、用户目录和内部域名；
- 写操作先 dry-run；
- 不展示 `debug db`、raw secret 或未稳定 command。
