# README 与配套文档发布检查清单

[English](release-readiness-checklist.md) | [简体中文](release-readiness-checklist.zh-CN.md)

## P0：发布前必须完成

- [ ] README 首段能在不解释 ANP、DID 或内部模块的情况下说明用户价值。
- [ ] 项目状态使用 `MVP`、`Developer Preview`、`Beta` 或 `Stable`，且有负责人确认。
- [ ] 所有下载、安装和升级地址已在无登录环境验证。
- [ ] 没有 `{{...}}`、`TODO(...)`、内部域名、临时分支或个人路径。
- [ ] 所有代码块中的命令已在干净环境执行。
- [ ] README 的第一次成功路径不依赖未说明的 sibling checkout、账号池或秘密配置。
- [ ] 平台支持与真实 SDK/打包/CI 状态一致。
- [ ] E2EE 描述区分 Direct、Group、客户端、服务端和依赖条件。
- [ ] 自托管服务首屏明确无 E2EE、单节点和生产身份提供方边界。
- [ ] `SECURITY.md` 中存在可用的私有漏洞报告渠道。
- [ ] 截图已替换占位说明，且不存在真实敏感信息。
- [ ] 中文与英文 README 的事实一致。
- [ ] 最终版本已进入默认分支。

## 链接检查

- [ ] 语言切换链接正确。
- [ ] 所有同仓库相对路径存在。
- [ ] 跨仓库链接指向组织 canonical repository。
- [ ] 文档锚点在 GitHub 渲染后可跳转。
- [ ] Releases、Roadmap、Issues、Security 和 License 入口可访问。
- [ ] 图片路径大小写与文件系统一致。

## 命令检查

- [ ] `awiki-me` 的源码构建在支持平台执行成功。
- [ ] `awiki-cli version`、`status`、`doctor` 与首次消息流程可执行。
- [ ] CLI 对外安装命令不包含发布模板变量。
- [ ] `awiki-open-server` 可从空目录创建 venv、安装、启动并通过 `healthz`。
- [ ] Open Server 的 `smoke-local` 或等价首次成功检查通过。
- [ ] 公网部署文档未启用 `AWIKI_ALLOW_UNSIGNED_PEER_DEV` 或联系验证兼容开关。

## 兼容性检查

- [ ] 记录验证日期和各组件版本/commit。
- [ ] AWiki Me ↔ 托管服务的 direct、group、attachment、contact 状态已验证。
- [ ] awiki-cli ↔ Open Server 的身份、私信、群参与、附件、people/site 状态已验证。
- [ ] AWiki Me ↔ Open Server 的自定义租户主流程已验证或明确标记未验证。
- [ ] 非 allowlist 自托管域名上的 Agent/Daemon 状态被明确说明。
- [ ] Web 不被误写为可用产品平台。

## GitHub 仓库首页检查

- [ ] About Description 与 README 首段一致。
- [ ] Topics 精准，数量控制在 4–8 个。
- [ ] Social Preview 已设置。
- [ ] Badges 不超过必要范围，且状态真实。
- [ ] 默认分支 README 为最新版本。
- [ ] 仓库名与产品身份不一致时，README 首屏已消除歧义。

## 陌生用户测试

邀请至少 3–5 位不了解当前架构的人，仅阅读仓库首页：

- [ ] 10 秒后能用一句话描述项目。
- [ ] 30 秒后知道它和另外两个 AWiki 仓库的关系。
- [ ] 60 秒后能判断自己是否是目标用户。
- [ ] 5 分钟内完成一次有意义的成功操作。
- [ ] 能准确说出成熟度、平台状态和安全边界。
