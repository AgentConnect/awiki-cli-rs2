# Gemini → DeepSeek 测试中转

仅用于本次真实客户端验收，不被 Daemon 或 APP 导入，不进入产品发布包。正式客户端是 Gemini CLI；模型请求经本机回环 LiteLLM 1.101.0 转成 DeepSeek Chat Completions。

`compatibility.py` 保留系统提示、图片和工具调用／结果的精确 ID。未知或缺失 ID、结果冲突、格式不支持返回 HTTP 400，不按工具名配对，也不丢弃历史结果。真正的超时／网络错误保留 LiteLLM 原分类。模型路由关闭 thinking，因为当前 Google 转换链不能往返 `reasoning_content`。模块仅在明确的 LiteLLM 版本上启用，不修改其安装文件。

在现有独立中转的 `serve.py` 中，设置必要环境并启动官方 `run_server()` 之前调用 `install_proxy_error_handler()`，它同时安装转换兼容层。计数接口绕过官方通用错误映射，需要为兼容层自有错误注册专门的 HTTP 400 处理器。监听必须是 `127.0.0.1`，API Key 与中转认证密钥从仓库外受限文件读取；不要写进命令参数、配置模板或日志。更换模块前保留旧文件用于回退，空闲时重启该独立测试服务。

本机已有 LiteLLM 环境时，在此目录执行：

```sh
~/.local/share/uv/tools/litellm/bin/python -m unittest -v test_compatibility.py
~/.local/share/uv/tools/litellm/bin/python verify_http.py --report /tmp/gemini-relay-http.json
```

上述测试不调用模型。HTTP 错误分类、流式、多轮和取消还需独立的真实客户端验收；单元通过不能替代它们。


## 模型目录与请求路由（Gemini CLI 0.60.0）

`settings.example.json` 使用官方 `experimental.dynamicModelConfiguration` 与
`modelConfigs.modelDefinitions/modelIdResolutions` 声明自定义 DeepSeek 模型，并隐藏当前版本内置但未接入的选项。把对应字段合并到受限用户配置，保留其他个人设置；更新官方 CLI 后需重新核对其模型配置合同。

`models.example.yaml` 只保留两个明确的 DeepSeek 路由和 Gemini 0.60 的内部辅助请求路由；不能使用 `gemini-*` 把所有用户选项静默映射到同一模型。内部辅助路由不作为用户选择。服务只监听 `127.0.0.1`，凭据仍从环境／受限文件读取。

APP 显示的“会话模型”是 CLI 确认的配置。模型聊天时自述的品牌、型号不作为路由证据，也不保证代理后的上游身份。真实验收可在独立测试代理中使用官方 LiteLLM callbacks 接口加载 `model_audit.audit`，将 `AWIKI_MODEL_AUDIT_PATH` 指向新的受限报告路径；它只记录模型路由字段，不记录密钥、提示或回复。Google 转换链可能不向回调传递请求模型元数据，此时 `requested` 为 null，不能由响应型号反推请求型号；需要另外只捕获 Google 接口路径中的模型名，与 provider／响应模型一同核对。测试完成停止临时代理，不在日常服务开启正文调试日志。

参考：[Gemini 配置](https://geminicli.com/docs/reference/configuration/)、[模型配置解析](https://geminicli.com/docs/cli/generation-settings/)。
