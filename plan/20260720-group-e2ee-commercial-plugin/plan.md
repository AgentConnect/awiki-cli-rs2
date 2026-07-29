# 计划：群组端到端加密商业插件化

状态：待执行
文档目录：`plan/20260720-group-e2ee-commercial-plugin/`
创建日期：2026-07-20
目标版本：待排期确认
计划负责人：待指定
恢复执行位置：从步骤 01 开始；任何实现前先复核本计划和权威架构文档。

## 1. 执行摘要

- 本计划把官方 Group E2EE 实现从公开发行物和公开源码依赖图中抽离。
- Community 版本继续提供普通群组、消息、附件、同步和 realtime，但不具备 Group E2EE 能力。
- Enterprise 版本基于同一公开 Core，通过静态链接私有 Rust provider 获得 Group E2EE。
- 插件形态采用“编译时静态链接 + 运行时 trait 注入”，不采用 `.so`、`.dylib` 或 sidecar。
- `im-core` 继续拥有 canonical Group identity、membership、auth、transport 和消息投影。
- 私有 provider 拥有 OpenMLS 运算、MLS 私有状态、KeyPackage 和 epoch 状态迁移。
- 公开高层 API 保持兼容；无 provider 时返回稳定、脱敏、fail-closed 的 capability error。
- Community 和 Enterprise 使用两套明确的构建目标与发行通道，不能仅靠 license flag 区分。
- 推荐首期只抽离 MLS 引擎和存储，不抽离全部 P6 编排，以降低长期维护成本。
- 若以后必须隐藏完整 P6 编排，应另立第二阶段计划，不在本次执行中扩大 SPI。

## 2. 目标

### 2.1 产品目标

- Community 官方二进制不支持创建、维护、发送或解密 Group E2EE 会话。
- Enterprise 官方二进制继续支持现有 Group E2EE 产品能力。
- 两个版本共享普通群组、消息、附件、同步和 UI-facing DTO。
- Enterprise 不维护 `im-core`、CLI 或 Flutter SDK 的长期私有 fork。
- 发布、安装和运行时部署形态尽量与当前单二进制模式一致。

### 2.2 工程目标

- 将 Group E2EE 行为选择从散落的 Cargo feature 收敛到单一 provider capability。
- 将 `GroupMlsProvider` 提升为可由私有 crate 实现的窄 SPI。
- provider 按 `owner_identity_id + device_id` 创建和隔离。
- Community 构建不再启用 `anp/mls` 和 OpenMLS 依赖。
- Enterprise 构建漏链接 provider 时在 Core open 或发行门禁阶段失败。
- required security 请求必须在远端副作用前确认本地 provider 可用。
- history、sync 和 realtime 在无 provider 时采用统一密文处理规则。

### 2.3 完成标准

- `cargo tree` 证明 Community 依赖图不含 `anp/mls`、OpenMLS 和私有 provider。
- `cargo tree` 证明 Enterprise 依赖图包含正确版本的私有 provider。
- Community 的 Group E2EE 正向操作全部稳定返回 unsupported capability。
- Community 收到 Group E2EE 密文时不会明文降级、错误投影或泄露 raw artifact。
- Enterprise 现有 create/add/remove/leave/send/read/realtime/repair 场景通过。
- 旧 MLS 私有状态可以被 Enterprise 新 provider 原路径打开或受控迁移。
- canonical conversation、membership 和 committed projection 不变量保持成立。
- CLI、Flutter facade 和公开 Rust DTO 没有私有 provider 类型泄露。
- 最终完整系统测试按仓库要求在 `../awiki-system-test` 执行并记录证据。

## 3. 必须遵守的权威约束

- 实现前必须阅读 `docs/architecture/im-core-sdk-architecture.md`。
- 实现前必须阅读 `docs/api/im-core-public-api.md`。
- 实现前必须阅读 `docs/flutter-sdk/awiki-im-core-flutter-sdk.md`。
- Group E2EE public API 仍只表达产品意图，不暴露 wire 或 crypto internals。
- `im-core` 继续拥有 auth retry、target resolution、local owner binding 和 projection。
- 插件不得根据 display name、Handle 文本或 DID 猜测 canonical conversation identity。
- 插件不得修改 `conversation_registry`、`conversation_summaries` 或 membership truth。
- Group send 仍必须由 Core 解析 canonical conversation 到权威 storage route。
- remote message 继续作为不可信输入处理。
- missing、malformed 或 conflicting security profile 必须 fail closed。
- committed patch 只能在 SQLite authoritative projection 成功后发出。
- public diagnostics 不得输出消息正文、密钥、KeyPackage、Welcome、Commit 或 provider path。
- Group MLS private state 必须保持 `owner_identity_id + device_id` 隔离。
- 公开发现保持 disabled，除非另有独立安全评审批准。
- 不修改 Direct E2EE 策略，不提供动态插件 ABI，也不把 MLS 私有状态迁移到服务端。

