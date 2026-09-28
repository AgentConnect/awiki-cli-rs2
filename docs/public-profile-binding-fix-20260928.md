# 2026-09-28 公开资料身份绑定修复与验证

## 根因和边界

公共资料复用了当前账号资料解析器；对方返回空 Handle 时注入当前账号 Handle，
Directory 又覆盖 subject，掩盖资料与身份的冲突。外域 WNS 的可用 Profile 被丢弃，
容易触发 Home 的不完整 Profile 回退。消息路由仍按验证后的 Persona/会话 ID 建立，
但展示与部分宿主资料入口因此出现错误身份信息。

修复拆开 self/public 解析上下文，public 只从查询目标和已验证 Handle 补齐缺项。
显式身份冲突失败关闭。外域 WNS 的 Profile 独立校验后仅作展示；不采用 provider
私有账号 ID，不改变 DID、generation、Persona、route、数据库 schema 或密钥。
同步/异步及 Handle/DID resolve 行为一致，get_me/update_me 兼容回退保留。

## 验证

本地隔离源码副本测试：Core 为本 PR 实现；ANP 1.0.5、Identity 0.2.4 使用 registry。
只在测试副本移除这两个依赖的本机 path，保存其 Cargo.lock；原仓库来源规则不变。
命令均使用 `--offline --locked`，开发/测试 debug info 关闭、独立 target：

- `cargo test -p awiki-im-core --lib profile`：42 通过。
- `cargo test -p awiki-im-core --lib directory`：结果见配套 PR 最新验证记录。
- `cargo test -p awiki-im-core --test phase2_identity_directory`：28 通过；覆盖真实 loopback HTTP、资料更新、Persona 持久化及重开。
- 新增 4 个回归测试在原实现全部失败；修复后通过。用例涵盖当前账号有 Handle、空公共资料、WNS 昵称/缺 Handle、显式 DID/Handle 冲突、无效 display Profile、同步异步和路由一致性。
- System 仓 `tests/non_did/test_multi_tenant_flow_contract.py` 与 `test_fresh_recovery_direct_contract.py`：13 通过。这是跨域编排合同检查，不是远端投递验收。

已审阅 System 的 `test_public_profile_projection.py` 和 App 的 CONTACT-MSG-E2E-001：
服务端公开资料合同未改，Core 对响应的处理由上述测试直接覆盖。未执行真实注册及
跨域消息 System/E2E；既有 Profile System 用例依赖远端测试手机号清理，本轮不扩大
该数据操作范围。发布前仍需用候选 native SDK 进行实际客户端跨域匹配、资料刷新和发送验收。

## 安全审查和交付

检查了另一身份资料、矛盾 subject 别名、缺失 subject/Handle、无效 WNS display、
同名用户及本地/外域权威分离。测试不能通过显示名称改变消息对象，也不创建或迁移用户
真实数据。没有新增网络目的地、密钥读取、权限、服务配置或 ABI。未执行发布或 SDK
版本提升；正式消费者必须先发布修复版 Core 并更新 registry pin，不能把旧 native 制品
配上新 Dart 源码作为完整修复。
