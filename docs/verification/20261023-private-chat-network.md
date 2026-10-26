# 私聊延迟与桌面系统代理源码验证记录

日期：2026-10-23。三个仓库沿用 `test/singapore-release-20261022`；保留已有群聊修复。
本轮交付源码、合同、夹具和定向验证，没有重新打包 App、部署服务或发布 SDK。
**源码修复已完成；现场延迟和跨平台原生验收尚未通过，仍是下一候选的明确门禁。**

## 实现与提交

| 仓库／提交 | 内容 |
| --- | --- |
| message-service `47fb0db` | P5 设备出口完整适配 method、params、sync schema；Legacy 保留 envelope，P6 不变 |
| awiki-cli-rs2 `700712fa` | 已认证 Home 的合法旧式 P5 提示转为 sync-only；目标 DID/device 精确校验，不直接解密或 ACK |
| awiki-cli-rs2 `346e1cc7` | NetworkRuntime、系统解析、路径探测、连接池、回退、流式 native HTTP、类型化 Dart 绑定和诊断 |
| awiki-me `10fd390f` | 默认开启的本机代理开关、登录前入口、共享传输、头像适配、中文／英文和布局测试 |
| awiki-cli-rs2 `54fb83db` | 对方优先四路 fan-out、逐设备有序票据、操作内 DID 复用、发布确认短缓存、后台持久会话确认 |
| awiki-me `9523e4a0` | 有界且不含身份／正文的同步调度阶段记录 |

App 已有的 active-sync 合并／下一轮补拉机制继续使用，未另造同步入口。连续操作只串行准备和准入，
不会等待自己的慢副本完成；逐设备票据保证顺序，取消中间任务不会使后来任务越过前驱。
每个副本保持独立台账、固定操作 ID、固定已准备密文和部分完成合同。

发布确认绑定本 Core 租户、owner、device、授权代次及 immutable publication；同时按单调时钟五分钟
与 signed-prekey 的 UTC 到期裁剪。轮换、代次变化、明确授权失效清除确认，失败不缓存，重启重新确认。
缓存锁不覆盖身份提供方或网络调用。没有加密协议、业务消息格式、账号数据或数据库迁移。

## 网络与安全检查

- macOS/Windows 使用当前用户系统解析；支持静态代理、SOCKS5、例外和已启用的 PAC/自动配置，不猜端口、不主动开启发现。
- 代理连接／隧道／TLS 探测不携带业务头或体，750ms 后探测直连。探测与实际连接共享一轮十秒连接预算；成功路径至多五分钟，失败代理跨来源冷却三十秒，相同来源合并探测。
- 系统配置在新请求时重读；原生路由／接口监视与宿主失效接口清除缓存。WS 在策略变化后受控重连，原协调器立即补拉。
- 写请求提交结果未知时不透明重发；只允许明确提交前连接失败或受限只读换路。401/403、ANP 业务错误、参数／协议错误不改写为代理失败。
- 连接池没有默认账号头或 cookie store，逐请求认证隔离。不同 Core 的显式 CA 保持隔离，Windows 保留原摘要校验的 Mozilla bundle。匿名固定 DNS/IP 发现保持专用直连安全路径。
- 原始 native HTTP 返回 head 与分块 body，取消可中止正在等待读取的响应；附件原有流式时限、头像大小／重定向／格式校验保留。移动端不增加代理界面或配置。
- Rust 最多 128 条 UTC／不透明操作摘要／固定 stage／elapsed_ms 记录，App 最多 128 条调度阶段记录；不记录正文、OTP、密钥、认证头、代理地址或密码。

本地信任、恢复、幂等、取消与通知所有权检查通过；**发布安全门禁尚缺真实兼容、原生代理与现场证据**，
不得把上述本地结果作为公开启用或性能验收。没有变更服务端认证含义、租户边界或证书验证。

## 实际验证

环境：macOS 27.0.1、Apple Silicon arm64；Rust 1.88.0 host 为 `aarch64-apple-darwin`。
Flutter tester 的目录名虽是 `darwin-x64`，实际 Mach-O 为 arm64，未以目录名推断平台。
未进行 App Debug/Release 构建，也没有 Windows 运行证据。

Core 原工作区固定要求 ANP 1.0.7，但邻接源码包声明 1.0.6，直接 cargo 命令在解析阶段失败。
因此复制本轮源码到 `/tmp/awiki-private-network-check`，只在隔离 manifest 移除 ANP/Identity 的
邻接 path，使用固定 registry ANP 1.0.7、Identity 0.2.6；Core/Dart facade 使用本轮源码。
原工作区依赖来源未切换。新增网络依赖锁保留原本地来源条目；这不代表候选 SDK 已发布。

以下 Rust 命令在上述隔离 manifest 执行，均使用 `cargo +1.88.0`；非全量 workspace／System／产品 E2E。

