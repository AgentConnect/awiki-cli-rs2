# discovery 模块接口设计

**所属 crate**：`crates/im-core`  
**阶段**：P1 只做消息发送所需的最小 endpoint / target discovery；后续补全能力发现。  
**职责**：服务发现和能力发现。

## 1. 目标

`discovery` 根据 DID document、profile 和服务能力选择 message、WebSocket、attachment 等 endpoint。它不解析 CLI config，也不选择 workspace 路径。该模块主要是 `messages`、`attachments`、`realtime` 等模块的内部能力，不作为 App 面向的主 SDK 模块暴露。

## 2. P1 职责

- 根据 `ImCoreConfig` 选择 message RPC endpoint。
- direct/group text 发送所需的最小 service endpoint 构造。
- `messages().send(Direct)` 内部所需的最小 DID/handle target resolve 可以临时放在 messages/directory internal helper 中。
- 遇到不支持的 capability 返回 `UnsupportedCapability`。

## 3. 后续职责

- 解析 DID document 中的 message service。
- 获取 message service capabilities。
- 选择 message RPC endpoint。
- 选择 WebSocket endpoint。
- 选择 attachment service。
- 校验 profile/security profile 支持情况。

## 4. 内部接口草案

```rust
pub(crate) struct DiscoveryService<'a> {
    core: &'a ImCore,
}

impl DiscoveryService<'_> {
    pub(crate) fn parse_did_document(
        &self,
        document: DidDocument,
    ) -> ImResult<DiscoveredServices>;

    pub(crate) async_or_blocking fn discover_peer_services(
        &self,
        peer: PeerRef,
    ) -> ImResult<DiscoveredServices>;

    pub(crate) fn select_message_endpoint(
        &self,
        services: &DiscoveredServices,
        requirements: MessageEndpointRequirements,
    ) -> ImResult<ServiceEndpoint>;

    pub(crate) fn select_websocket_endpoint(
        &self,
        services: &DiscoveredServices,
        requirements: RealtimeEndpointRequirements,
    ) -> ImResult<ServiceEndpoint>;

    pub(crate) fn select_attachment_endpoint(
        &self,
        services: &DiscoveredServices,
        requirements: AttachmentEndpointRequirements,
    ) -> ImResult<ServiceEndpoint>;
}
```

`async_or_blocking` 只是文档占位：Phase 1 实现按 blocking-first，后续如引入 async API 再调整。

## 5. 边界说明

- 网络 endpoint 来自 `ImCoreConfig`、DID document 或领域请求。
- `discovery` 不读取 CLI 配置文件。
- `discovery` 不知道当前调用方是 CLI 还是 App。
- App/CLI 通常不直接调用 endpoint selection API；高层业务接口应在发送消息、下载附件或启动 realtime 时自动完成 discovery。
