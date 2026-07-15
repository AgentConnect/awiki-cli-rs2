# awiki-cli-rs2 兼容性与成熟度

[English](compatibility.md) | [简体中文](compatibility.zh-CN.md)

最后整理日期：2026-07-14。对外发布时需写入真实 release version、commit 和验证日期。

## 1. 能力状态

当前 Skill 元数据给出的状态：

| 领域 | 当前状态 | 对外写法 |
| --- | --- | --- |
| Identity | Implemented | 可用，但注册/恢复依赖服务与身份提供方 |
| Messaging | Partially implemented | 不应笼统写成所有消息能力生产就绪 |
| Group | Implemented | 具体服务端仍可能限制管理方法 |
| Runtime | Partially implemented | listener/host-notify 可用范围需按平台验证 |
| Page | Implemented | handle 页面能力 |
| Site Pages | Implemented | tenant bare-domain 页面能力 |
| Discovery | Partially implemented | 不能把 workflow 描述为完整自动发现系统 |
| People | Partially implemented | relationship/local contacts 可用，search 未完整支持 |
| Debug helpers | Partially implemented | 仅排障，不是产品主路径 |

## 2. CLI 平台目标

当前 release config 列出：

| Target | 状态说明 |
| --- | --- |
| `darwin-arm64` | 发布目标 |
| `darwin-amd64` | 发布目标 |
| `linux-amd64` | 发布目标，Linux artifact 使用静态 musl 策略 |
| `windows-amd64` | 发布目标 |

“列为 target”不等于每个 release 已通过完整系统测试。Manifest 应记录实际产出的平台。

## 3. SDK 平台

`awiki_im_core` 原生支持目标：

- Android；
- iOS；
- macOS；
- Linux。

Flutter Web 当前只提供运行时抛出 `UnsupportedError` 的 stub。

## 4. 服务端兼容性

| 服务 | Identity | Direct | Group | Attachment | People/Site | Secure |
| --- | --- | --- | --- | --- | --- | --- |
| AWiki 托管服务 | 主路径 | 主路径 | 主路径 | 主路径 | 按服务能力 | 按 release 验证 |
| `awiki-open-server` | 本地注册/兼容路由 | 明文 send/inbox/history | join/send/messages 等参与者能力 | 本地对象能力 | 已有 local smoke 路径 | 不支持 E2EE |
| 其他 AWiki-compatible 服务 | 逐项验证 | 逐项验证 | 逐项验证 | 逐项验证 | 逐项验证 | 不可推断 |
| 纯 ANP 远端 | DID/服务发现范围内 | 取决于 public method | 有限方法 | 有限对象方法 | 不等于 AWiki 产品 API | 不可推断 |

## 5. Open Server 限制对 CLI 的影响

连接 `awiki-open-server` 时：

- 可以验证本地 DID 注册、Direct、Inbox、History；
- 可以验证 open group join/send/messages；
- 可以验证 People relationship 与 Site Pages 的兼容入口；
- 可以使用本地附件 slot、object 与 ticket；
- 不能要求 Direct/Group E2EE；
- 不能假设完整 group create/add/remove/update；
- 不能依赖生产 SMS、email 或 Aliyun provider；
- 不能把两个都指向 `awiki.info` 的 CLI workspace 当作 Open Server 互通证据。

## 6. 安全消息

CLI 的 canonical 用户入口包括：

```bash
awiki-cli msg send --to <handle> --text "..." --secure required
awiki-cli msg secure status
awiki-cli msg secure repair
awiki-cli group secure status
awiki-cli group secure repair
```

准确表述：

> CLI 提供高层安全消息入口，并把协议与本地密钥状态交给共享 IM Core。实际成功依赖当前身份、对端、服务端与对应安全 profile；Open Server 当前不支持 E2EE。

不要向普通用户暴露 MLS private state、KeyPackage、prekey、ciphertext 或 debug low-level commands。

## 7. 版本一致性

发布前应检查：

```bash
cargo run -p xtask -- check-version
cargo run -p awiki-cli -- version
```

同时对齐：

- `crates/awiki-cli/Cargo.toml`；
- `scripts/release/cli/release-config.json`；
- npm wrapper/package；
- manifest；
- Skill metadata；
- binary embedded commit；
- ANP commit；
- Daemon 与 Flutter native artifact provenance。

## 8. 验证记录模板

```text
日期：YYYY-MM-DD
CLI channel/version：
CLI commit：
ANP commit：
服务端/domain/version：
平台/arch：

通过：
- install + version
- init + doctor
- register/recover
- direct send/inbox/history/read
- group lifecycle/messages
- attachment send/download
- runtime listener
- host notification
- secure direct/group（如适用）
- people/pages/site

未通过或未验证：
- ...
```