## 4. 开源与私有边界

### 4.1 保持开源的内容

- `ImCore`、`ImClient` 和现有高层服务 API。
- canonical conversation identity 和 Group DID 处理。
- Group membership、profile、policy 和普通群组生命周期。
- auth/session、DID origin proof 和 HTTP/RPC transport。
- reliable sync、realtime runner 和 committed local projection。
- Group E2EE 高层 request mode、status 和 redacted result DTO。
- P6 公共协议模型、方法名、错误码和互操作 wire contract。
- provider SPI、Null provider 和 conformance contract。
- Community fail-closed 行为和负向测试。

### 4.2 移入私有仓库的内容

- OpenMLS 具体依赖和 cryptographic operations。
- `NativeAnpMlsProvider` 的正式实现。
- MLS private state store、schema 和 migration。
- KeyPackage private material 和签名密钥状态。
- Welcome、Commit、Proposal 和 epoch state 的处理实现。
- encrypt、decrypt、finalize、abort 的具体实现。
- provider 的安全加固、性能优化和私有测试向量。
- Enterprise provider assembly 和商业构建配置。

### 4.3 首期明确不移动的内容

- Core 对 Group Host 的 authenticated RPC orchestration。
- Core 对 group snapshot 和 membership 的权威读取。
- Core 对 decrypted message 的 canonical projection。
- Core 的 reliable checkpoint 和 runtime patch 处理。
- Core 的 redacted secure status DTO 映射。
- Core 的 Group E2EE 服务 capability 检查。

### 4.4 边界取舍

- 首期边界能让 Community 官方版本没有可工作的 MLS 引擎。
- 首期仍公开一部分 P6 编排，因此不是隐藏所有协议流程。
- 该取舍显著减少 SPI host port 和私有 Core internals 暴露。
- 若完整编排也必须闭源，预计需要额外 30 到 45 人日。
- 完整编排私有化会扩大 SPI，并提高后续每次消息改动的同步成本。
- 推荐在首期稳定运行后再基于实际商业价值决定是否继续抽离。

## 5. Provider SPI 设计

### 5.1 Factory

- 新增 `GroupMlsProviderFactory: Send + Sync`。
- factory 提供 provider descriptor。
- factory 按 owner/device scope 打开 provider。
- factory 不接收完整 `ImClient`。
- factory 不接收 bearer token 或 DID private key。
- factory 不接收任意业务 SQLite connection。
- factory 只接收专用 storage context 和已验证 identity scope。

### 5.2 Scope

- `owner_identity_id` 是稳定所有者主键。
- `credential_did` 是当前 credential snapshot，不是所有者主键。
- `device_id` 必须稳定并参与 provider state 隔离。
- 新状态不得依赖隐式全局 default identity。
- legacy `default` device scope 仅作为受控兼容输入处理。
- group DID 由每个 operation 显式传入并由 Core 预校验。

### 5.3 Descriptor

- `spi_major` 表示不兼容 SPI 版本。
- `spi_minor` 表示向后兼容能力扩展。
- `implementation_version` 仅用于内部构建和诊断。
- `protocol_profiles` 声明支持的 P6 profile。
- `state_schema_version` 声明 provider 私有状态版本。
- descriptor 不得包含文件路径、密钥标识或 license secret。

### 5.4 Provider operation

- SPI 保留现有 generate/prepare/finalize/abort/welcome/notice/encrypt/decrypt/status typed operation 集合。

### 5.5 Error contract

- provider 返回稳定 error code 和脱敏 message。
- owner scope mismatch 映射为 invalid input 或安全不变量错误。
- missing state 映射为 local state unavailable。
- incompatible SPI 映射为 unsupported/incompatible capability。
- corrupt state 不得自动创建新空状态覆盖。
- raw OpenMLS error 不直接穿透 public API。
- error 的 `Debug` 和 `Display` 不得包含 artifact 或路径。
- retryable 属性由稳定 code 决定，不能解析 message 文本。

## 6. Core 注入和生命周期

