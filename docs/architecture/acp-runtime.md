# ACP Runtime 合同

本模块仅用于 OpenCode、Gemini CLI、Kimi Code CLI、DeepSeek Harness。使用官方 Rust SDK、稳定 ACP v1、stdio 子进程。Hermes、Claude Code、Codex 的原有通路保持不变。

## 所有权与状态

Daemon 拥有安装/协议探测、任务接受、唯一私聊等待位、取消、原生 session、模型选择和问题的状态。会话按 Runtime Agent、Controller Scope 和经过授权的稳定 conversation scope 隔离；Direct 传输 DID 别名改变不改变原生上下文归属。APP 显示路由取自 Core 已提交消息的本地规范 conversation ID，不使用 Daemon 侧别名推导；Daemon 通道只能更新 Runtime 通道已建立的映射。APP 只消费 Core 已提交的 control message；不建立第二套消息、同步或任务事实源。ACP 状态使用单调 revision，旧回放不能覆盖新状态。

私聊 A 执行期间 B 占用唯一等待位；再来的 C 拒绝接受，由 APP 保留草稿。A 正常结束自动执行 B。停止 A 后 B 暂停，必须执行或取消；立即执行 B 必须先等待 A 确认取消或子进程退出。异常和 Daemon 重启暂停等待位，不重跑旧任务。群聊按 Agent＋群串行，忙碌指令明确拒绝，不创建等待位。

在问题等待期间停止任务时，问题关闭不记录为交互失败。原生 elicitation 和共享 MCP 均遵守同一规则：取消确认前可能返回问题关闭／RPC 错误，连接及子进程退出后以该 run 已接受的停止意图收敛为 cancelled，保留部分输出与等待位。没有停止意图的过期、无效问题或客户端自行放弃仍明确失败。

模型结束与最终消息投递分别持久化，最终输出复用既有 `runtime_final_outbox`，发送失败只重试投递。最终 outbox 入库与 ACP 任务完成使用同一事务：停止先被接受时丢弃最终输出，完成先提交时停止返回过期任务；网络投递在事务之后进行。控制命令关联准确会话、任务、问题和 revision；重复请求幂等，过期请求拒绝。问题回答独立于新消息，不占等待位；群内仅任务发起人可以回答。

## ACP 与环境

### 2026-09 可靠交互补充合同

- `acp_task_records` 按 `run_id` 保存执行详情；`acp_sessions` 只负责当前会话控制及兼容摘要。完成、停止、失败或重启中断时，先捕获当前任务文字／工具／问答，再启动等待任务，避免下一轮清空历史。
- 流式 `acp_events.event_kind=snapshot` 可以合并；任务与问题终态使用 `event_kind=task` 和稳定事件 ID，与任务事实同事务写入，禁止后续快照删除。可靠控制载荷沿用 `awiki.acp.status.v1`，新增 `acp_task`（`awiki.acp.task.v1`）；旧 `acp` 快照继续发送。
- 模型完成与消息投递分离。任务 `delivery` 为 `none/pending/sent/failed`；最终 outbox 状态更新与对应任务事件同事务提交。投递重试不启动模型。ACP 最终回复保留来源消息注解并增加 `annotations.awiki_run_id`。
- 通过现有 `runtime.acp.control` 的 `task_history` 按会话及当前消息窗口的 source_message_ids 分页读取执行详情（最多 20 条、正常页面 512 KiB；单条完整记录不截断）。快照通过 task_history_available 显式声明支持，APP 不向旧版本盲发查询，继续验证控制者及当前 controller scope；历史未保存的详情不伪造。

群上下文在调用权限通过后，由 Daemon 经 Core 的 `local_history_before_async` 读取当前指令之前的已提交消息。锚点、owner 和 conversation 绑定及排序由 Core 校验；Daemon 不拼接游标、不直接读取 Core 数据库。沿用最近 30 条、12,000 字符预算并过滤控制消息。分页只用于补足有效上下文，受扫描预算约束；本地锚点缺失、读取失败与真实空历史须分别表达。上下文只是背景，不授予权限；七种运行时共用相同入口。

附件服务发现、凭证获取及对象传输的可重试错误由 Core 统一处理，Daemon 不叠加网络重试。下载失败保留失败阶段、稳定错误码和可重试属性；未取得并校验授权附件前不调用模型。取消、中断及权限拒绝不得转换成自动模型重跑。

创建前仅检查版本、配置和 initialize 能力，不调用模型。明确失败阻止创建，无法验证账号有效性不等于失败。成功创建沿用既有 ready 前消息同步门禁和幂等欢迎消息。

会话恢复优先使用广告的 `session/resume`，其次 `session/load`；加载回放不作为新输出发送。不可恢复时等待用户确认重建，保留聊天记录。模型选择只作用于当前会话，且仅在空闲、无等待项时允许。

