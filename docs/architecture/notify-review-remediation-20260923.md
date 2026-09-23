# Notify 修复 PR #46 的审查整改

## 范围与行为

基线已合入 `release/0910` 的 `99ae6196ffee969996cd5cccdca5fab377f0c067`。本次处理 High 发现：按入站 DID 自动发现外域发送者时，首次 WBA 文档读取与后续公开绑定读取都必须经过公共网络安全边界。实现见 `public_discovery_http.rs`，同步和异步 Directory transport 共享该实现；普通认证服务调用保持原路径。

只允许标准 HTTPS 域名，拒绝私网及混合 DNS 结果，固定已校验的连接地址；不使用代理、不跟随重定向。总超时 30 秒、正文上限 1 MiB，仍校验 TLS 与 JSON。任何失败不产生 Persona。

## PASS：本地组件验证

使用 `cargo test -p awiki-im-core --lib --features blocking` 加以下过滤器：

- `public_discovery_http`：5 项。真实本地 TLS 服务覆盖正常 JSON、证书不受信任、301/302/303/307/308 不访问重定向目标、声明与流式超大正文；DNS 测试覆盖空集、私网及公私混合地址，确认没有连接。TLS fixture 只在私有测试 helper 传入本地地址，不是生产环境可配置的豁免入口。
- `handle_discovery::`：32 项，包括生产 transport 的首个 DID 与绑定安全边界、同步/异步权威与投影回归。
- `directory_runtime::`：5 项。

可复现依赖：ANP `4208d1ca386c46e68ff3d281535308b3da737363`（已有标签 `1.0.4-rc.1`），Identity `5c4a142766feebce868053c8ab9bd58ebf3281cd`。使用独立 SDK 工作区，未修改原有 SDK 工作区，也未发布 SDK。

## BLOCKER / UNVERIFIED

本地结果不是 registry、现有 source manifest 或 Node CI 固定依赖的通过证明。现有依赖门禁仍需按当前 PR HEAD 单独核对。此次修改没有重装或复验 M153，因此此前真机验收仅适用于当时源码；新网络限制对实际 Provider 的兼容性与整条 Notify 流程仍为 UNVERIFIED。
