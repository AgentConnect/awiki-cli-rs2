# aWiki-CLI src Module Map

更新日期：2026-05-26

本文档记录 `crates/awiki-cli/src` 当前文件系统下的模块数量和职责划分。统计包含本 `README.md` 文件。

## 数量统计

| 范围 | 数量 |
| --- | ---: |
| `src` 递归文件总数 | 135 |
| `src` 递归目录总数，包含 `src` 自身 | 16 |
| `src` 子目录总数，不包含 `src` 自身 | 15 |
| `src` 直属文件数 | 9 |
| `src` 直属文件夹数 | 11 |

## 顶层结构

当前 `src` 已经基本按新职责分成四块：

| 类别 | 模块 |
| --- | --- |
| CLI 壳 | `cli_shell`、`cli_parser`、`command_catalog`、`cli_output`、`cli_docs`、`cli_trace` |
| M-Core 适配 | `m_core_cli_adapter` |
| 本机宿主能力 | `host_runtime` |
| 工作区、迁移、诊断 | `workspace_config`、`workspace_upgrade`、`diagnostics` |

仍然带 `legacy` 字样的内容主要集中在 `workspace_upgrade::legacy_*` 和 `diagnostics::legacy_*`，职责是老数据迁移和只读诊断，不是默认业务路径。

## 顶层文件

| 文件 | 作用 |
| --- | --- |
| `README.md` | 本文档，记录 `src` 模块数量和职责。 |
| `lib.rs` | crate 的 public module 入口；当前只导出新的模块名，如 `cli_shell`、`m_core_cli_adapter`、`workspace_upgrade` 等，并 `pub use cli_shell::execute`。 |
| `main.rs` | 二进制入口；调用库里的 CLI 执行入口并处理进程退出。 |
| `build_info.rs` | 版本、构建信息、目标平台名等 `version` / `status` 需要的 metadata。 |
| `cli_http.rs` | CLI 自有 HTTP 模块入口；包装 HTTP client / profile 解析。 |
| `cli_output.rs` | CLI 输出 envelope、JSON / pretty 渲染、`ExitError` / 错误响应结构。 |
| `cli_shell.rs` | CLI 命令执行核心壳层；解析后的 command dispatch、上下文、handler 聚合。 |
| `cli_trace.rs` | `AWIKI_TRACE_TIMING` 之类的 CLI trace / timing 记录和输出。 |
| `durable_fs.rs` | 原子写、目录 fsync、跨平台耐久化文件写入 helper。 |

## 顶层文件夹

| 文件夹 | 直属文件数 | 作用 |
| --- | ---: | --- |
| `cli_docs` | 1 | CLI 内置帮助/文档主题。 |
| `cli_http` | 1 | HTTP client 具体实现。 |
| `cli_parser` | 1 | 参数解析、flag 解析、direct invocation gate。 |
| `cli_shell` | 18 | 各命令族 handler 和 CLI 壳层 helper。 |
| `command_catalog` | 1 | 命令 schema、metadata、audience、cutover 分类。 |
| `diagnostics` | 3 | `doctor` / debug 诊断，只读检查和 legacy 诊断 facade。 |
| `host_runtime` | 36 | 本机 runtime、listener service、bridge、host notify、Hermes / OpenClaw 宿主；另有 `host_runtime/hermes_bridge` 子目录。 |
| `m_core_cli_adapter` | 19 | CLI 参数/输出到 M-Core / `im-core` API 的薄适配层。 |
| `self_update` | 3 | CLI 自更新、版本检查、metadata cache。 |
| `workspace_config` | 2 | workspace 路径、配置读取/解析/写入。 |
| `workspace_upgrade` | 13 | workspace schema 升级、迁移、锁、备份、legacy 数据导入；另有 `legacy_identity`、`legacy_identity/auth`、`legacy_sqlite` 子目录。 |

## `cli_docs`

| 文件 | 作用 |
| --- | --- |
| `cli_docs/mod.rs` | `docs list` / `docs topic` 之类内置文档主题和查询逻辑。 |

## `cli_http`

