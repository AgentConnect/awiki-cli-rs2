# awiki-cli DID 文档中的 ANP Service 方案

## 1. 目标

本文记录 `awiki-cli` 在生成 Agent DID 文档时，`ANPMessageService` 应如何填写，以及本地配置应该如何组织。

设计依据：

- `../anp/AgentNetworkProtocol/chinese/message/02-身份与发现.md`
- 本仓库当前 CLI / runtime / identity 结构

当前结论：

- 每个 Agent DID 文档只公开一个 `ANPMessageService`
- `serviceEndpoint` 指向 **公开 HTTP RPC 入口**
- `serviceDid` 使用 **bare-domain did:wba DID**
- Agent / handle 本地身份默认生成 **e1 profile DID**（例如 `did:wba:example.com:user:e1_xxx`、`did:wba:example.com:alice:e1_xxx`）
- 当前 **不声明** `anp.direct.e2ee.v1` / `direct-e2ee`

上面的 v1 示例仅描述 legacy identity adapter。新建的 AWiki 多设备 DID 从 genesis
开始使用 vNext service profile，其中附件控制面使用 `anp.attachment.v2`。附件下载必须
读取原消息发送者 DID Document，并使用该 `ANPMessageService` 实际声明的精确附件
Profile；同一服务同时声明 v2 与 v1 时优先使用 v2，只声明 v1 时则显式使用 v1，
二者不能隐式互换或在 v2 失败后静默降级。

`attachment.get_download_ticket` 的客户端第一跳固定提交到本域 `/im/rpc`，并使用
本域 ActorUser 会话认证；请求中的 `meta.target.did` 仍绑定原消息发送者公开的
`serviceDid`，由本域 Home 完成后续联邦调用。客户端不得把本域 bearer 直接发送到
发现得到的远端绝对 URL。

## 2. DID 文档填写规则

`awiki-cli` 生成 DID 文档时，固定写入如下语义的 service 条目：

```json
{
  "id": "<agent_did>#message",
  "type": "ANPMessageService",
  "serviceEndpoint": "https://example.com/anp-im/rpc",
  "serviceDid": "did:wba:example.com",
  "profiles": [
    "anp.core.binding.v1",
    "anp.direct.base.v1",
    "anp.attachment.v1"
  ],
  "securityProfiles": [
    "transport-protected"
  ]
}
```

说明：

- `serviceEndpoint` 是 DID 文档里的公开发现地址，不是 CLI 本地 bridge、listener socket 或 websocket 地址
- `serviceDid` 是服务身份提示字段，当前固定要求为 bare-domain DID
- direct E2EE 还未作为 awiki-cli 的公开互通能力启用，因此不写 `anp.direct.e2ee.v1` / `direct-e2ee`

## 3. 本地配置项

租户注册表拥有平台后端和 DID host；`config.yaml` 只保存租户本地运行配置和可选服务覆盖。

```text
~/.awiki-cli/
  global.json
  tenants/
    registry.json
    default/
      config.yaml
```

职责拆分：

- `tenants/registry.json` 中的 `backend_base_url`：CLI 连接 User-Service / Message-Service / content / group 等平台服务的基础地址。
- `tenants/registry.json` 中的 `did_host`：决定本地生成的 DID provider host；bare handle 会按当前租户 host 补全。
- `config.yaml` 中的 `services.anp_service_endpoint`：写入 DID 文档的公开 RPC 地址，可选。
- `config.yaml` 中的 `services.anp_service_did`：写入 DID 文档的 service DID，可选。

默认值：

- `anp_service_endpoint` 从当前租户 `backend_base_url` 推导：`<backend_base_url>/anp-im/rpc`
- `anp_service_did` 从当前租户 `backend_base_url` 的 hostname 推导：`did:wba:<backend-base-url-host>`

租户创建示例：

```bash
awiki-cli tenant create acme --backend-base-url https://awiki.ai --did-host a.com
awiki-cli tenant use acme
```

该租户会生成 `did:wba:a.com:...`，默认 DID 文档中的 `ANPMessageService` 仍从租户后端推导。需要声明其他公开消息服务时，只配置 `anp_service_endpoint` / `anp_service_did`。

配置来源：

- 后端地址和 DID host 统一来自租户注册表
- 租户内运行配置来自 `tenants/<name>/config.yaml`
- 未配置时使用默认推导值
- 除 `AWIKI_CLI_WORKSPACE_HOME_DIR` 外，不再支持通过环境变量注入这些字段

## 4. 校验规则

为了避免把本地实现细节写进 DID 文档，CLI 在生成 DID 文档和执行 `doctor` 时都做以下校验：

- `anp_service_endpoint` 只能是 `http` 或 `https`
- 不能使用 `localhost`
- 不能使用 loopback IP（如 `127.0.0.1`、`::1`）
- 不能使用 websocket URL
- `anp_service_did` 必须是 `did:wba:*`
- `anp_service_did` 不能带 fragment
- `anp_service_did` 必须是 bare-domain DID，不能带额外路径段

当前 **不要求** `serviceEndpoint` 和 `serviceDid` 必须指向同一个 home message-service；两者各自只做格式与公开性校验。

## 5. 实现锚点

当前 Rust workspace 中的主要实现锚点：

- `crates/awiki-cli/src/workspace_config/`
  - 从租户注册表读取 `backend_base_url` / `did_host`，从 `config.yaml` 读取 `anp_service_endpoint` / `anp_service_did`，并在缺省时从当前租户后端推导默认值。
- `crates/im-core/src/config.rs`
  - 在 `ImCoreConfig` 中承载 SDK 需要的公开 ANP service endpoint / DID。
- `crates/im-core/src/internal/identity_generation.rs`
  - 生成 DID 文档时写入 `ANPMessageService`。
- `crates/awiki-cli/src/diagnostics/mod.rs`
  - `doctor` 的 `anp_service` 检查项校验 endpoint 与 DID 的公开性和格式。
- `docs/installation.md`
  - 配置样例、工作区和运行约束说明。

## 6. 验收点

- `awiki-cli id create` 生成的 DID 文档包含且仅包含一个 `ANPMessageService`
- `profiles` 只包含 direct base + attachment
- `securityProfiles` 只包含 `transport-protected`
- 无效 `anp_service_endpoint` / `anp_service_did` 会在 DID 生成和 `doctor` 中暴露
