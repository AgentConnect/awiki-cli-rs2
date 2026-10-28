# 原生系统代理发布门禁

本入口只验证已发布 `awiki-im-core = 0.1.12` 的公共 `NetworkRuntime`，依赖固定在
`probe/Cargo.lock`，启用与正式消费者一致的默认 features。首次隔离探针错误地只选 `http`，
已发布 Core 的该非默认组合不能编译；这次门禁不改变产品 features，也不以此扩大 SDK 修改范围。
不发布 SDK，不改变正式 App / CLI / Daemon 源码基线。

通过既有手动 `Dependency modes` workflow 的 `native_network_gate=true` 运行
macOS 14 与 Windows Server 2022 两条原生 lane。仅接受一次性 GitHub-hosted runner，
拒绝本机和 self-hosted runner；关闭或恢复系统配置、移除临时 loopback 地址后才算通过。
不在开发电脑执行 `gate.py`。开发电脑只可执行：

```sh
python3 -m unittest discover -s scripts/network-native -p 'test_*.py' -v
```

失败后的定向复验可选择 `native_platform=macos` 或 `windows`。macOS 夹具在
`networksetup` 返回后轮询 CFNetwork 的活动配置，确认代理端口、PAC、例外已生效再发请求；
不在设置尚未由 configd 传播时误判 Core，也不调用 Core 的人工失效接口掩盖缓存问题。

检查静态代理、系统例外、原生 PAC、配置变化、接口地址变化失效、关闭/卡住/拒绝认证代理回退、
失败冷却、业务 403 保留、POST 已提交响应丢失不重发，以及直连禁用环境变量代理。
CONNECT 探测不含业务标记；所有请求只到自建无账号的夹具。系统设置 API 来自
[Windows WinINet](https://learn.microsoft.com/en-us/windows/win32/api/wininet/ns-wininet-internet_per_conn_optionw)
与 macOS `networksetup`；Windows 地址仅加到 loopback index 1，按
[netsh 合同](https://learn.microsoft.com/en-us/windows-server/administration/windows-commands/netsh-interface)
使用 `store=active`，不改 DHCP 网卡。守卫依据
[GitHub runner 环境变量](https://docs.github.com/en/actions/reference/workflows-and-actions/variables)。

报告记录原生 OS、实际二进制摘要、逐项结果及清理状态，不包含系统代理凭据。
这是 HTTP 网络运行时的原生集成证据，**不是** App UI、WebSocket、真实 VPN 切换、生产连通性
或全部代理协议的验收；这些范围须以相应独立证据报告。接口变化区分后台监测先行失效与
下一请求读取系统指纹失效，不把后者写成后台自动通知已验证。
