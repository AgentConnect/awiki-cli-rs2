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
- `cargo test -p awiki-im-core --lib directory`：27 通过。
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


## 首轮配套 App 固定源码联调（历史）

`dependencies.source.json` 固定本 PR 的实现提交 `183c8ae99ca5bc534430ff484b7f228e73d56c50`，
只有 Core 使用源码，ANP/Identity 使用既有 registry pin；配套
`dependencies.source.Cargo.lock` 随 PR 提交。App review 构建的 `cli_ref` 应指定包含这两份
文件的本分支完整提交。校验入口：

```bash
python3 scripts/dependencies/build.py --deps source \
  --source-manifest dependencies.source.json --package im-core-dart --check
```

上述固定源码 `im-core-dart --check` 已通过，解析回执确认 Core 为固定提交，ANP/Identity 为 registry。

此清单只用于 review 集成，不改变正式 registry 构建默认值。Core 发布并更新消费者的
正式 pin/lock 后删除临时清单和锁；不得以源码联调成功替代 SDK 已发布的核验。

## 统一 review 的当前入口

原 #53 的实现与来源提交完整合入发布 PR #52，配套 App #46 整合进 #45。
当前 source 清单固定整合后的 Core 提交 `bfc3815b5f8056519dde22684ab19f516cf599db`；
`pull_request` 更新为 #52，联调锁同步 Daemon 0.1.106。Core 的资料实现和测试与原修复
提交逐文件一致。上节 SHA 和结果保留为首轮历史证据，当前构建请使用本节来源及仓内清单。

正式发布仍先发布修复 SDK，再更新 registry pin、撤除临时 source 清单并构建客户端。
本次整合没有发布 SDK 或客户端；最终复验记录见 awiki-plan 的统一 review 清单。