- 在 `ImCoreOpenOptions` 增加 Rust-only runtime extensions。
- extensions 持有可选 `Arc<dyn GroupMlsProviderFactory>`。
- 为 `ImCoreOpenOptions` 实现手写、脱敏的 `Debug`。
- 增加 `with_group_mls_provider(...)` builder。
- 增加本地 capability policy：`Disabled`、`Optional`、`Required`。
- Community 默认使用 `Disabled` 或 `Optional + None`。
- Enterprise 必须使用 `Required + Some(factory)`。
- `Required` 且 factory 缺失时 Core open 失败。
- descriptor incompatible 时 Core open 失败。
- provider 私有状态可以延迟到 identity bind 时打开。
- 每个 `ImClient` 缓存 identity-scoped provider。
- provider open 失败不得回退到空 provider。

## 7. Community fail-closed 行为矩阵

| 场景 | Community 期望行为 | 是否允许远端副作用 |
|---|---|---|
| secure group create | `unsupported_capability("group-e2ee")` | 否 |
| secure add member | stable unsupported | 否 |
| secure remove member | stable unsupported | 否 |
| secure leave | stable unsupported | 否 |
| Group E2EE send text | stable unsupported | 否 |
| Group E2EE send payload | stable unsupported | 否 |
| Group E2EE send attachment | 上传前失败 | 否 |
| group secure status | `Unavailable + Unsupported` | 只允许必要的普通读取 |
| group secure repair | stable unsupported | 否 |
| required profile + plain send | 拒绝，禁止降级 | 否 |
| encrypted history row | locked/redacted projection | 允许普通 history read |
| encrypted sync row | durable opaque/backlog 后推进 | 允许同步读取 |
| encrypted realtime message | 不发 plaintext authoritative patch | 允许接收通知 |
| Group E2EE notice | 忽略或持久化受控 pending，不执行 MLS | 否 |
| secure attachment download | stable unsupported | 不下载密文对象或不解密 |
| DID rebind awaiting P6 | 保持 paused，报告 capability 缺失 | 不伪造完成 |

## 8. 构建和发行模型

### 8.1 Community

- 构建公开 `awiki-cli` binary。
- 构建公开 `im-core-dart` native artifacts。
- 不启用 `anp/mls`。
- 不包含 OpenMLS dependency graph。
- 不包含 Enterprise provider symbol 或资源。
- 发行脚本验证“没有 Group E2EE engine”。

### 8.2 Enterprise

- 构建薄的 Enterprise CLI assembly。
- assembly 调用共享 CLI library 并注入 provider factory。
- Flutter 使用 Enterprise native artifact assembly。
- Enterprise Dart API 尽量复用相同 package 和 generated DTO。
- native artifact 内部静态链接 provider。
- 发行脚本验证 provider descriptor 和 feature graph。
- 产物使用独立 channel、artifact name、签名和 SBOM。

### 8.3 防误发门禁

- Community CI 对私有 crate 名称做 dependency deny check。
- Community CI 对 OpenMLS crates 做 dependency deny check。
- Enterprise CI 要求 provider descriptor 存在。
- Enterprise smoke test 要求 Core `Required` policy open 成功。
- 发行物生成 SBOM 并对两类 artifact 做差异检查。
- Community 和 Enterprise 不共用同一个上传目录或 release job token。
- 不允许通过运行时 license flag 把 Enterprise binary 当 Community 发布。

## 9. 分步执行计划

### 步骤 01：冻结边界和更新权威文档

- 状态：`pending`。
- 复核三份 canonical Core/SDK 文档。
- 更新 `docs/architecture/im-core-sdk-architecture.md` 的 provider ownership。
- 更新 `docs/api/im-core-public-api.md` 的 capability 和 unsupported 语义。
- 更新 Flutter SDK 文档的 Community/Enterprise native capability。
- 更新 `docs/architecture/group-e2ee-operations.md` 的静态 provider 模型。
- 明确 P6 public contract 与 private implementation 的边界。
- 明确 Community encrypted ingress 行为。
- 评审后再开始代码修改。
- 预计工作量：3 到 4 人日。

### 步骤 02：建立 SPI 和 Null provider

- 状态：`pending`。
- 把 typed operation DTO 从 OpenMLS implementation 解耦。
- 定义 `GroupMlsProviderFactory`。
- 定义 provider descriptor、scope、storage context 和 error。
- 将现有 `GroupMlsProvider` 调整为可由外部 crate 实现。
- 增加 Null/Unavailable provider。
- 增加 SPI contract tests。
- 验证 SPI 不依赖 CLI、Dart 或 App DTO。
- 验证所有 Debug/Display 输出脱敏。
- 预计工作量：5 到 7 人日。

