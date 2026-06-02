# E2EE Attachment

本目录记录附件端到端加密传输的设计方案和后续可执行实现计划。

## 文档

- [e2ee-attachment-transfer-design.md](e2ee-attachment-transfer-design.md)：协议、服务端、`im-core`、CLI、Dart / data-rs2 的完整设计方案。
- [plan.md](plan.md)：按 `awiki-plan` 生成的主执行计划，包含步骤拆分、执行台账、验证策略、Review gate 和 Codex Goal 提示词。
- [steps/](steps/)：每个实现步骤的小 Plan。

## 当前结论

1. 附件 E2EE 不新增 ANP 业务方法，继续复用 ANP-P7 的 `attachment.create_slot`、`attachment.commit_object`、`attachment.get_download_ticket` 和 P5/P6 的 E2EE 消息承载。
2. 对象字节使用 `object-e2ee` 独立随机对象密钥加密；`object_key_b64u` 与 `nonce_b64u` 只出现在 E2EE 内层 `attachment_manifest`，不得进入控制面、日志、CLI 输出或公开 DTO。
3. 服务端仍只存储和路由 opaque E2EE 消息；为了建立 Access Grant，需要客户端在发送到 sender-home 时提供不含密钥的 `attachment_grant_refs` 本地域内提示，sender-home 在消息最终 accepted 后校验并创建 grant。
4. 上层优先复用高层接口：`client.messages().send(MessageBody::Attachment, MessageSecurityMode::E2eeRequired)` 是 canonical SDK 入口；`client.attachments().send()` 只作为便捷封装扩展安全策略。
5. CLI 不新增低层 E2EE 附件命令，复用 `msg send --file --secure required` 和 `msg attachment download`，下载时自动校验、取票据、解密和写出明文。
6. 当前工作区未检出 `data-rs2` 仓库；本方案按 Dart / FRB / data 绑定层的高层 DTO 边界设计，后续执行时需要在实际仓库中确认命名和落点。