| 文件 | 作用 |
| --- | --- |
| `cli_http/http.rs` | 底层 HTTP client：超时、proxy、CA bundle、请求/响应读取。 |

## `cli_parser`

| 文件 | 作用 |
| --- | --- |
| `cli_parser/mod.rs` | CLI argv 解析、全局/局部 flag 校验、unknown flag 行为、命令路径解析。 |

## `command_catalog`

| 文件 | 作用 |
| --- | --- |
| `command_catalog/mod.rs` | 命令目录和 schema 输出：命令树、flags、audience、cutover status、direct invocation policy。 |

## `diagnostics`

| 文件 | 作用 |
| --- | --- |
| `diagnostics/mod.rs` | `doctor` 诊断主逻辑：配置、workspace、身份、SQLite、服务可达性等只读检查。 |
| `diagnostics/legacy_identity.rs` | diagnostics 私有 legacy identity facade，只读复用 workspace upgrade 的旧身份扫描能力。 |
| `diagnostics/legacy_sqlite.rs` | diagnostics 私有 legacy SQLite facade，用于旧库只读扫描和 `debug db handle-history`；不提供 raw SQL 执行。 |

## `cli_shell`

| 文件 | 作用 |
| --- | --- |
| `cli_shell/debug_handlers.rs` | `debug` 命令 handler；保留受控迁移/诊断入口，如 import-v1、handle-history；raw SQL query 是 unsupported。 |
| `cli_shell/error_hints.rs` | 把底层 IO、权限、平台错误转换成 CLI 友好的 hint。 |
| `cli_shell/group_e2ee_handlers.rs` | group E2EE 命令壳层；当前多为 gated / unsupported 或转 M-Core 策略。 |
| `cli_shell/group_handlers.rs` | group create / join / member / list / update 等命令 handler，主要转 `m_core_cli_adapter`。 |
| `cli_shell/handle_helpers.rs` | handle 标准化、补全、输入边界 helper。 |
| `cli_shell/id_recover_handlers.rs` | `id recover` CLI handler，走 M-Core / `im-core` recovery 流程。 |
| `cli_shell/id_replace_did_handlers.rs` | `id replace-did` handler，计划/执行边界和 warning 输出。 |
| `cli_shell/mail_handlers.rs` | mail 命令 handler，转 `im-core` email API。 |
| `cli_shell/msg_handlers.rs` | msg send / inbox / history / mark-read / secure 等命令 handler。 |
| `cli_shell/page_handlers.rs` | page 命令 handler，转 `im-core` content/page RPC。 |
| `cli_shell/people_handlers.rs` | people / profile / contact / follow 类命令 handler。 |
| `cli_shell/runtime_handlers.rs` | runtime / listener / host notify / OpenClaw 等宿主命令 handler。 |
| `cli_shell/runtime_hermes_handlers.rs` | Hermes bridge / guide / setup / status 相关 CLI handler。 |
| `cli_shell/runtime_host_notify_refresh.rs` | host notify 配置变更后刷新/重启 listener 的 CLI 边界。 |
| `cli_shell/site_handlers.rs` | site 命令 handler，转 `im-core` site RPC。 |
| `cli_shell/unsupported.rs` | 统一 unsupported capability 错误和 stub handler。 |
| `cli_shell/update_handlers.rs` | self-update 命令 handler。 |
| `cli_shell/update_preflight.rs` | 普通命令执行前的版本检查/升级提示 preflight。 |

## `m_core_cli_adapter`