### 步骤 03：把 provider 注入 Core

- 状态：`pending`。
- 扩展 `ImCoreOpenOptions`。
- 增加 capability policy 和 builder。
- 在 `ImCoreInner` 保存 factory。
- 在 `ImClient` 按 identity/device 延迟打开 provider。
- 处理 blocking 和 async 调用共享同一 provider。
- 增加 missing、incompatible、open failed 测试。
- 更新 Dart mapping，使 Rust-only extension 不进入 FFI DTO。
- 保持现有 vault options 行为和脱敏 Debug。
- 预计工作量：5 到 7 人日。

### 步骤 04：收敛 groups/messages/secure 调用点

- 状态：`pending`。
- 用 runtime provider lookup 替换业务路径的 feature 分支。
- secure create 在普通 group.create 副作用前完成 provider preflight。
- add/remove/leave 在 P4 副作用前完成 provider preflight。
- text/payload/attachment send 统一通过 provider lookup。
- secure status/repair 使用相同 capability 状态。
- 保留现有 service capability 检查和 redacted result。
- 删除 `native_provider_for_client` 的硬编码调用。
- 补齐每个入口的 no-side-effect 测试。
- 预计工作量：7 到 10 人日。

### 步骤 05：加固 history/sync/realtime 无插件路径

- 状态：`pending`。
- 定义 encrypted wire record 的 Community 存储策略。
- history 不再对 Group E2EE projection 做 no-op。
- realtime 不再原样透传 Group E2EE notification。
- sync checkpoint 只在 opaque record/backlog 成功持久化后推进。
- 不向 conversation timeline 发出伪 plaintext patch。
- locked/redacted message DTO 不包含 raw cipher object。
- attachment manifest 不泄露 object key 或 nonce。
- DID rebind awaiting P6 在无 provider 时保持 paused。
- 增加重启和 checkpoint convergence 测试。
- 预计工作量：7 到 10 人日。

### 步骤 06：迁移正式 provider 到私有 crate

- 状态：`pending`。
- 在私有仓库创建 `awiki-group-e2ee-enterprise`。
- 移入 OpenMLS operations 和 storage implementation。
- 实现新的 factory 和 provider SPI。
- 保留 current owner/device scope 校验。
- 保持旧 state path 和 schema 可读。
- 为 legacy state 增加只读 inspection 和受控 migration 测试。
- 从公开依赖图移除 `anp/mls`。
- 从公开仓库删除正式 native provider 实现。
- 使用 conformance suite 验证行为等价。
- 预计工作量：7 到 10 人日。

### 步骤 07：拆分 CLI 构建目标

- 状态：`pending`。
- Community `awiki-cli` 不注入 provider。
- 私有 assembly crate 注入 Enterprise provider。
- CLI 解析层继续接受稳定 secure 输入以保持脚本兼容。
- Community help 可隐藏商业 secure group 命令。
- 直接调用隐藏参数时返回 stable unsupported。
- Enterprise help 和 command catalog 显示支持的高层命令。
- 更新 release feature graph 检查。
- 增加 Community/Enterprise CLI contract tests。
- 预计工作量：3 到 5 人日。

### 步骤 08：拆分 Flutter native artifact

- 状态：`pending`。
- 确定公开 Dart package 是否承载两类 native artifact。
- 保持 Dart DTO 和 FRB API 形状一致。
- Community native library 不链接 provider。
- Enterprise native assembly 注入 provider。
- 更新 Linux、Android、iOS 和 macOS 构建脚本。
- 验证 iOS static library/XCFramework 符号完整。
- 验证 Android ABI 和 Linux bundled library。
- 执行 codegen check 和各平台 smoke test。
- 预计工作量：5 到 8 人日。

### 步骤 09：双版本测试和安全审查

- 状态：`pending`。
- 运行 Community 全量 Rust tests。
- 运行 Community Group E2EE negative tests。
- 运行 Enterprise provider conformance tests。
- 运行 Enterprise Group E2EE focused tests。
- 运行 state compatibility 和 migration tests。
- 审查 dependency tree、SBOM、symbols 和 public output。
- 审查 canonical identity、membership 和 checkpoint 不变量。
- 审查 raw artifact、path、token 和 plaintext 泄露。
- 修复全部高、中严重度发现后进入系统测试。
- 预计工作量：6 到 9 人日。

