# ACP Runtime 合同

七种智能体统一使用官方 Rust SDK、稳定 ACP v1 与 stdio 子进程。Hermes 使用官方原生 ACP；Codex、Claude Code 使用 Daemon 随包分发的固定版本上游 ACP 适配器和宿主机 Node.js（≥22，推荐 24 LTS），调用宿主机已有客户端。OpenCode、Gemini CLI、Kimi Code CLI、DeepSeek Harness 沿用原有 ACP 启动方式。

## 2026-09-20 统一迁移合同

- 所有新建类型使用 `acp` 运行时和品牌驱动。旧 Hermes Gateway、Codex exec、Claude stream-json 不再作为执行或回退路径；仍被公共执行使用的提示词、权限、附件和上下文能力迁入共享模块。独立 CLI 产品的 Hermes 通知功能不属于本次删除范围。
- 旧三种运行时有持久停用原因，保留已提交聊天记录，但不迁移身份、原生会话、记忆或任务。启动恢复、自动修复及个人助理 bootstrap 不得复活旧记录；旧未完成执行收敛为终态，撤销 token。APP 区分旧接入停用与删除，用户手动重新创建。
- 三种类型与既有四种共用任务状态机、私聊等待位、群忙碌拒绝、授权群历史、附件、流式展示、问答、模型刷新、上下文恢复和可靠最终 outbox。运行时不理解 UI 布局，服务端不执行 ACP。
- Codex/Claude 组件包含固定上游依赖、许可及完整性信息，随整个 Daemon 版本原子安装/更新，不运行时下载、不重复打包底层客户端。适配器仅支持 macOS 13.5+ arm64/x64、Linux glibc x64 Ubuntu 20.04+ 同等级环境；不支持不阻止 Daemon 或其他类型。
- Hermes 内置记忆、配置及历史按 agent/controller/conversation scope 隔离。首次仅复用模型配置和静态凭据，不复制旧记忆、会话、OAuth 刷新凭据或共享外部记忆配置；之后配置独立维护，不自动覆盖。账号登录在对应 profile 完成。隔离 profile 不宣称为文件系统沙箱。
- 个人助理仍仅支持 Hermes，使用相同 ACP 执行机制及独立后台任务角色；保留原 delegated inbox、AppAction 授权与确认、结果同步和幂等。后台任务不占聊天等待位、不向来信者自动发送、不等待交互问答；信息不足时交付注明缺失的结果或明确结束并提示手动处理。投递重试不得重跑模型。
- 新 APP 通过执行协议能力判断三种类型是否需要升级 Daemon，不静默走旧路径；新 Daemon 明确拒绝旧执行请求。检测仍只表示安装和启动/协议条件，不验证模型账号，不调用模型或自动安装。
- MCP 问答共用同一任务授权服务。客户端声明 HTTP 时使用原有 HTTP；否则使用 ACP 标准 stdio 传输及随 Daemon 提供的任务桥接。桥接只连接给定回环端点，凭据经环境传递，stdout 只含协议消息，进度、取消、EOF和任务关闭均正确终止。
- Hermes 恢复使用严格加载，避免其 `resume` 的缺失会话自动新建语义。校验准确会话身份，只有明确不存在才提示用户重建；未知或不完整结果不能证明丢失。历史回放不作为新消息投递。

本节为本次迁移的目标合同；交付和验证状态以对应执行记录为准。

所有产品别名只用于选择 ACP 品牌。`runtime.hermes`、`generic-cli` 和旧 `runtime.cli.*`
执行入口均拒绝创建；旧 Gemini 占位配置与其余旧接入一起退役，仅保留历史读取。

控制命令在运行时分发前复用公共 `application/json`／JSON object／发送者和目标非空校验。省略内容类型沿用公共 JSON 默认行为；显式错误类型必须拒绝，ACP 不另开宽松入口。权限、任务发起人、幂等与过期校验仍由各自的既有职责承担。

## 所有权与状态

Daemon 拥有安装/协议探测、任务接受、唯一私聊等待位、取消、原生 session、模型选择和问题的状态。会话按 Runtime Agent、Controller Scope 和经过授权的稳定 conversation scope 隔离；Direct 传输 DID 别名改变不改变原生上下文归属。APP 显示路由取自 Core 已提交消息的本地规范 conversation ID，不使用 Daemon 侧别名推导；Daemon 通道只能更新 Runtime 通道已建立的映射。APP 只消费 Core 已提交的 control message；不建立第二套消息、同步或任务事实源。ACP 状态使用单调 revision，旧回放不能覆盖新状态。