私聊 `prepare_session` 只握手、查询配置和验证选择，不发送 prompt；新建的探测 session ID 不持久化。准备、模型切换和任务接受使用同一会话锁，锁不跨越模型执行。`model_id` 是客户端确认的会话配置，不承诺等于代理后的实际上游；`selected_model_id` 是用户会话选择。读取 configOptions.currentValue，兼容 models.currentModelId 和原生更新。切换失败不得将请求值或恢复时的默认值写成当前模型，后续 prompt 前重新验证已确认的模型。APP 在准备或切换的发送、响应不确定及等待 Core 路由投影期间保留草稿并阻止新指令；关闭模型窗口不解除这一约束。

`refresh_models` 只重新读取当前客户端提供的目录，不发送 prompt，不调用模型切换，不更新供应商配置或安装客户端。沿用原生会话、工作目录与配置，刷新结果仅更新目录与 `model_catalog_updated_at_ms`，不改当前模型和选择意图。快照通过 `model_refresh_supported` 声明支持；命令结果的 `model_refresh` 标记 `refreshed/deferred`，安全加载窗口未到时附带 `retry_after_ms`。忙碌、等待项或更高优先级操作使刷新延期，不能将缓存返回伪装成成功读取。并发查询合并；任务接受、模型切换和重建上下文取消低优先级查询，并在其连接/子进程清理完成后取得同一会话锁。失败保留已有目录与模型，迟到查询不覆盖后发操作。

APP 模型窗口先显示缓存，成功读取超过 5 分钟时按需刷新一次，另提供手动刷新；不后台轮询。窗口保持打开时，忙碌结束或安全加载时间到达可续接延期查询。查询不阻止草稿和新指令。当前模型未列入候选项时显示独立只读当前项，不补造可选能力，不将目录缺项推断为模型失效；模型未提供、目录为空、读取失败分别表达。模型来源始终是对应宿主机 CLI，APP 不直连供应商列模型，也不根据聊天正文推断模型身份。

问答增量合同：共享 MCP 工具的问题声明交互版本 2、来源、定义摘要，以及自定义、补充文字和取消能力。`awiki.answer.v2` 的 structured 模式保留合法 content 和可选 text；custom 模式只含非空 text，最多 16 KiB UTF-8。原生 elicitation 严格按原 schema 返回，不携带 AWiki 扩展字段。新版提交携带问题定义摘要，旧 accept/content 保持兼容。任务发起人、run、question 和有效期在同一状态事务验证；文字流 revision 不使有效答案过期。问题答案、跳过、过期及关闭原因随任务记录持久化，终态事件不合并丢弃。工具权限选项不能仅因标题类似问答就当成业务答案。

恢复失败必须有明确证据才能标记上下文丢失。除协议的资源不存在错误外，Kimi 与 DSH 的参数错误必须精确匹配当前原生 session ID。OpenCode 的 session 服务内部错误需再查询当前工作目录的完整分页会话列表；请求失败、无效记录、游标循环或超时均不等于会话不存在。Gemini 在 initialize 前退出时，仅识别其官方的明确会话缺失诊断，不根据退出码推断。诊断文本只在内存中判断，不记录协议或 stderr 内容；已停止或已被替换的任务不得修改上下文状态。

文字、图片和文件经过既有授权附件下载链路后转换；不解析用户文本里的本地路径取得附件。能力来自 initialize 与当前会话模型选项，不能按品牌臆造。工具权限自动允许一次；用户询问必须等待真实答案；未知交互明确失败。

共享问答使用任务内的 `awiki_questions.request_user_input` MCP 工具，参数为 `message` 与 JSON 字符串 `schema_json`；原生 ACP elicitation 仍直接支持。MCP HTTP 监听器只绑定回环随机端口，以随机 Bearer token、Host 校验和任务标识约束请求，拒绝浏览器 Origin，任务结束关闭监听器。创建探测要求客户端声明 MCP HTTP 支持。工具名避开 `ask_user`：Gemini CLI 0.59 的 ACP 排除规则会同时排除同名 MCP 工具。