| 文件 | 作用 |
| --- | --- |
| `m_core_cli_adapter/mod.rs` | M-Core adapter 模块入口和公共薄壳 API 聚合。 |
| `m_core_cli_adapter/auth.rs` | CLI 身份/auth 上下文到 `im-core` auth scope/request 的转换。 |
| `m_core_cli_adapter/content.rs` | page/content 命令到 `im-core` content API 的映射。 |
| `m_core_cli_adapter/core.rs` | 构建 `im-core` client/runtime 的核心 glue。 |
| `m_core_cli_adapter/core_config.rs` | workspace config 到 `im-core` config 的字段映射。 |
| `m_core_cli_adapter/email.rs` | mail 命令 DTO、附件、收件人解析、结果渲染适配。 |
| `m_core_cli_adapter/error.rs` | `im-core` error 到 CLI `ExitError` / envelope 的映射。 |
| `m_core_cli_adapter/groups.rs` | group lifecycle / members / messages 到 `im-core` group API 的适配。 |
| `m_core_cli_adapter/identity.rs` | id register / bind / recover / profile / resolve / refresh 等到 `im-core` identity API 的适配。 |
| `m_core_cli_adapter/identity_replace_did_plan.rs` | replace-did 的 dry-run / plan DTO 构造。 |
| `m_core_cli_adapter/message_result.rs` | message send / inbox / history / mark-read 结果转换和兼容输出。 |
| `m_core_cli_adapter/messages.rs` | msg 命令主体适配：发送、收件箱、历史、附件、mark-read、secure 策略；direct send 返回本地 peer-scope `ThreadRef::Thread` 时仍按 direct delivery 渲染，并从 metadata 恢复目标 handle/DID。 |
| `m_core_cli_adapter/messages_tests.rs` | `messages.rs` 的 test-only 单元测试实现，避免大段测试夹在业务实现文件中。 |
| `m_core_cli_adapter/paths.rs` | workspace 路径到 `im-core` path struct 的映射。 |
| `m_core_cli_adapter/people.rs` | people / contact / follow / profile 类命令到 `im-core` API 的适配。 |
| `m_core_cli_adapter/realtime.rs` | realtime runner/event 到 host runtime 的适配边界。 |
| `m_core_cli_adapter/render.rs` | adapter 层统一成功/错误/plan 输出渲染。 |
| `m_core_cli_adapter/site.rs` | site 命令到 `im-core` site API 的映射。 |
| `m_core_cli_adapter/tests.rs` | adapter 内部单元测试。 |
| `m_core_cli_adapter/unsupported.rs` | adapter 侧 unsupported capability DTO / 错误。 |

## `host_runtime`

| 文件 | 作用 |
| --- | --- |
| `host_runtime/mod.rs` | host runtime 模块入口，聚合 bridge / listener / notify / Hermes / OpenClaw。 |
| `host_runtime/bridge.rs` | 本地 bridge endpoint、request / response framing、health probe。 |
| `host_runtime/hermes_bridge.rs` | Hermes bridge 高层配置、状态、setup / guide / service glue。 |
| `host_runtime/hermes_host_notify.rs` | Hermes host notification sink、签名、URL / secret 校验和发送。 |
| `host_runtime/host_notify.rs` | host notification event DTO、事件归一化、消息/群/邮件通知转换。 |
| `host_runtime/host_notify_sink.rs` | noop / log / file / OpenClaw / Hermes sink 的统一构造和 dispatch。 |
| `host_runtime/listener.rs` | listener runtime 总入口、状态合并、配置解析。 |
| `host_runtime/listener_bridge_connection.rs` | listener bridge 连接生命周期和 request 发送。 |
| `host_runtime/listener_bridge_dispatch.rs` | bridge request dispatch 到 listener / session 方法。 |
| `host_runtime/listener_bridge_runtime.rs` | bridge server runtime 启停和请求循环。 |
| `host_runtime/listener_connect_session.rs` | 为身份建立 realtime / session 连接。 |
| `host_runtime/listener_foreground.rs` | 前台 listener run，处理信号和 cleanup。 |
| `host_runtime/listener_identity_record.rs` | host runtime 自有 identity DTO，替代旧 `StoredIdentity`。 |
| `host_runtime/listener_identity_watch.rs` | 监听新身份/当前身份变化并启动 session。 |
| `host_runtime/listener_im_event_adapter.rs` | `im-core` realtime events 到 host notify / local notification / 状态更新的适配。 |
| `host_runtime/listener_json_helpers.rs` | listener JSON map / serialization helper。 |
| `host_runtime/listener_known_sessions.rs` | 已知 session 加载、启动等待、错误记录。 |
| `host_runtime/listener_launchd.rs` | macOS launchd service plist / status helper。 |
| `host_runtime/listener_local_notification_flush.rs` | 本地通知队列 flush 到当前身份/session。 |
| `host_runtime/listener_local_notifications.rs` | 本地通知队列文件读写。 |
| `host_runtime/listener_notification_consume.rs` | realtime notification channel 消费循环和 ping / 取消逻辑。 |
| `host_runtime/listener_service.rs` | listener 后台服务 install / start / stop / status / restart 跨平台计划。 |
| `host_runtime/listener_service_did.rs` | 从 listener/session 查询 message service DID。 |
| `host_runtime/listener_session_bootstrap.rs` | session bootstrap：选择身份、创建 session、等待 ready。 |
| `host_runtime/listener_session_lookup.rs` | 根据 DID / identity 查找 active session / record。 |
| `host_runtime/listener_session_methods.rs` | session 状态方法、连接/断开、secure RPC client 访问。 |
| `host_runtime/listener_session_state.rs` | session map、状态 snapshot、错误记录。 |
| `host_runtime/listener_shutdown_signal.rs` | SIGINT / SIGTERM 等 shutdown signal 等待和状态标记。 |
| `host_runtime/listener_supervisor_init.rs` | supervisor 初始化计划：打开 store、schema、host notify、remote client。 |
| `host_runtime/listener_supervisor_run.rs` | supervisor 主运行循环和断连/错误记录。 |
| `host_runtime/listener_supervisor_shutdown.rs` | supervisor shutdown 顺序：session、listener、notify sink、database。 |
| `host_runtime/listener_systemd.rs` | Linux systemd unit / status helper。 |
| `host_runtime/listener_windows_service.rs` | Windows service 配置/status helper。 |
| `host_runtime/openclaw_host_notify.rs` | OpenClaw host notify sink，route delivery 和请求构造。 |
| `host_runtime/openclaw_routes.rs` | OpenClaw route registry / config 解析、增删、probe。 |
| `host_runtime/openclaw_webhook.rs` | OpenClaw webhook URL / request / confirmation helper。 |