私聊 A 执行期间 B 占用唯一等待位；再来的 C 拒绝接受，由 APP 保留草稿。A 正常结束自动执行 B。停止 A 后 B 暂停，必须执行或取消；立即执行 B 必须先等待 A 确认取消或子进程退出。异常和 Daemon 重启暂停等待位，不重跑旧任务。群聊按 Agent＋群串行，忙碌指令明确拒绝，不创建等待位。

在问题等待期间停止任务时，问题关闭不记录为交互失败。原生 elicitation 和共享 MCP 均遵守同一规则：取消确认前可能返回问题关闭／RPC 错误，连接及子进程退出后以该 run 已接受的停止意图收敛为 cancelled，保留部分输出与等待位。没有停止意图的过期、无效问题或客户端自行放弃仍明确失败。

模型结束与最终消息投递分别持久化，最终输出复用既有 `runtime_final_outbox`，发送失败只重试投递。最终 outbox 入库与 ACP 任务完成使用同一事务：停止先被接受时丢弃最终输出，完成先提交时停止返回过期任务；网络投递在事务之后进行。控制命令关联准确会话、任务、问题和 revision；重复请求幂等，过期请求拒绝。问题回答独立于新消息，不占等待位；群内仅任务发起人可以回答。

最终回复必须匹配活动任务接受时的 Agent、controller scope、controller DID、conversation scope、传输路由和回复接收者。会话摘要可随同一控制者的其他设备提交等待任务而更新路由，不能反向改变活动任务绑定。等待任务仅在自己的执行回合使用自己的回复路由。

状态事件在本地保存投递次数和下次尝试时间，按到期时间及插入顺序选取有界批次；失败使用 1 秒起、最多 256 秒的指数退避。尚未尝试的事件不被一批持续失败的旧事件挡住，重启保留调度信息和幂等 ID。控制者身份已被权威围栏阻止时，事件保留未发送事实及 `controller_identity_changed` 原因，退出自动扫描。共享最终回复 outbox 同样将该情况收敛为已有的 `failed_terminal`，保留原内容与绑定；不得假报已发送、重新调用模型或改投新身份。

## ACP 与环境

### 宿主机客户端安装检测

`config_summary.runtime_client_detection.schema_version=1` 声明 `runtime.clients.inspect`。
该命令沿用控制者授权和可靠回复，结果单独投影，不改变在线或任务状态。
`refresh` 绕过 30 秒内存缓存；并发请求合并，每项 5 秒、最多 3 项并行、整批 20 秒。
返回 `schema_version/checked_at_ms/cache_age_ms/clients`，每项含 `kind/status/version/reason_code`。
`status` 为 `ready/missing/unavailable/unknown`，只表示安装和启动条件，不验证账号或模型。
七种客户端复用实际运行的程序/PATH。Hermes 使用官方 `hermes acp --version` 与
`hermes acp --check` 检查 ACP 安装依赖；其余客户端执行版本命令。Codex、Claude Code
还检查随 Daemon 分发的适配器清单、入口与宿主机 Node.js（≥22，推荐 24 LTS） 版本。结果包含 `execution_protocol=acp`，
适配器类型另带 `adapter_version`。不访问模型、账号登录或执行 prompt；原始输出、环境、路径不回传。
创建前先复核所选客户端，再注册；ACP 继续协议校验。幂等命中已创建结果时不重复检查。
APP 对所有类型都要求 Daemon 的 ACP supported_drivers 声明；旧 Daemon 只声明四种时，另三种提示升级且禁止创建。安装检测未声明时沿用已声明 ACP 能力的创建检查；已声明但未知的检测结果不放行。


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

模型 ID 作为不透明标识传递，Daemon 不按品牌改写或猜测别名。`set_model` 期间收集准确 session 的配置通知；旧式空成功回执合法，但不得覆盖同次操作中明确不一致的模型通知。当前 SDK 已移除的 `current_model_update` 用最小类型兼容层接收，其他更新继续使用 SDK schema；加载历史仍不发布。客户端报告的配置不等于服务端最终路由，不能通过询问模型身份或读取客户端私有日志作为产品校验。