| 命令／范围 | 本轮最终结果 |
| --- | --- |
| message-service：`cargo test -p im-runtime --lib session_registry::tests` | 14 通过；含完整 P5 协商矩阵和 P6 出口回归 |
| `cargo test -p awiki-im-core --features secure-direct --lib network::` | 10 通过；无代理、正常代理、挂起、407、跨来源冷却、并发合并、直连不可用、认证／本地校验错误、响应丢失、CA 隔离、十秒预算 |
| `cargo test -p awiki-im-core --features secure-direct --lib internal::secure_direct::v2_` | 45 通过；含四路、部分失败、固定密文恢复、确认持久化、取消票据、预密钥、ratchet／replay／Vault 原子性 |
| `cargo test -p awiki-im-core --features secure-direct --lib realtime::` | 45 通过；含旧 P5 提示、错误 DID/device、冻结旧路径、协商、实时投影 |
| 同一 Core 参数，分别过滤 `central_dispatcher`、`timed_out_task`、`foreground_wait`、`sync_dispatcher` | 各 1 通过；分发额度、超时占位、滞后通知补偿、确认唤醒竞态 |
| `cargo test -p im-core-dart --features secure-direct --lib api::network::tests` | 1 通过；head 先返回、分块读取、取消正在等待的 body |
| `cargo check -p im-core-dart` | 通过；Core 既有 `messages/security.rs` 未使用参数警告保留 |
| App：`flutter test` 定向七个文件，见下文 | 194 通过 |
| App：新增传输适配器、原 HTTP 与同步协调器复验 | 72 通过，新增独立用例 2；与前行重叠 70，不重复计数 |
| App 受影响目录／文件 `dart analyze`；SDK native/web/model/生成 network API `dart analyze` | 均无问题 |
| 离线 System 合同：`tests/non_did/test_message_sync_v2_contract.py`、`test_message_sync_v3_contract.py`、`test_message_sync_foreground_trace.py` | 11 通过、失败 0、跳过 0，0.89 秒；service-independent，不连接 URL/DID/远端服务 |

定向七文件：`desktop_network_preference_test.dart`、`app_http_client_test.dart`、
`avatar/avatar_image_cache_test.dart`、`message_sync_coordinator_test.dart`、`settings_page_test.dart`、
`onboarding_page_test.dart`、`data/im_core/awiki_im_core_runtime_test.dart`。
复验为 `desktop_network_transport_test.dart`、`app_http_client_test.dart`、`message_sync_coordinator_test.dart`。
去重合计 **326 项通过**，不表示全量测试或现场验收。

Flutter 原生依赖 hook 过滤了 DEVELOPER_DIR，初次执行失败；使用临时 xcrun 包装器显式指向现有
Command Line Tools 后通过，没有接受／修改系统 Xcode 许可。Dart facade 既有群摘要测试重复字段
导致测试编译失败，已只删除两项重复初始化，群聊生产逻辑未改。初次传输迁移的头像 Fake 已按
package:http IOClient 的接口契约补齐，旧覆盖保留。全 App `dart analyze` 曾报告既有
`onboarding_mobile_controls_part.dart:16` 的 unused_element_parameter；本轮受影响目标分析无问题。

原始定向日志在工作区 `tmp/private-chat-network-20261023/`；未生成 HTML 报告。

## 兼容矩阵与尚未执行的门禁

| 服务／客户端组合 | 本地证据 | 候选／现场状态 |
| --- | --- | --- |
| 新服务 → Legacy 客户端 | 原 envelope／无新 sync hint 的服务单测 | 尚未运行真实旧客户端 |
| 新服务 → V2／V3 客户端 | method＋params＋schema 完整；超限回退与 dirty_lanes 冻结 | 尚未跨服务运行 |
| 旧服务 → 新 Core | 目标匹配旧 P5 仅触发补拉，不消费密文或 ACK | 尚未运行真实旧服务 |
| 旧服务 → 旧客户端 | 冻结旧路径未改 | 历史基线，本轮未运行 |
| 重复／乱序、同步中提示、重连／前台 | Core 与 App 相关单测通过 | 双设备候选门禁 |
| P6 公共通知出口 | 方法、正文与 delivery-context 的回归单测通过 | 群聊跨服务门禁 |
| macOS 原生代理／PAC／网络切换 | 本机编译及 loopback 夹具；未修改真实系统代理 | 必须真实配置验证 |
| Windows 静态／PAC／例外／路由变化／WS | 无 Windows 原生证据 | **未验证，不能以 Mac 或交叉编译替代** |
| 连续普通／加密私聊 3–5 秒、首次 5–10 秒 | 没有候选实测样本 | **未验收**；超过十秒必须有分段解释 |

已审阅 System 的 Sync V2/dirty-hint、Schema V3/lane、foreground trace 及 App 多设备／重启同步覆盖。
Schema V3 完整远端 suite 在 System 仓仍明确 active_blocked，本轮未放宽该门禁。
没有运行远端 System 或产品 E2E：本轮没有匹配新源码的候选，也未获准打包／部署。
不调用真实大模型，不准备被用户排除的测试。后续按
[桌面代理与私聊候选验证](../../../awiki-me/tests/computer-use/desktop-network-private-chat.md)
先验证服务／客户端兼容，再用本次手机与电脑对照。新加坡只运行必要服务与低频验证，
不进行重型构建或压测，保留磁盘与内存余量。
