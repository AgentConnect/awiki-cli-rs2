# Windows 原生 Daemon 与全量产品 E2E

状态：2026-11-02 实现中，尚未完成 Windows 验收。

用户明确要求 Windows 本机运行 E2E runner、AWiki Me App、CLI、Daemon 及离线 ACP 测试子进程；上海 awiki.info 只承担既有 User/Message/Realm 后端职责。不能用远端 Linux Daemon 或 Linux E2E runner 替代。业务代码在上海测试服务器编写，再以可审计提交同步至 Windows 原生构建；不推送本轮提交。

## 平台边界与安全合同

- Unix 继续使用原 UDS、同 UID 校验、flock 和现有凭据存储；Windows 增加独立平台实现，保留 RPC JSON、runtime token、controller/agent 归属与 outbox 状态机。
- Windows 本地 RPC 使用命名管道；不开放 TCP 监听，不信任客户端自报 PID/SID。管道仅允许当前用户及 SYSTEM，拒绝远程客户端，核验连接的操作系统身份；现有 runtime token 的用途、方法、目标、到期校验仍逐请求执行。
- 管道名称从规范化的本机状态根和用户身份稳定派生，不能由外部消息指定。客户端和服务端使用同一解析入口；保留原调用超时，停止时退出等待并释放本进程拥有的句柄和子进程。
- Windows 状态根使用操作系统独占文件锁，第二个 owner 必须被拒绝，进程退出或崩溃后由内核释放。不能以 PID 文件是否存在替代独占锁。
- Windows 新建秘密目录和文件在写入前设置当前用户/SYSTEM 的受保护 ACL，拒绝重解析点和宽权限秘密。使用用户范围 DPAPI 保护持久根秘密，不加入明文、共享组权限或测试专用生产回退。保留已有状态，不能覆盖旧根或静默创建替代身份。
- 运行时子进程使用本机 Node 和仓库离线 ACP fixture；不依赖已安装 Agent CLI，不调用真实模型 API。测试身份、运行目录、进程与清理凭据按 run scope 隔离。

## 实施和验收顺序

1. 在原平台适配边界补齐锁、私有文件/目录、秘密保护和命名管道；先检查现有实现和 Windows API 合同，再实现。
2. 在 Windows 原生验证独占锁、崩溃释放、管道往返/超时/停机、权限拒绝、错误 token、DPAPI 重开和秘密不进入日志；Linux 运行相应既有回归。只对确属 Unix API 的测试做平台限定，共同业务断言继续在 Windows 执行。
3. 补齐 Windows 的 Daemon/CLI 制品准备、离线 ACP fixture、生命周期和所有权检查，再接入 App 既有 Agent/Recovery/Root Transfer 用例。不能通过跳过共同 case 或降低 oracle 获得成功。
4. Windows 本机完成 Realm 专项和 `--case full` 的原 case 清单；macOS 专属原生断言按真实平台标明适用性，保留用户的 did-method-web 排除。必要的本地 fixture 另按既有 exhaustive 规则执行。
5. 发布脱敏 HTML 报告，提交测试脚本和对应实现、文档、验证证据；不推送。报告区分 Windows App/CLI/Daemon 证据、后端环境、失败历史及未验收项。

## 当前证据

- 原实现的 foreground owner lock 和 local RPC 在非 Unix 上明确拒绝启动，不能以 Windows 编译成功替代运行验收。
- Realm Windows 实测发现多 Handle 列表被 Core 的 32 KiB 响应上限拒绝；服务器提交 `fa29cfab` 已按目标端点设置独立 512 KiB 上限，保留 256 目标限制及其他响应边界；PIN 模块 35 项单元测试通过，Windows DLL 已重建，真实复测待完成。
- 本文件记录用户范围与待实现合同，不宣称 Windows Daemon 已可用。

## 本轮实现与验证记录

- 已接入 Windows `LockFileEx` owner guard、当前用户/SYSTEM 私有 ACL、DPAPI 根秘密封装和命名管道 RPC worker。Unix 原入口及 runtime token 处理函数保留；Windows 管道拒绝远端连接，连接双方从操作系统取得 peer 身份。
- 同一份服务器源码的独立 Windows 原生平台 probe 已验证私有权限、额外 Everyone ACE 拒绝、重解析点拒绝、DPAPI 篡改拒绝、跨进程锁冲突与强制终止后释放、管道往返及重复监听拒绝、读超时与停机取消。该 probe 不代表完整 Daemon 或 App E2E 已验收。
- Linux 现有秘密存储 9 项、ACP component 6 项、RPC 安全 integration 29 项通过。Windows 完整 crate 编译、持久化 Root 集成和 foreground/Agent 验证仍待执行。
- Windows 的 Python ACP 单元夹具要求 `AWIKI_TEST_PYTHON_BIN` 指向已准备的原生 `python.exe`；使用独立 `.cmd` 启动现有离线 Python fixture，不依赖系统文件关联或已安装 Agent CLI。