Hermes 0.15.1 的 DeepSeek 名称归一化会把 `deepseek-flash` 改成 `deepseek-chat`，而 ACP 仍报告请求值。已核对并离线验证官方 v0.21.3（`v2026.9.14`）修复；宿主机应使用包含此修复的版本。Daemon 不自动升级或修改外部 CLI，不维护模型别名补丁。候选客户端兼容验证可运行 `scripts/release/daemon/smoke-hermes-models.py --python <候选官方源码环境的 Python>`：临时 HOME、假凭据、回环 HTTP、禁止外部网络，验证默认、切换、进程重启恢复后的实际请求参数；常规 ACP 合同仍只需要仓内模拟子进程。

`refresh_models` 只重新读取当前客户端提供的目录，不发送 prompt，不调用模型切换，不更新供应商配置或安装客户端。沿用原生会话、工作目录与配置，刷新结果仅更新目录与 `model_catalog_updated_at_ms`，不改当前模型和选择意图。快照通过 `model_refresh_supported` 声明支持；命令结果的 `model_refresh` 标记 `refreshed/deferred`，安全加载窗口未到时附带 `retry_after_ms`。忙碌、等待项或更高优先级操作使刷新延期，不能将缓存返回伪装成成功读取。并发查询合并；任务接受、模型切换和重建上下文取消低优先级查询，并在其连接/子进程清理完成后取得同一会话锁。失败保留已有目录与模型，迟到查询不覆盖后发操作。

APP 模型窗口先显示缓存，成功读取超过 5 分钟时按需刷新一次，另提供手动刷新；不后台轮询。窗口保持打开时，忙碌结束或安全加载时间到达可续接延期查询。查询不阻止草稿和新指令。当前模型未列入候选项时显示独立只读当前项，不补造可选能力，不将目录缺项推断为模型失效；模型未提供、目录为空、读取失败分别表达。模型来源始终是对应宿主机 CLI，APP 不直连供应商列模型，也不根据聊天正文推断模型身份。

问答增量合同：共享 MCP 工具的问题声明交互版本 2、来源、定义摘要，以及自定义、补充文字和取消能力。`awiki.answer.v2` 的 structured 模式保留合法 content 和可选 text；custom 模式只含非空 text，最多 16 KiB UTF-8。原生 elicitation 严格按原 schema 返回，不携带 AWiki 扩展字段。新版提交携带问题定义摘要，旧 accept/content 保持兼容。任务发起人、run、question 和有效期在同一状态事务验证；文字流 revision 不使有效答案过期。问题答案、跳过、过期及关闭原因随任务记录持久化，终态事件不合并丢弃。工具权限选项不能仅因标题类似问答就当成业务答案。

智能体提供的 `pattern` 仅在 Daemon 使用无回溯的 Rust regex 校验，APP 不解释这些规则。单条规则最多 1024 UTF-8 字节、嵌套深度 64、编译尺寸及 DFA 缓存各 1 MiB；问题总量仍为 64 KiB／32 字段，单个文本回答 16 KiB、整体回答 64 KiB。超限或不支持的规则明确拒绝，不静默忽略约束。回答格式不符时不提交答案事实，APP 保留草稿并允许修改后以新命令提交；不把明确拒绝当成传输结果未知。

恢复失败必须有明确证据才能标记上下文丢失。除协议的资源不存在错误外，Kimi 与 DSH 的参数错误必须精确匹配当前原生 session ID。OpenCode 的 session 服务内部错误需再查询当前工作目录的完整分页会话列表；请求失败、无效记录、游标循环或超时均不等于会话不存在。Gemini 在 initialize 前退出时，仅识别其官方的明确会话缺失诊断，不根据退出码推断。诊断文本只在内存中判断，不记录协议或 stderr 内容；已停止或已被替换的任务不得修改上下文状态。

文字、图片和文件经过既有授权附件下载链路后转换；不解析用户文本里的本地路径取得附件。能力来自 initialize 与当前会话模型选项，不能按品牌臆造。工具权限自动允许一次；用户询问必须等待真实答案；未知交互明确失败。

共享问答使用任务内的 `awiki_questions.request_user_input` MCP 工具，参数为 `message` 与 JSON 字符串 `schema_json`；原生 ACP elicitation 仍直接支持。MCP HTTP 监听器只绑定回环随机端口，以随机 Bearer token、Host 校验和任务标识约束请求，拒绝浏览器 Origin，任务结束关闭监听器。客户端未声明 HTTP 时使用同服务的 stdio 桥；不按品牌伪造 HTTP 能力。工具名避开 `ask_user`：Gemini CLI 0.59 的 ACP 排除规则会同时排除同名 MCP 工具。

