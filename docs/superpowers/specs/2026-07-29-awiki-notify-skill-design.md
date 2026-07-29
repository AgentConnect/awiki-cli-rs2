# AWiki Notify Skill 设计

## 1. 目标

为现有 AWiki Skill Agent 增加一个按需加载的 Notify 模块，让 Coding Agent 在当前任务进入终态时，主动调用 `awiki-cli msg send`，向用户指定的 AWiki Me Handle 或 DID 发送一条普通文本消息。

第一阶段只验证以下闭环：

```text
Coding Agent
  -> AWiki Notify Skill
  -> awiki-cli msg send
  -> message-service
  -> AWiki Me 消息列表与系统横幅
```

本阶段不依赖 Daemon、不启用 E2EE，也不使用 `runtime host-notify`。`runtime host-notify` 面向宿主 Agent，不等价于给 AWiki Me 用户发消息。

## 2. 现有架构约束

仓库当前采用“单入口、两层加载”：

- `skills/SKILL.md` 是唯一默认入口；
- 领域能力放在 `skills/references/*.md`，由入口按关键词懒加载；
- 因此 Notify 应新增为 `skills/references/12-notify.md`，不新增第二个入口 `SKILL.md`。

入口只增加 Notify 路由、能力边界和最窄授权规则，具体流程放入 reference。

## 3. 触发条件

Notify 只处理当前 Coding Agent 任务的终态变化：

| 状态 | 含义 | 是否发送 |
|---|---|---|
| `completed` | 用户要求的工作已经完成 | 是 |
| `blocked` | 因外部依赖、权限或环境阻塞而无法继续 | 是 |
| `action_required` | 必须由用户作出决定或执行操作才能继续 | 是 |
| `failed` | 本次执行失败且已停止 | 是 |
| 普通进度 | 任务仍在执行 | 否 |

Skill 只提供“Agent 主动调用”的尽力通知。它不能保证 Coding Agent 在异常退出、进程被杀死或未加载 Skill 时仍然发送。需要强保证时，应在后续阶段增加 Coding Agent 生命周期 hook 或 Daemon 终态事件。

## 4. 授权与目标

`msg send` 是外部写操作。Notify 只能在用户明确指定以下信息后启用：

1. 接收目标：一个准确的 AWiki Me Handle 或 DID；
2. 允许的事件：至少一个终态；
3. 授权范围：当前任务。

用户明确表达“这个任务完成、阻塞或需要我处理时，通知 `<target>`”时，视为对当前任务内、指定目标、指定终态的一次窄授权。该授权不允许：

- 向其他目标发送；
- 发送普通进度、附件或任意聊天内容；
- 延续到后续任务；
- 读取或发送密钥、Token、私钥、完整日志、手机号码或其他敏感数据。

如果目标或授权范围不明确，Agent 必须先询问，不能根据历史消息、通讯录或本机配置猜测。

发送前必须通过 `awiki-cli id current --format json` 确认当前发送身份。多身份环境中若发送者不明确，必须询问用户；Notify 不得自行执行 `id use` 切换身份。实际调用应显式传入 `--identity <local-alias>`。

随后使用同一身份调用 `id resolve --handle <target>` 或 `id resolve --did <target>`：

- Handle 输入要求 `data.lookup.did` 与 `data.resolve.did` 非空且一致，并核对返回的完整 Handle；
- DID 输入只强制 `data.resolve.did` 等于授权 DID；`data.lookup` 可以不存在，存在时再核对 DID 一致。

把验证后的 DID 作为实际发送目标。`msg send --dry-run` 只做语法和计划回显，不负责验证身份存在性或 Handle 解析。

## 5. 消息合同

消息使用普通文本，格式固定为：

```text
[Coding Agent][<status>] <task_title>
<summary>
下一步：<next_action>
```

要求：