### 步骤 10：系统测试、发行门禁和收尾

- 状态：`pending`。
- 在 `../awiki-system-test` 同步 Community negative E2E。
- 在 `../awiki-system-test` 同步 Enterprise positive E2E。
- 使用 Enterprise artifact 执行 Group E2EE remote 场景。
- 使用 Community artifact 验证稳定 unsupported 和无降级。
- 执行 remote `awiki.info` 完整系统测试。
- 实际命令必须设置 `AWIKI_SYSTEM_TEST_MODE=remote` 并使用 `awiki.info` HTTPS/WSS endpoint。
- 记录通过、失败、跳过数量和原因。
- 记录关键环境变量、artifact version 和 commit。
- 更新安装、发布、兼容性和安全文档。
- 完成最终 release dry-run 和回滚演练。
- 预计工作量：5 到 7 人日。

## 10. 工作量估算

### 10.1 推荐范围

| 工作包 | 估算人日 | 主要角色 |
|---|---:|---|
| 架构和权威文档 | 3-4 | Core/安全 |
| SPI 和 Null provider | 5-7 | Rust Core |
| Core 注入和生命周期 | 5-7 | Rust Core |
| groups/messages/secure 收敛 | 7-10 | Rust Core |
| history/sync/realtime fail-closed | 7-10 | Rust Core |
| 私有 provider 抽离和状态兼容 | 7-10 | Crypto/Rust |
| CLI 双版本装配 | 3-5 | CLI/Rust |
| Flutter native 双版本装配 | 5-8 | Rust/Flutter |
| 测试、安全审查和系统集成 | 11-16 | Core/QA/安全 |
| 合计 | 53-77 | 多角色协作 |

### 10.2 推荐排期

- 两名熟悉 Rust/Core 的工程师全职投入。
- 一名 Flutter/发行工程师在步骤 08 和步骤 10 介入。
- 一名安全或 crypto reviewer 在步骤 01、06、09 介入。
- 一名 QA/系统测试负责人在步骤 09、10 介入。
- 推荐日历周期为 6 到 8 周。
- 关键路径是步骤 01 → 02 → 03 → 04 → 05 → 06 → 09 → 10。
- CLI 和 Flutter 装配可在步骤 03 接口稳定后部分并行。
- 若只能单人执行，预计需要 11 到 15 周日历时间。
- 估算已经包含正常审查和缺陷修复，但不包含大规模服务端改造。

### 10.3 裁剪场景

- 只支持 Enterprise CLI、暂不支持 Flutter：约 43 到 62 人日。
- 保留 Flutter 但不改变 CLI help 可见性：可减少约 1 到 2 人日。
- 不迁移旧 MLS state、确认无真实用户数据：可减少约 3 到 5 人日。
- 同时把完整 P6 orchestration 移入私有 crate：额外增加约 30 到 45 人日。
- 增加官方服务端 tenant entitlement：另估约 8 到 15 人日，不计入本计划。
- 增加动态插件热插拔：至少额外增加 25 到 40 人日，不建议。

### 10.4 估算置信度

- 当前置信度为中等。
- 主要不确定性是旧 MLS state 的真实数据兼容要求。
- 第二个不确定性是 Enterprise Flutter native assembly 的最终形态。
- 第三个不确定性是 system-test 是否已有双 artifact 注入能力。
- SPI 仅抽离 crypto/storage 时，估算更接近下界。
- 若执行中发现 orchestration 必须一并闭源，必须暂停并重估。

## 11. 风险和缓解

| 风险 | 影响 | 缓解 |
|---|---|---|
| 无 feature 路径原样透传密文 | 安全和投影错误 | 步骤 05 先定义统一 ingress gate |
| secure preflight 晚于 P4 副作用 | 产生半完成群状态 | provider availability 必须前置 |
| SPI 过宽 | 长期维护成本上升 | 首期只暴露 MLS typed operations |
| SPI 过窄 | 私有 provider 无法完成迁移 | 增加专用 storage context，不暴露 Core internals |
| Community 误链接私有实现 | 闭源代码泄露 | dependency deny、SBOM、独立 release job |
| Enterprise 漏链接 provider | 线上 capability 缺失 | `Required` policy 和启动 smoke |
| 旧 MLS state 不兼容 | 用户无法解密历史/继续群 | 原路径兼容、fixture 和迁移演练 |
| Flutter assembly 重复 generated code | 维护和符号漂移 | 共用 facade/DTO，只分 native 装配 |
| provider 错误泄密 | 暴露路径或 artifact | 稳定 error mapping 和 redaction tests |
| 第三方重实现 provider | 商业授权绕过 | 官方服务端 entitlement，不能依赖闭源本身 |
| Core 与 provider 版本漂移 | 状态损坏或行为分叉 | SPI major、descriptor、Cargo.lock |
| 完整编排闭源诉求中途加入 | scope 和排期失控 | 暂停执行、更新计划、重新估算 |

