# PR #64 review 修复验证记录

日期：2026-10-24。基于 `3ef59f2b0089db9512aef8ead1090af7a49bec08`，沿用
`test/singapore-release-20261022`，目标 `release/1014`。
本轮修复源码、更新合同与候选检查清单，不打包 App、不发布 SDK、不部署服务器。

## 修复与边界

| 问题 | 根因与修复 | 回归证据 |
| --- | --- | --- |
| Direct 模式在 WS Upgrade 失败后仍可能访问系统代理 | 调用方和运行时 `alternate` 都限制为 SystemPreferred；实际建连和池客户端再次校验策略；过期探测不能写回缓存 | Direct 的 401/403/503 保持原错误且代理无连接；探测途中关闭代理不能恢复旧路由 |
| 群恢复提示只记台账，没有主动执行 | 成功持久化提示后唤醒现有 SyncDispatcher；同数据库最多一个群维护任务，计入原共享额度和处理租约；保留处理中到达的新提示、退避及重启扫描 | 单条提示实际进入生产 repair；失败不伪装 ready；处理中再提示、重启、blocking runner 空闲均能推进 |
| WS 策略变化检测可被连续帧及读取取消无限推迟 | 一秒原生配置检查期限保存在 transport，帧边界廉价检查代次；默认 250ms 外层取消不会重置期限；握手前绑定代次并在完成后核验 | 空闲、持续消息/Ping/Pong、外层取消、未直接更新代次的配置变化、握手途中切换均覆盖 |

接收关键路径只唤醒群维护，不再等待原来的八秒 inline recovery。blocking realtime 自有
runtime 使用一个工作线程，使提示处理返回后、WS 空闲期间的维护仍能执行。
复用原 journal、权威 group/DID 读取、冻结设备与 Add-before-Remove 约束、原始群主设备权限、
单项超时及最多八项的批次边界；没有新增群修复协议、数据库表、账号迁移或公共 DTO。
未扩大到发送按钮的完成合同或服务端通知队列背压。

## 本轮实际验证

本机 macOS arm64，Rust 1.88.0。原工作区 sibling ANP/Identity 版本与 manifest 要求不一致，
因此将本轮 Core 源码复制至工作区 `tmp/core-review-fixes-20261024/source`，只在隔离 manifest
规范化 ANP/Identity 来源。`cargo metadata --offline --locked` 确认实际解析为 registry
ANP **1.0.7**、Identity **0.2.6**；Core **0.1.10** 为本轮源码，没有冒充已发布新 SDK。
全部 Core Rust 文件与受测快照逐字节一致，原工作区 manifest/lock 没有修改。

以下测试使用同一隔离 manifest：

```sh
cargo test --manifest-path ../tmp/core-review-fixes-20261024/source/Cargo.toml \
  -p awiki-im-core --lib --locked --features blocking,group-e2ee,secure-direct \
  <filter> -- --nocapture
```

| filter／检查 | 结果 |
| --- | --- |
| `network_tests` | 5 通过；async WS 专用夹具 |
| `network::tests` | 11 通过；含 Direct 防线、过期探测及既有安全换路 |
| `realtime::runner::tests` | 29 通过；含四项群恢复执行/续跑回归 |
| `sync_dispatcher::wakeup_tests` | 2 通过；提示代次与 owner 隔离 |
| `central_dispatcher`、`timed_out_task_keeps`、`foreground_wait_after` | 各 1 通过；共享额度、租约占位和等待补偿 |
| `internal::group_e2ee::transition` | 5 通过；权威、冻结设备及顺序不变式 |
| `v2_product_tests` | 19 通过；P5 持久确认、固定密文重试、对方优先 |
| `internal::realtime::` | 35 通过；包含前述 5 项 WS 夹具，另覆盖 Legacy/V2/V3/P6 协商、认证及投影 |
| `cargo check --manifest-path … -p awiki-im-core --locked`（默认特性） | 通过；既有 `messages/security.rs` 未使用参数警告保留 |
| 14 个受影响 Rust 文件 `rustfmt --edition 2021 --config skip_children=true --check` | 通过 |
| `git diff --check`、本轮文档路径/结构检查 | 通过 |

测试去重合计 **104 项通过，0 失败、0 忽略**，不是全量 workspace/System/product E2E。
最初错误过滤器 `internal::secure_direct::v2_product_tests` 匹配 0 项，已用上表的
`v2_product_tests` 实际执行 19 项，不把零选择计为覆盖。群夹具最初断言了不符合其前置条件的
错误分类，按真实缺少加密准备状态修正预期后通过；生产权限或错误分类没有改变。

负向对照：将同样回归测试放入修复前 `3ef59f2b` 快照，网络三项和群恢复三项分别全部失败，
复现 Direct 越界、空闲/持续帧漏检、提示不执行、重启不续跑和 blocking 空闲不推进。

额外执行 `cargo check --manifest-path … -p awiki-im-core --locked --no-default-features
--features http` 失败。修复前后均为 **886 项错误**，错误标题与次数完全一致，主要为既有
SQLite 条件编译边界；不是本轮回归，也不标为通过。本轮未扩大修复该配置。
原始日志、解析来源、去重测试名及前后对照 JSON 在工作区 `tmp/core-review-fixes-20261024/`，
不提交缓存、临时源码、二进制或凭据，不生成 HTML。

## 安全检查、覆盖归属与未执行门禁

- Direct 在入口、换路及路由使用处保持禁用应用代理；异步探测完成时核验策略/代次。
  探测仍无业务头或体，不增加未知结果写请求重试；401/403 和协商错误语义保留。
- WS 检测仅触发现有受控重连和补拉，不退出账号、不重建身份状态、不放松 TLS。
- 群提示仍仅来自已认证 Home、精确目标身份；内容不能充当群成员或密钥变更权威，
  不产生聊天投影、解密或 P6 ACK。恢复失败保留 journal 和退避，不自动授权其他设备。
- 已审阅 System 的 Sync V2 dirty-hint/recovery、V3 lanes/observer 及 App 现有多设备/重启覆盖。
  本次精确故障位于 Core transport 和后台调度，新增回归归属 Core；没有新服务 wire 行为。
  原有跨服务覆盖仍需匹配候选，不能以历史服务端 901 项结果证明本次 Core 已生效。
- App 的[候选检查清单](../../../awiki-me/tests/computer-use/desktop-network-private-chat.md)
  补齐关闭代理后的 Upgrade 失败、探测/握手中切换、空闲/繁忙 WS 与单提示群恢复。
  本次未运行远端 System、产品 E2E 或真机检查：没有包含这次修复的新候选，且本轮不打包部署。
- Windows 原生代理/PAC/网络变化、macOS 真实系统配置、真实群恢复成功链路和前台私聊延迟
  仍是候选门禁。loopback 和缺前置状态的 repair 执行夹具不能替代这些结果。

源码层信任边界检查通过；现场性能与跨平台发布验收未完成。之前交付的 App
`0.1.40+52` 使用旧 Core `3ef59f2b`，不会因本轮 PR 推送自动包含此次修复。
