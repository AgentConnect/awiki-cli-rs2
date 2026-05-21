# SDK Refactor 2：本版修改原因

## 1. 为什么收窄 Phase 1

上一版 `sdk-refactor-2` 的 Phase 1 包含 directory/profile、完整群生命周期、本地状态收口、conversation projection 等能力。它更像“普通 IM SDK 完整第一轮”，不是“先让 SDK 跑起来”的 MVP。

本版把 Phase 1 收窄为：

```text
SDK 骨架 + 身份鉴权/Handle 注册 + 私聊文本 + 群聊文本 + inbox/history 基础读取
```

原因：

- 第一阶段目标是让 `im-core` 可运行、可被 CLI/App 调用，而不是一次性迁完所有 IM 能力。
- 身份、私聊、群聊是最小闭环；其他能力都可以在 SDK 入口稳定后叠加。
- 避免加密、realtime、附件、群管理、本地 conversation projection 同时进入，导致风险过大。

## 2. 为什么保留 sdk-refactor-2

`docs/sdk-refactor` 作为长期模块设计更完整，`docs/sdk-refactor-2` 适合作为执行导向方案：

- 更集中展示 public API；
- 更明确 CLI adapter；
- 更强调 public/internal deny list；
- 更适合开发者快速理解第一阶段要改哪些 handler。

因此本版不是删除 `sdk-refactor-2`，而是把它改成与主方案一致的 Phase 1 执行文档。

## 3. 吸收的建议

本版吸收：

- `public-api.md` 作为统一接口总览。
- `cli-boundary.md` 的 handler 模板。
- `IdentitySelector::LocalAlias` 命名。
- `owner_identity_id` 优先的本地状态隔离建议。
- `blocking-first` 策略。
- feature flag 分层。
- public/internal deny list。
- Phase 1A/1B/1C/1D/1E 子阶段拆法。

## 4. 没有吸收的建议或已后移的内容

本版不把以下能力放进 Phase 1：

- 完整 directory/profile；
- recover handle；
- replace DID；
- 完整 group lifecycle；
- mark-read；
- conversation projection；
- attachments；
- realtime runner；
- secure direct / group E2EE；
- provider traits。

这些能力不是不做，而是后移到 Phase 2+。

## 5. Phase 1 的成功标准

Phase 1 成功不是“SDK 功能完整”，而是：

- `crates/im-core` 独立存在并可测试；
- CLI 通过 SDK 完成身份鉴权和 Handle 注册；
- CLI 通过 SDK 完成私聊/群聊文本发送；
- inbox/history 基础读取可用；
- `im-core` 不依赖 CLI 类型；
- public API 没有暴露 actor/path/wire/store/crypto 细节。
