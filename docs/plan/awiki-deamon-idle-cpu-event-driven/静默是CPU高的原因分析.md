
• 结论：awiki-deamon 现在的“静默状态”并不是事件驱动的空闲等待，而是一个 每 250ms 跑一次的主动轮询循环。当前机器上有 8 个 active agent，所以即使你没有主动运行任务，它也会
  持续做 agent inbox 轮询、session 检查、outbox flush、heartbeat 检查，并且代码里还有一些“每轮都重写本地身份文件”的逻辑。6% 到 7% CPU 在这个实现下
  是可以解释的，但从工程角度看，确实偏高，主要原因是轮询频率过高加上不必要的磁盘写入。

  现场证据
  当前进程是：

  <daemon-bin>/awiki-deamon foreground --state-root <daemon-state-root> ...

  ps 显示 PID 1275 的平均 CPU 约 6.9%。线程采样里主要消耗在主线程，im-core-local-state-db 线程也有少量占用。top -H 里我看到主线程瞬时可到 4% - 13%，同时系统 %wa 一度在
  4.8% - 20%，说明这里不只是纯 CPU 计算，也包含不少 I/O wait。

  5 秒 /proc/1275/io 采样结果比较关键：

  syscr: 约 1143 次/秒
  syscw: 约 2482 次/秒
  wchar: 约 4.06 MB/秒
  write_bytes: 约 5.64 MB/秒

  也就是说，它“静默”时仍然有明显的系统调用和磁盘写入。累计值也很大：write_bytes 已经超过 5.7GB。状态目录最新更新时间也印证了这一点：identity/*/did.json、private.key、
  e2ee-agreement-private.pem、identity/registry.json、identity/default、im-core/local-state.sqlite-wal 等文件在持续更新。

  第一原因：主循环默认 250ms 轮询一次
  默认轮询间隔在 crates/awiki-deamon/src/foreground.rs:118：

  poll_interval_ms: 250,

  主循环在 crates/awiki-deamon/src/foreground.rs:263 开始，每轮都会执行多项工作：

  - process_inbox_once(...)
  - flush_message_sync_outbox(...)
  - drain_cli_route_message_queue_once(...)
  - drain_runtime_retry_queue_once(...)
  - flush_runtime_final_outbox(...)
  - 第二次 flush_message_sync_outbox(...)
  - heartbeat.tick(...)

  最后才在 crates/awiki-deamon/src/foreground.rs:352 sleep：

  tokio::time::sleep(Duration::from_millis(options.poll_interval_ms)).await;

  所以它大约每秒跑 4 轮。这个服务的 systemd 配置没有传 --poll-interval-ms，所以就是用默认 250ms。代码虽然支持命令行参数，见 crates/awiki-deamon/src/main.rs:277，但当前
  service 的 ExecStart 没有设置它。

  第二原因：每轮会扫所有 active agent
  process_inbox_once 每轮都会从 DB 读取所有 agent，见 crates/awiki-deamon/src/foreground.rs:379：

  let agents = state.list_agent_definitions()?;

  当前 agent-list 显示有 8 个 active agent：7 个 runtime agent 加 1 个 daemon agent。对每个 agent，它会：

  - 加载 agent identity
  - 加载 auth token
  - 创建 im-core client
  - 确保 messaging session
  - 轮询 direct inbox
  - 轮询 group inbox

  session 检查在 crates/awiki-deamon/src/foreground.rs:420，轮询 direct/group 两类 inbox 在 crates/awiki-deamon/src/foreground.rs:717：

  [RuntimeInboxPollScope::Direct, RuntimeInboxPollScope::Group]

  所以按现在配置粗略看，每秒是 4 轮 × 8 个 agent × direct/group/session/client 相关操作。即使没有消息，这也不是轻量空转。

  第三原因：创建 agent client 时会无条件重写 identity 文件
  这是最明显的本地 I/O 放大点。

  每个 agent 每轮都会走 crates/awiki-deamon/src/im_core_adapter.rs:43 的 client_for_agent_identity，里面第一步就是：

  sync_agent_identity_to_im_core(config, identity, jwt_token)?;

  而 sync_agent_identity_to_im_core 不是“有变更才写”，而是每次都写：

  - did.json，见 crates/awiki-deamon/src/im_core_adapter.rs:74
  - private.key，见 crates/awiki-deamon/src/im_core_adapter.rs:78
  - e2ee-agreement-private.pem，见 crates/awiki-deamon/src/im_core_adapter.rs:82
  - auth.json，见 crates/awiki-deamon/src/im_core_adapter.rs:88
  - identity/registry.json，见 crates/awiki-deamon/src/im_core_adapter.rs:116
  - identity/default，见 crates/awiki-deamon/src/im_core_adapter.rs:120

  这就解释了为什么状态目录里的 identity 文件 mtime 一直在变化，也解释了 /proc/1275/io 里 write_bytes 很高。以 8 个 agent、每秒 4 轮算，理论上每秒几十次身份文件重写是完全
  可能的。

  第四原因：它仍在访问网络，不是完全离线空闲
  ss -tpn 显示 awiki-deamon 当前连接到了：

  47.101.44.101:443
  127.0.0.1:7890

  配置里的 base/user/message/mail/anp endpoint 都指向 https://awiki.info。日志里也能看到启动后有多次 session refresh 失败：

  daemon.runtime_inbox.session.failed ... refresh DID WBA session for agent ...
  daemon.status.heartbeat.latest_failed
  daemon.status.heartbeat.control_failed: HTTP 502 Bad Gateway

  这些失败本身不是当前持续 6% CPU 的唯一原因，因为最新 audit 主要集中在启动阶段，但它们说明 daemon 的空闲路径确实会触发远端 session / inbox / status 相关调用。网络请求、
  TLS、代理、本地 DB 更新叠加起来，会进一步推高 CPU 和 I/O。

  heartbeat 不是主因
  heartbeat 代码里有节流，见 crates/awiki-deamon/src/agent_status.rs:27：

  IDLE_HEARTBEAT_MS = 5 * 60 * 1000
  ACTIVE_HEARTBEAT_MS = 30 * 1000
  LATEST_STATUS_CHECK_MS = 10 * 1000

  所以 heartbeat 每 250ms 会被 tick 调一次，但内部多数时候会早退或复用状态。它有贡献，但不是 6% 到 7% 的主要来源。主要来源还是 250ms 轮询所有 agent，加上每轮重复创建
  client / session 检查 / inbox 查询 / 本地文件重写 / SQLite WAL 活动。

  我的判断
  这不是因为后台真的有某个任务在跑，而是 daemon 当前实现把“等待消息”做成了高频主动扫描。当前 active agent 数较多，而且每轮有不必要的本地 identity 同步写入，所以静默时 6%
  到 7% CPU 是这个实现自然产生的结果。

  优先优化方向我建议按这个顺序做：

  1. 短期止血：把 --poll-interval-ms 从默认 250ms 提到 2000ms 或 5000ms。
     当前 systemd service 没传这个参数，可以先用较大间隔验证 CPU 是否明显下降。这是最快的验证手段。

  2. 避免每轮无条件写 identity 文件。
     sync_agent_identity_to_im_core 应该改成内容变化才写，或者只在启动 / identity 变更 / token 变更时同步。

  3. 缓存每个 agent 的 ImClient 或至少缓存 identity sync 状态。
     现在每轮每个 agent 都走 client 创建和身份同步，成本被 active agent 数放大。

  4. 给失败路径加 backoff。
     session refresh、远端 502 这类失败不应该每 250ms 级别重试。可以按 agent 维度指数退避，例如 5s、15s、60s。

  5. 拆分不同任务的轮询周期。
     direct inbox、group inbox、outbox flush、runtime retry、heartbeat 不应该共享同一个 250ms 主循环频率。比如 inbox 2s，group 10s，heartbeat 保持现有节流。

  6. 进一步做事件驱动或长轮询。
     如果消息系统支持 long-poll / websocket / server push，daemon 空闲 CPU 可以明显降低。

  我没有修改代码。当前证据足够支持结论：CPU 高的核心原因是 awiki-deamon 的空闲模型本身是高频轮询，并且轮询过程中存在大量重复本地写入。
