# 配置说明

[English](configuration.md) | [简体中文](configuration.zh-CN.md)

本文是 **awiki-cli-rs2**（CLI、daemon、im-core）的权威配置说明。真源是 typed loader 与 `std::env::var` / `option_env!` / Cargo `[features]`。

## 编译开关

| 标识符 | 来源 | 作用 | 默认 |
| --- | --- | --- | --- |
| `awiki-cli:default` | `crates/awiki-cli/Cargo.toml` | 空 default | 开（空） |
| `awiki-cli:system-test-probe` | 同上 | 系统测试探针二进制 | **关** |
| CLI → `im-core` features | 同上 `dependencies` | **`secure-direct` + `group-e2ee` + `blocking`** | CLI 显式打开 |
| `im-core:default` | `crates/im-core/Cargo.toml` | `sqlite` + `http` + `identity-native-anp` | **开** |
| `im-core:group-e2ee` / `secure-direct` | 同上 | crate default 关；CLI 打开 | crate **关** |
| `im-core-dart` Flutter 构建 | `scripts/flutter/build-*.sh` | `group-e2ee` + `secure-direct` | 见脚本 |
| `AWIKI_CLI_RELEASE` / `AWIKI_DAEMON_RELEASE` | 编译期 `option_env!` | 发布线 | 未注入则为 **`0815`** |
| `AWIKI_CLI_VERSION` | `option_env!` | 嵌入版本 | 未设 → 运行时 `dev` |

## 工作区 / 租户

| 标识符 | 来源 | 作用 | 默认值 |
| --- | --- | --- | --- |
| `global.active_tenant` | `global.json`；`--tenant` 覆盖 | 当前租户 | 全新工作区 → `china` |
| `registry.schema_version` | `tenants/registry.json` | 租户注册表格式 | `2` |
| `registry.official_catalog_version` | 同上 | 已对齐的官方目录 | `1` |
| `registry.aliases.default` | 同上 | 兼容别名 | 全新工作区 → `china` |
| `registry.tenants[].kind` | 同上 | `built_in` 或 `custom` | 官方项 → `built_in` |
| 官方 `china` 端点 | 同上 | 上海租户 | `https://awiki.me` / `awiki.me` |
| 官方 `global` 端点 | 同上 | 硅谷租户 | `https://awiki.ai` / `awiki.ai` |
| `AWIKI_CLI_DEFAULT_BACKEND_BASE_URL` | 环境变量 | 私有发行首次初始化 backend（成对覆盖） | 空 → 官方中国租户 |
| `AWIKI_CLI_DEFAULT_DID_HOST` | 环境变量 | 私有发行首次初始化 DID host（成对覆盖） | 空 → 官方中国租户 |
| `AWIKI_CLI_WORKSPACE_HOME_DIR` | 环境变量 | 产品工作区根 | 未设 → `~/.awiki-cli` |

官方租户不允许重新配置。首次读取 v1 注册表时，指向 `awiki.ai` 的历史
`default` profile 会变成标准 `global` profile，但保留 `dir_name=default`；
`default` 别名仍指向 `global`。原子替换 v2 注册表和 active tenant
控制文件前会先生成 `.v1.bak` 备份。迁移失败时继续使用 v1 注册表，
下次运行再重试。

## 运行时 / 通知 / 密钥

| 标识符 | 来源 | 作用 | 默认值 |
| --- | --- | --- | --- |
| `runtime.listener.enabled` | `config.yaml` | 消息 listener | `true` |
| `runtime.host_notify.enabled` | `config.yaml` | 入站通知到 host | `true` |
| `secret_storage.mode` | `config.yaml` | 密钥存储 | 空 → `vault_required` |
| `AWIKI_IM_CORE_VAULT_ROOT_KEY_B64` | 环境变量 | im-core vault 根密钥 | vault 模式必填 |
| `AWIKI_MULTI_DEVICE_DEVICE_REVOKE_ENABLED` | 环境变量 | 设备吊销 | 未设 = **开**；`0` 紧急关 |
| `AWIKI_MULTI_DEVICE_DIRECT_E2EE_ENABLED` | 环境变量 | Direct E2EE 能力 | 未设 = **开**；`0` 紧急关 |
| `AWIKI_MULTI_DEVICE_GROUP_E2EE_ENABLED` | 环境变量 | Group E2EE 能力 | 未设 = **开**；`0` 紧急关 |
| `AWIKI_DID_TRANSITION_VNEXT_HIDDEN_ROLLOUT_ENABLED` | 环境变量 | DID transition 客户端路径 | 未设 = **开**；`0` 紧急关 |
| im-core `did_transition_vnext_hidden_rollout_enabled` | `ImCoreOpenOptions` | 同上的 SDK 闸 | **`true`**（App Dart 映射继承 Default） |

## Daemon

状态根默认 `~/.awiki-daemon/deamon/state`。优先级：环境变量 > `config.json` > 代码默认。

| 标识符 | 来源 | 作用 | 默认值 |
| --- | --- | --- | --- |
| `AWIKI_DAEMON_BASE_URL` | 环境变量 | 覆盖 base_url | 未设则文件或 `https://awiki.ai` |
| `AWIKI_DAEMON_VAULT_ROOT_KEY_B64` | 环境变量 | daemon vault 根密钥 | vault 模式必填 |
| `AWIKI_HERMES_TUI_TOOLSETS` | 环境变量 | TUI toolset | `terminal,skills` |
| `AWIKI_CLI_ENABLE_DIAGNOSTIC` | 环境变量 | diagnostic 命令闸（`=1`） | 关闭 |
| `AWIKI_CLI_ENABLE_MIGRATION` | 环境变量 | migration 命令闸（`=1`） | 关闭 |

超时、代理、更新缓存等其余环境变量见源码 `cli_http.rs` / `self_update`；未列出的超时默认与 2026-08-23 盘点一致。

## 测试 / 探针

| 标识符 | 来源 | 默认值 |
| --- | --- | --- |
| `awiki-cli:system-test-probe` | Cargo feature | **关** |
| `AWIKI_SYSTEM_TEST_PROBE_DAEMON_STATE_ROOT` | 环境变量 | 缺则探针失败 |
| `AWIKI_DAEMON_TEST_RUNTIME_PRE_FINISH_DELAY_MS` | 环境变量 | 未设则无延迟 |
