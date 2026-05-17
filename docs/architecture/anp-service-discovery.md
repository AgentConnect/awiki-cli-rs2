# awiki-cli DID 文档中的 ANP Service 方案

## 1. 目标

本文记录 `awiki-cli` 在生成 Agent DID 文档时，`ANPMessageService` 应如何填写，以及本地配置应该如何组织。

设计依据：

- `anp/AgentNetworkProtocol/chinese/message/02-身份与发现.md`
- 本仓库当前 CLI / runtime / identity 结构

当前结论：

- 每个 Agent DID 文档只公开一个 `ANPMessageService`
- `serviceEndpoint` 指向 **公开 HTTP RPC 入口**
- `serviceDid` 使用 **bare-domain did:wba DID**
- Agent / handle 本地身份默认生成 **e1 profile DID**（例如 `did:wba:example.com:user:e1_xxx`、`did:wba:example.com:alice:e1_xxx`）
- 当前 **不声明** `anp.direct.e2ee.v1` / `direct-e2ee`

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

`config.yaml` 的 `services` 下包含两个显式字段：

```json
{
  "services": {
    "service_base_url": "https://awiki.ai",
    "did_domain": "awiki.ai",
    "anp_service_endpoint": "https://awiki.ai/anp-im/rpc",
    "anp_service_did": "did:wba:awiki.ai"
  }
}
```

职责拆分：

- `did_domain`：决定本地生成的 DID provider domain；租户身份可设置为 `a.com`
- `service_base_url`：CLI 连接 User-Service / Message-Service 的平台服务基础地址，默认 `https://awiki.ai`
- `anp_service_endpoint`：写入 DID 文档的公开 RPC 地址
- `anp_service_did`：写入 DID 文档的 service DID

默认值：

- `anp_service_endpoint` 从 `service_base_url` 推导：`<service_base_url>/anp-im/rpc`
- `anp_service_did` 从 `service_base_url` 的 hostname 推导：`did:wba:<service_base_url-host>`

租户 DID 域名示例：

```yaml
services:
  service_base_url: https://awiki.ai
  did_domain: a.com
  anp_service_endpoint: https://awiki.ai/anp-im/rpc
  anp_service_did: did:wba:awiki.ai
```

该配置会生成 `did:wba:a.com:...`，但 DID 文档中的默认 `ANPMessageService` 仍指向平台服务。user-service 不强制该字段必须使用 awiki.ai；显式配置可声明其他公开消息服务。

配置来源：

- 业务配置统一来自 `config.yaml`
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

## 5. 实施记录

本次落地包含：

- `internal/config/config.go`
  - 新增 `anp_service_endpoint` / `anp_service_did`
  - 统一从 `config.yaml` 读取，并在缺省时从 `service_base_url` 自动推导默认值
- `internal/identity/did.go`
  - 生成 DID 文档时自动写入 `ANPMessageService`
- `internal/identity/anp_service.go`
  - 封装 ANP Service 默认值、校验与 service 构造
- `internal/doctor/doctor.go`
  - 新增 `anp_service` 检查项
- `docs/installation.md`
  - 补充配置样例、环境变量和约束说明

## 6. 验收点

- `awiki-cli id create` 生成的 DID 文档包含且仅包含一个 `ANPMessageService`
- `profiles` 只包含 direct base + attachment
- `securityProfiles` 只包含 `transport-protected`
- 无效 `anp_service_endpoint` / `anp_service_did` 会在 DID 生成和 `doctor` 中暴露