## `host_runtime/hermes_bridge`

| 文件 | 作用 |
| --- | --- |
| `host_runtime/hermes_bridge/route.rs` | Hermes route / env / config 解析、ensure route、deliver target 逻辑。 |
| `host_runtime/hermes_bridge/service.rs` | Hermes bridge 后台服务 install / start / stop / status / run。 |

## `self_update`

| 文件 | 作用 |
| --- | --- |
| `self_update/mod.rs` | self-update 主逻辑：检查 registry metadata、版本策略、strict disable、输出 decision。 |
| `self_update/cache.rs` | 版本 metadata cache 读写、fresh / stale 策略。 |
| `self_update/version.rs` | semver-ish 版本比较和 prerelease 处理。 |

## `workspace_config`

| 文件 | 作用 |
| --- | --- |
| `workspace_config/mod.rs` | workspace 路径、配置结构、配置解析、默认值、环境/flag override。 |
| `workspace_config/write.rs` | YAML 配置写入和局部字段 mutation。 |

## `workspace_upgrade`

| 文件 | 作用 |
| --- | --- |
| `workspace_upgrade/mod.rs` | workspace upgrade 模块入口和公开迁移 API。 |
| `workspace_upgrade/backup.rs` | 升级前备份 config、identity、SQLite 等。 |
| `workspace_upgrade/detect.rs` | 检测当前 workspace、legacy config、旧身份、旧 SQLite、schema version。 |
| `workspace_upgrade/fsutil.rs` | 迁移专用文件工具：copy tree、atomic write、路径检查。 |
| `workspace_upgrade/journal.rs` | upgrade journal 读写，用于恢复/幂等执行。 |
| `workspace_upgrade/lock.rs` | workspace upgrade 文件锁和旧锁兼容判断。 |
| `workspace_upgrade/meta.rs` | workspace schema metadata 读写。 |
| `workspace_upgrade/migration_v0_to_v1.rs` | v0 -> v1：legacy config / identity / SQLite 导入和当前 schema 初始化。 |
| `workspace_upgrade/migration_v1_to_v2.rs` | v1 -> v2：旧 listener / service / skill artifacts 清理。 |
| `workspace_upgrade/migration_v2_to_v3.rs` | v2 -> v3：K1 DID 替换/重绑相关迁移。 |
| `workspace_upgrade/settings.rs` | 旧 settings / config 解析和导入。 |
| `workspace_upgrade/types.rs` | upgrade context、detection、inspection、migration result 等类型。 |
| `workspace_upgrade/upgrader.rs` | migration plan、执行器、`upgrade_if_needed` 总控。 |

