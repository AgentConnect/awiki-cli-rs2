# README 截图与演示素材规范

[English](screenshot-guidelines.md) | [简体中文](screenshot-guidelines.zh-CN.md)

## 1. 通用目标

截图不是装饰。每张图必须回答一个用户问题：

- 产品长什么样？
- 第一次操作如何完成？
- Agent 与普通聊天有什么不同？
- CLI 输出为什么适合自动化？
- 自托管服务如何证明真实互通？

## 2. 文件与尺寸

建议资源目录：

```text
docs/assets/readme/
```

建议尺寸：

| 素材 | 推荐尺寸 | 格式 |
| --- | --- | --- |
| README Hero | 1440×900 或 1600×1000 | PNG/WebP |
| 功能截图 | 1200×750 以上 | PNG/WebP |
| Social Preview | 1280×640 | PNG |
| 终端静态图 | 1400×800 左右 | PNG |
| 短演示 | 20–40 秒，宽度 1200–1440 | MP4 转 GIF/WebP，或托管视频缩略图 |

文件名统一使用小写英文和连字符，例如：

```text
awiki-me-hero-conversation.png
awiki-cli-first-message.gif
open-server-cross-domain-smoke.png
```

## 3. 隐私与安全

拍摄前必须替换或遮挡：

- 真实姓名、头像、手机号、邮箱；
- 完整 DID、handle 和内部域名；
- access token、refresh token、JWT、私钥、验证码；
- 本机用户名、绝对路径、IP、Team ID；
- 测试账号池、真实群成员和内部消息；
- 浏览器书签、通知、菜单栏敏感信息。

建议统一使用：

```text
alice@example.com
bob@example.com
alice.example
bob.example
did:wba:example.com:users:alice:e1_demo
```

## 4. 产品截图要求

- 使用真实可运行版本，而不是与实现明显不一致的旧设计稿；
- 对核心区域进行轻量标注时，不遮挡 UI；
- 保持同一缩放比例、窗口尺寸和主题；
- 不展示报错、调试面板或未完成入口，除非截图主题就是限制说明；
- 图片下方提供一句说明，指出用户应该观察什么。

## 5. 终端演示要求

- 使用干净的 shell prompt；
- 命令逐步出现，输出停留足够时间；
- 不录制安装下载等待和无意义日志；
- 只展示关键输出字段；
- 对写操作先演示 `--dry-run`，再演示执行；
- 字号至少 18px，保证 GitHub 页面缩放后可读。

## 6. Alt Text 模板

推荐：

```markdown
![AWiki Me 会话页，左侧是会话列表，右侧展示人与 Agent 的任务和授权消息](docs/assets/readme/awiki-me-hero-conversation.png)
```

不推荐：

```markdown
![screenshot](image.png)
```

## 7. 更新责任

任何改变以下内容的 PR，都应检查 README 素材是否需要重拍：

- 顶层导航；
- 登录/注册流程；
- 核心命令及输出结构；
- Agent 授权或任务卡；
- 安装方式；
- 公开服务域名；
- 兼容性状态；
- 品牌与图标。