- `task_title` 是简短任务名；
- `summary` 只写用户可理解的结果或阻塞原因；
- `next_action` 在无需后续操作时写“无需操作”；
- 不包含 Markdown 表格、原始命令输出、绝对路径或秘密信息；
- 单条通知只表达一个终态。
- 第一阶段使用明文消息；如果任务摘要可能包含敏感项目信息，必须先提醒用户并缩减到最小披露。

示例：

```text
[Coding Agent][completed] 修复 AWiki Me 消息横幅
已完成普通消息的前台横幅与后台系统通知处理。
下一步：请在 AWiki Me 中确认是否收到测试消息。
```

## 6. 命令流程

Agent 必须把每个参数作为独立 argv 传给进程，不能用 `eval`、shell 插值或拼接不可信标题/摘要。发送前先确认身份：

```text
["awiki-cli", "id", "current", "--format", "json"]
```

然后执行 dry-run：

```text
["awiki-cli", "--identity", "<local-alias>", "id", "resolve", "--handle", "<target-handle>", "--format", "json"]
["awiki-cli", "--identity", "<local-alias>", "msg", "send", "--to", "<resolved-did>", "--text", "<message>", "--dry-run", "--format", "json"]
```

解析成功且 dry-run 中的 identity、action、target DID 与前一步输入一致后，执行实际发送：

```text
["awiki-cli", "--identity", "<local-alias>", "msg", "send", "--to", "<resolved-did>", "--text", "<message>", "--format", "json"]
```

成功判定以 CLI JSON envelope 为准：

- `ok` 为 `true`；
- `data.delivery.accepted` 或 `data.delivery.final_acceptance` 为 `true`；
- 返回非空 `data.message.id`。

不能只根据自然语言 `summary` 判断发送成功。这个结果只证明服务端已接受消息，不证明 AWiki Me 已经展示消息或横幅。

## 7. 失败与重复发送

- dry-run 失败：不执行实际发送，并在 Coding Agent 最终回复中说明通知未发送；
- 实际发送明确失败：不改变原任务终态，只附加说明通知失败；
- 实际发送结果不确定：不得盲目重试，避免重复消息；
- 同一任务的同一终态最多发送一次；
- 收到新的、不同终态时，可再次发送一次，例如先 `action_required`，用户处理后最终 `completed`。
- Skill 没有跨进程持久幂等账本；若当前任务上下文丢失，不能据此假定“尚未发送”并重试。

## 8. 代码与文档改动

第一阶段改动范围：

1. 新增 `skills/references/12-notify.md`；
2. 更新 `skills/SKILL.md` 的路由表、确认规则与能力状态；
3. 将 Notify reference 加入 `awiki-cli docs skills` 的引用列表；
4. 更新 Skill 架构文档中的 reference 数量和模块映射；
5. 新增合同测试，验证路由、授权边界、四种终态、dry-run 和成功判定均存在。

不修改：

- `awiki-cli msg send` 的网络实现；
- Daemon 事件协议；
- AWiki Me 客户端；
- E2EE；
- `runtime host-notify`。

## 9. 验收标准

自动化验收：

- Notify reference 存在且能从唯一入口路由到；
- reference 明确四种终态，不把普通进度当通知；
- reference 使用 `awiki-cli msg send`，并要求先 dry-run；
- reference 不使用 `runtime host-notify`、Daemon 或 E2EE；
- reference 明确当前任务级授权和禁止猜测目标；
- `awiki-cli docs skills` 能发现 Notify reference；
- 相关测试通过，且仓库不存在格式错误。

人工闭环验证：

1. 用户在 Coding Agent 任务开始时指定 AWiki Me 接收 Handle；
2. Coding Agent 在 `completed` 或其他终态调用命令；
3. CLI 返回发送已接受；
4. AWiki Me 消息列表出现消息；
5. AWiki Me 前台显示 App 内横幅，后台显示 macOS 系统通知。

第 4、5 项依赖客户端运行环境，不能由文档合同测试代替。