客户端提供 MCP `progressToken` 时，问答通过 Streamable HTTP 的 SSE 响应每 10 秒报告等待进度，直到真实回答、取消或 15 分钟有效期结束；进度不包含推测答案。OpenCode 1.18.31 使用进度通知重置工具超时。客户端主动取消当前问题，或在问题尚未回答时结束模型回合，均视为交互失败，不能生成成功的最终消息。依据：[MCP 进度协议](https://modelcontextprotocol.io/specification/2025-11-25/basic/utilities/progress)、[OpenCode MCP 调用实现](https://github.com/anomalyco/opencode/blob/v1.18.31/packages/opencode/src/mcp/catalog.ts)。

Kimi 0.43.1 不根据进度延长默认 60 秒 MCP 调用期限。Daemon 仅在 Kimi 任务子进程设置官方 `KIMI_MCP_TOOL_TIMEOUT_MS=960000`，覆盖 15 分钟问题有效期和返回余量，不写入日常配置；客户端已有的逐服务器期限仍按官方规则优先。自动测试通过模拟超时与进度验证该设置，不调用真实模型。依据：[Kimi MCP 配置](https://www.kimi.com/code/docs/en/kimi-code-cli/customization/mcp.html)。

DeepSeek Harness 0.1.5-rc.1 的 ACP MCP 配置固定使用 60 秒默认期限。Daemon 使用官方 `--patch` 配置叠加入口，为当前子进程加入 `@deepseek-ai/dsh-mcp-client`，设置 `toolCallTimeoutMs: 960000`。同一问答服务不再通过该进程的 ACP `mcpServers` 重复加入。临时补丁权限 0600、只包含环境变量引用，任务结束删除；URL 和 Bearer 仅通过子进程环境传递，不修改已安装客户端或日常配置。

该版本 DSH 的 shell 子进程会清除环境中名称包含 `TOKEN` 的变量。任务补丁另加载一个临时 Cordis 环境贡献模块，通过官方 `shellEnv` registry 只显式传递当前任务的 `DSH_AWIKI_RUNTIME_RPC_TOKEN`；文件 wrapper 调用时将其赋给原有 `AWIKI_RUNTIME_RPC_TOKEN` 环境变量。模块不包含凭据值，不传递模型 API Key，权限 0600，随任务删除。文件仍经过既有 Runtime RPC 授权和授权工作目录检查，停止／结束撤销任务 token。

Gemini CLI 0.59 的会话记录文件名只包含 UTC 分钟；在创建当分钟启动新进程加载同一 session，会覆盖其初始记录。因此首次恢复等待创建分钟结束，期间显示“正在恢复上下文”，可取消。恢复进程同时使用官方 `--resume <session ID>` 参数，让启动清理器识别正在使用的会话，避免空检查点导致同 ID 的完整历史被连带删除。启动目录先解析为实际路径，保证 macOS `/var` 与 `/private/var` 等别名不会分裂原生项目历史。Daemon 不修改客户端记录，也不以重发历史替代原生恢复。版本升级时应重新验证并移除不再需要的兼容处理。相关上游问题：[会话加载失败](https://github.com/google-gemini/gemini-cli/issues/28693)。

Gemini CLI 0.59.0／0.60.0 的 `loadSession` 未等待 `streamHistory`：响应返回后继续发送历史回放。Daemon 用 Node 模块加载钩子补上 `await`，限定官方 npm 包名、版本及调用形态；其他版本保持原样。0.60.0 额外校验历史转换函数的 SHA-256 与记录调用形态：新任务记录原始 model parts，保留调用 ID、轮次和签名；旧记录只在精确 ID、名称、参数及真实 functionResponse 轮次都可确认时在内存中重建调用位置。已有真实结果不再从工具展示元信息重复合成，不按名称猜配，不删除真实消息。缺失、跨用户轮次或冲突的历史进入 `context_reset_required`，由用户确认重新开始；源代码形态不匹配是兼容失败，不推断上下文已丢失。安装文件和既有历史不被修改。钩子文件权限 0600，子进程结束删除；保留原 `NODE_OPTIONS`，要求 Node 20.6+。版本升级需复核并移除已被上游修复的兼容逻辑。依据：[上游回放顺序报告](https://github.com/google-gemini/gemini-cli/issues/28775)、[Node 模块加载钩子](https://nodejs.org/api/module.html#moduleregisterspecifier-parenturl-options)。

ACP 子进程在独立执行线程的 Tokio runtime 中运行，使用 SDK 所有的进程组；固定 shell 启动器仅通过位置参数切换子进程工作目录，不插值命令文本。Daemon 正常退出先取消 ACP 任务并暂停等待项，再等待线程结束。启动器的同进程组监视器检查客户端的父进程；Daemon 被强制结束后会关闭该进程组，避免模型或工具子进程残留。重启撤销旧 ACP 文件投递 RPC token。停止时立即撤销文件投递 RPC token，等待协议取消，超过宽限期关闭整个进程组，随后才允许等待任务运行。状态投递由独立阻塞工作线程串行调度，不占用接收控制消息的异步线程或 RPC outbox 锁；状态投递失败不阻塞其他会话，也不阻塞原有最终消息 outbox。

## 验证边界

自动测试只使用随源码维护的模拟 ACP 子进程、临时状态和 APP 模拟服务，不读取真实模型凭据、不启动已安装 Agent CLI。真实模型验收及其转换服务测试工具已移除；协议通过不代表第三方 CLI／上游模型兼容性已实测。入口见 [ACP 无模型回归](../../scripts/testing/README.md)。


## 旧数据与后台助理迁移

schema 37 的 `runtime_retirement` 永久记录旧接入的 DID、profile 与原因。每次打开状态时
重施退役门禁：终止旧运行，撤销任务令牌，停止排队与未发送 outbox，保留消息正文、
原生文件和审计。创建/upsert/恢复均不能把退役 DID 复活。心跳只读投影为
`profile_status=retired / config_summary.protocol=legacy`，不再修补 Gateway 配置。

新的个人助理由设置页显式启用，使用 `app-personal-agent:acp-v1:<owner>:<appInstance>`
绑定和独立 handle 哈希，bootstrap 重试复用同一代次。旧 bootstrap 不能创建替代身份。
后台角色只获得 `rpc.ping / app.action.request`，不获得消息发送或问答工具；最终投递
与执行中都校验当前绑定。停用/撤权后取消运行，最终结果不投递给来信者。

### 宿主机 Node 与分发体积（0.1.102）

Codex、Claude Code 的固定适配器复用宿主机 Node；检测与执行共用程序发现和版本校验，支持常规安装符号链接。缺失、版本不兼容、启动失败、超时使用独立 reason_code；每次启动复核，清除 NODE_OPTIONS/NODE_PATH，避免与进程注入配置耦合。其他五种客户端不新增此要求。APP 提供官方安装入口与重新检测，不自动安装或修改宿主机。

组件构建删除 source map、声明文件、测试、示例与非法律 Markdown，保留运行时源码、锁文件和许可声明；禁止捆绑原生 Agent CLI。为兼容 0.1.101 升级器，schema_version 保持 1，新增 runtime=host-node 描述；保留极小的 acp/node shell 转发器与说明文件 LICENSE.node，不包含 Node 二进制。新 Daemon 直接发现宿主机 Node，不调用兼容转发器。全部分发文件仍进入清单哈希校验。

### 客户端路径与升级（2026-09-21）

七种类型创建前仍执行安装检查和 ACP 探测。未显式提供 `driver_config.binary_path` 时，探测只临时使用当次解析结果，持久化的 `binary_path` 保持空；每次新建子进程从 Daemon 的有效 PATH 重新发现客户端。因此升级或替换宿主机客户端不需要重建 Agent。独立终端里的 `nvm use` 不会自动修改已运行 Daemon 的环境，仍以 Daemon 的实际发现规则为准。

显式非空路径保存并优先使用；路径失效时明确失败，不静默调用另一份客户端。schema 38 仅一次性清除已保存成功创建探测、属于 ACP profile、原始配置明确未指定 `binary_path` 的绝对默认路径；显式配置（包括异常值）、旧运行时、缺少来源证据或无法解析的配置均保留。迁移不修改身份、工作目录、会话或模型设置。

个人助理新建使用 `app-personal-agent:acp-v1:{userDid}:{appInstanceId}`，加密 envelope 的 `binding_id` 与 payload 的 `ensure_once_key` 相同。Daemon 与 Rust System Test 探针共用生成函数；旧代际请求继续拒绝，不自动创建替代身份。