客户端提供 MCP `progressToken` 时，问答通过 Streamable HTTP 的 SSE 响应每 10 秒报告等待进度，直到真实回答、取消或 15 分钟有效期结束；进度不包含推测答案。OpenCode 1.18.31 使用进度通知重置工具超时。客户端主动取消当前问题，或在问题尚未回答时结束模型回合，均视为交互失败，不能生成成功的最终消息。依据：[MCP 进度协议](https://modelcontextprotocol.io/specification/2025-11-25/basic/utilities/progress)、[OpenCode MCP 调用实现](https://github.com/anomalyco/opencode/blob/v1.18.31/packages/opencode/src/mcp/catalog.ts)。

Kimi 0.43.1 不根据进度延长默认 60 秒 MCP 调用期限。Daemon 仅在 Kimi 任务子进程设置官方 `KIMI_MCP_TOOL_TIMEOUT_MS=960000`，覆盖 15 分钟问题有效期和返回余量，不写入日常配置；客户端已有的逐服务器期限仍按官方规则优先。真实客户端验收在问题出现后等待至少 65 秒再回答。依据：[Kimi MCP 配置](https://www.kimi.com/code/docs/en/kimi-code-cli/customization/mcp.html)。

DeepSeek Harness 0.1.5-rc.1 的 ACP MCP 配置固定使用 60 秒默认期限。Daemon 使用官方 `--patch` 配置叠加入口，为当前子进程加入 `@deepseek-ai/dsh-mcp-client`，设置 `toolCallTimeoutMs: 960000`。同一问答服务不再通过该进程的 ACP `mcpServers` 重复加入。临时补丁权限 0600、只包含环境变量引用，任务结束删除；URL 和 Bearer 仅通过子进程环境传递，不修改已安装客户端或日常配置。

该版本 DSH 的 shell 子进程会清除环境中名称包含 `TOKEN` 的变量。任务补丁另加载一个临时 Cordis 环境贡献模块，通过官方 `shellEnv` registry 只显式传递当前任务的 `DSH_AWIKI_RUNTIME_RPC_TOKEN`；文件 wrapper 调用时将其赋给原有 `AWIKI_RUNTIME_RPC_TOKEN` 环境变量。模块不包含凭据值，不传递模型 API Key，权限 0600，随任务删除。文件仍经过既有 Runtime RPC 授权和授权工作目录检查，停止／结束撤销任务 token。

Gemini CLI 0.59 的会话记录文件名只包含 UTC 分钟；在创建当分钟启动新进程加载同一 session，会覆盖其初始记录。因此首次恢复等待创建分钟结束，期间显示“正在恢复上下文”，可取消。恢复进程同时使用官方 `--resume <session ID>` 参数，让启动清理器识别正在使用的会话，避免空检查点导致同 ID 的完整历史被连带删除。启动目录先解析为实际路径，保证 macOS `/var` 与 `/private/var` 等别名不会分裂原生项目历史。Daemon 不修改客户端记录，也不以重发历史替代原生恢复。版本升级时应重新验证并移除不再需要的兼容处理。相关上游问题：[会话加载失败](https://github.com/google-gemini/gemini-cli/issues/28693)。

Gemini CLI 0.59.0／0.60.0 的 `loadSession` 未等待 `streamHistory`：响应返回后继续发送历史回放。Daemon 用 Node 模块加载钩子补上 `await`，限定官方 npm 包名、版本及调用形态；其他版本保持原样。0.60.0 额外校验历史转换函数的 SHA-256 与记录调用形态：新任务记录原始 model parts，保留调用 ID、轮次和签名；旧记录只在精确 ID、名称、参数及真实 functionResponse 轮次都可确认时在内存中重建调用位置。已有真实结果不再从工具展示元信息重复合成，不按名称猜配，不删除真实消息。缺失、跨用户轮次或冲突的历史进入 `context_reset_required`，由用户确认重新开始；源代码形态不匹配是兼容失败，不推断上下文已丢失。安装文件和既有历史不被修改。钩子文件权限 0600，子进程结束删除；保留原 `NODE_OPTIONS`，要求 Node 20.6+。版本升级需复核并移除已被上游修复的兼容逻辑。依据：[上游回放顺序报告](https://github.com/google-gemini/gemini-cli/issues/28775)、[Node 模块加载钩子](https://nodejs.org/api/module.html#moduleregisterspecifier-parenturl-options)。

ACP 子进程在独立执行线程的 Tokio runtime 中运行，使用 SDK 所有的进程组；固定 shell 启动器仅通过位置参数切换子进程工作目录，不插值命令文本。Daemon 正常退出先取消 ACP 任务并暂停等待项，再等待线程结束。启动器的同进程组监视器检查客户端的父进程；Daemon 被强制结束后会关闭该进程组，避免模型或工具子进程残留。重启撤销旧 ACP 文件投递 RPC token。停止时立即撤销文件投递 RPC token，等待协议取消，超过宽限期关闭整个进程组，随后才允许等待任务运行。状态投递由独立阻塞工作线程串行调度，不占用接收控制消息的异步线程或 RPC outbox 锁；状态投递失败不阻塞其他会话，也不阻塞原有最终消息 outbox。

## 验证边界

自动测试只使用随源码维护的模拟 ACP 子进程、临时状态和 APP 模拟服务，不读取真实模型凭据、不启动已安装 Agent CLI。真实模型验收及其转换服务测试工具已移除；协议通过不代表第三方 CLI／上游模型兼容性已实测。入口见 [ACP 无模型回归](../../scripts/testing/README.md)。