## 12. 验收清单

- [ ] 权威架构/API/Flutter 文档已更新并通过评审。
- [ ] provider SPI 是外部私有 crate 可实现的窄接口。
- [ ] `ImCoreOpenOptions` 支持脱敏 provider 注入。
- [ ] Community 无 provider 路径 fail closed。
- [ ] Enterprise `Required` policy 能检测漏链接。
- [ ] groups/messages/secure 不再硬编码 native provider factory。
- [ ] history/sync/realtime 无 provider 路径不原样透传密文。
- [ ] Community dependency graph 不含 OpenMLS。
- [ ] Enterprise dependency graph 包含固定版本 provider。
- [ ] 旧 MLS state compatibility tests 通过。
- [ ] canonical identity 和 membership invariants 通过。
- [ ] committed projection 和 checkpoint tests 通过。
- [ ] CLI Community/Enterprise contract tests 通过。
- [ ] Flutter native artifact smoke tests 通过。
- [ ] public DTO、logs、diagnostics redaction 审查通过。
- [ ] Community negative E2E 通过。
- [ ] Enterprise Group E2EE positive E2E 通过。
- [ ] remote `awiki.info` 完整系统测试完成并记录结果。
- [ ] Community 和 Enterprise release dry-run 均通过。

## 13. 执行台账

状态取值：`pending`、`in_progress`、`review`、`blocked`、`done`。

| 步骤 | 状态 | 负责人 | 预计人日 | 开始 | 完成 | 提交/PR | 验证证据 |
|---|---|---|---:|---|---|---|---|
| 01 | pending | 待定 | 3-4 | | | | |
| 02 | pending | 待定 | 5-7 | | | | |
| 03 | pending | 待定 | 5-7 | | | | |
| 04 | pending | 待定 | 7-10 | | | | |
| 05 | pending | 待定 | 7-10 | | | | |
| 06 | pending | 待定 | 7-10 | | | | |
| 07 | pending | 待定 | 3-5 | | | | |
| 08 | pending | 待定 | 5-8 | | | | |
| 09 | pending | 待定 | 6-9 | | | | |
| 10 | pending | 待定 | 5-7 | | | | |

## 14. 执行协议

- 每次开始或恢复前阅读本计划、三份权威文档和当前 `git status`。
- 从第一个非 `done` 步骤继续。
- 同一时间只允许一个步骤处于 `in_progress`。
- 每步必须同步修改对应单元测试、系统测试和 E2E 测试。
- 跨仓库测试必须在同一任务中同步修改权威测试仓库。
- 每步完成代码后先审查，再验证，再提交。
- 提交后在执行台账记录 commit/PR 和实际验证证据。
- 改变 SPI、ownership、fail-closed 或 migration 边界前先更新本计划。
- 出现完整 orchestration 私有化诉求时必须重新估算，不静默扩大范围。
- 出现服务端 entitlement 诉求时创建独立跨仓库计划。

## 15. 开放决策

- 私有仓库最终名称和访问控制策略。
- Enterprise CLI artifact/package 名称。
- Enterprise Flutter 是复用同一 Dart package 还是使用独立 package channel。
- provider contract DTO 放在 `im-core` 还是独立 SPI crate。
- legacy `default` device scope 的保留期限。
- Community 对 encrypted history 使用 locked row 还是仅 durable backlog。
- Community CLI help 是否隐藏所有 secure group 命令。
- Enterprise provider version 是否进入内部 doctor 输出。
- 旧 MLS state 是否已有需要真实迁移的生产数据。
- 官方服务端是否同步增加 tenant entitlement。

上述决策中，SPI crate 位置、encrypted history projection 和旧状态兼容必须在步骤 01 结束前确定。

## 16. 计划变更记录

| 日期 | 变更 | 原因 | 影响步骤 | 是否需评审 |
|---|---|---|---|---|
| 2026-07-20 | 创建群组端到端加密商业插件化执行计划。 | 用户要求整理执行方案和工作量。 | 全部 | 是 |
