# awiki-cli 邮件命令（简要说明）

本说明用于记录邮件能力在 v2 CLI 中的归属与最小用法。

## 命令归属
- 邮件命令作为顶级域提供：`awiki-cli mail ...`
- `mail` 与 `msg` 同级，便于终端自动补全、schema 暴露和 AI 工具调用。

## 主要命令
- `awiki-cli mail inbox --folder inbox --limit 20 --offset 0 [--unread]`
- `awiki-cli mail notify --limit 20`
- `awiki-cli mail read --id <MESSAGE_ID>`
- `awiki-cli mail mark-read <MESSAGE_ID...>`
- `awiki-cli mail account`
- `awiki-cli mail send --to a@b.com,b@c.com --subject "Hello" --body "Hi" [--cc ...] [--html ...]`
- `awiki-cli mail attachment download --message-id <MESSAGE_ID> --attachment-index 0 --output <path>`

## 配置说明
- 邮件服务地址通过 `config.yaml` 的 `services.mail_service_url` 配置。
- 若未配置，默认复用 `services.service_base_url` 作为邮件服务基础地址。
- 在 awiki.ai 线上环境，推荐在 `config.yaml` 中显式设置：`services.mail_service_url: https://mail.awiki.ai`。
- `config show` 会展示 `mail_service_url` 及其来源。