## `workspace_upgrade/legacy_identity`

| 文件 | 作用 |
| --- | --- |
| `workspace_upgrade/legacy_identity/mod.rs` | legacy identity 迁移边界入口；不是 root public legacy API。 |
| `workspace_upgrade/legacy_identity/layout.rs` | 老身份目录布局、index、credential 文件定位。 |
| `workspace_upgrade/legacy_identity/legacy.rs` | 老身份扫描/导入逻辑。 |
| `workspace_upgrade/legacy_identity/key_compat.rs` | 老 ANP / key PEM 格式兼容读取和转换。 |
| `workspace_upgrade/legacy_identity/types.rs` | legacy identity 迁移/导入所需 DTO 和 error 类型。 |
| `workspace_upgrade/legacy_identity/service.rs` | 迁移/导入需要的旧 identity service 边界。 |
| `workspace_upgrade/legacy_identity/replace_did.rs` | workspace v2->v3 / replace-did 迁移相关 DID 替换和备份。 |
| `workspace_upgrade/legacy_identity/client.rs` | legacy identity 迁移场景里需要的旧 client wrapper。 |
| `workspace_upgrade/legacy_identity/wire.rs` | legacy identity wire DTO / 序列化兼容。 |
| `workspace_upgrade/legacy_identity/did.rs` | legacy DID 生成/解析/兼容 helper。 |
| `workspace_upgrade/legacy_identity/handle_input.rs` | legacy handle/contact 输入解析。 |
| `workspace_upgrade/legacy_identity/legacy_store.rs` | legacy identity 导入时需要读取/写入的旧身份本地存储兼容层。 |
| `workspace_upgrade/legacy_identity/legacy_import_tests.rs` | legacy identity import 的单元测试。 |

## `workspace_upgrade/legacy_identity/auth`

| 文件 | 作用 |
| --- | --- |
| `workspace_upgrade/legacy_identity/auth/mod.rs` | 旧 auth service 迁移兼容逻辑。 |
| `workspace_upgrade/legacy_identity/auth/wire.rs` | 旧 auth wire DTO，供迁移/兼容路径读取旧格式。 |

## `workspace_upgrade/legacy_sqlite`

| 文件 | 作用 |
| --- | --- |
| `workspace_upgrade/legacy_sqlite/mod.rs` | legacy SQLite 迁移模块入口；只在 workspace upgrade / diagnostics 边界使用。 |
| `workspace_upgrade/legacy_sqlite/open.rs` | 打开旧 SQLite、只读/读写连接、schema version helper。 |
| `workspace_upgrade/legacy_sqlite/schema.rs` | 旧 SQLite schema 检测、目标 schema 初始化/校验。 |
| `workspace_upgrade/legacy_sqlite/import.rs` | 老 SQLite 数据导入到当前 local state。 |
| `workspace_upgrade/legacy_sqlite/rebind.rs` | 旧 local state owner DID 重绑和 E2EE 清理。 |
| `workspace_upgrade/legacy_sqlite/contacts.rs` | 旧 contacts / handle history 读取，供 migration / diagnostics。 |
| `workspace_upgrade/legacy_sqlite/query.rs` | legacy import 内部用的受控查询 helper，不是 raw SQL debug API。 |
| `workspace_upgrade/legacy_sqlite/helpers.rs` | 旧 store import / rebind 的 ID、时间、normalize 等 helper。 |
| `workspace_upgrade/legacy_sqlite/types.rs` | legacy SQLite import / rebind / scan 的类型和 error。 |
| `workspace_upgrade/legacy_sqlite/import_tests.rs` | legacy SQLite import 单元测试。 |
| `workspace_upgrade/legacy_sqlite/rebind_tests.rs` | legacy SQLite rebind 单元测试。 |
