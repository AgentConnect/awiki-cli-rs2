# ACP 无模型回归

在 CLI 仓库根目录执行：

```sh
python3 scripts/testing/acp_contract.py
python3 -m unittest discover -s scripts/testing -p 'test_acp_contract_runner.py' -v
```

支持 macOS 与 Linux；需要仓库要求的 Rust 工具链、Python 3 和 Node.js 20.6+。
无需 OpenCode、Gemini CLI、Kimi、DeepSeek Harness、模型 API Key 或运行中的服务。
依赖首次下载和 Cargo 编译使用正常开发缓存，编译时间单独计算；预热后测试通常为秒级。
可用 `--cargo-toolchain` 或既有 `AWIKI_DAEMON_RUST_CARGO_TOOLCHAIN` 指定已安装工具链。

入口只构建一次 Daemon 单元测试程序，再以临时 HOME/XDG 目录、受限 PATH 和环境白名单执行。
只向测试提供 Python、Node 与系统工具；不继承模型凭据、个人客户端配置、代理或 `NODE_OPTIONS`。
协议子进程来自提交的 `tests/fixtures/acp_agent.py`；回环 MCP 请求不连接模型。
失败、空测试选择和缺工具都会非零退出。退出时清理本次测试进程组及临时目录。

覆盖：单等待位与竞争、停止和迟到输出、子进程清理、原生会话 ID／回放、问答／多设备竞争、
模型确认／目录刷新、附件传递、可靠状态与最终投递、群上下文。
`crash_parent_fixture` 是由父测试显式调用的崩溃子进程夹具，保留 `ignored`；不是未执行的模型验收。

真实模型测试、图片语义诊断、DeepSeek 转换验收与路由审计工具已移除，不保留按需入口。
已安装在用户环境中的 CLI／中转配置不属于测试资产。模拟测试不证明第三方模型的回答质量或实时兼容性。

APP 的无模型组件与聊天回归：在 `awiki-me` 执行 `dart run tests/unit/runner.dart --suite acp`。
System 聚合入口与归属见 [ACP 测试说明](../../../awiki-system-test/docs/acp-testing.md)。

宿主机安装检测与创建准入也由此入口覆盖：使用临时可执行脚本模拟已安装／缺失／异常客户端，不读取日常安装或模型配置。
