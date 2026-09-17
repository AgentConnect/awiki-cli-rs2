# CLI 本地退出与重新加入

`awiki-cli --identity <本地别名、DID 或 Handle> id logout` 调用 Core 的 credential-only 身份退役接口，移除所选身份的本地凭据，保留消息历史和业务数据。必须显式选择身份；省略 `--identity` 或使用 `default` 都会被拒绝。`--dry-run` 只展示计划，不退出身份。

此命令不会撤销远端设备。管理配置失效时按以下顺序恢复：

1. 在现有管理设备的设备列表中撤销失败的成员设备。
2. 在失败设备的原 CLI 工作区运行上述 `id logout`。
3. 仍在原工作区运行 `id device join start`，按既有账号验证流程发起加入。
4. 管理设备开始验证后，新设备运行 `id device join poll --session <会话 ID>`，核对双方安全码，再由管理设备批准加入。
5. 新设备再次 poll 完成本地激活和预密钥发布，然后运行 `--identity <身份> msg inbox` 接收根密钥。发送端按既有最多三次、失败间隔五秒策略自动发送；已耗尽预算时在管理设备使用现有“重试自动配置”入口。
6. 用 `--identity <身份> id device list` 确认当前设备为 `admin` 且 `management_ready=true`。

CLI 不直接删除 custody/provider 文件，不在普通 Join 中隐式退出已有身份。崩溃恢复、删除 journal、稳定账号归属与旧控制消息由 Core 保持。其他本地身份不受影响；退出默认身份后的默认选择遵循 Core 返回的 `next_default`。
