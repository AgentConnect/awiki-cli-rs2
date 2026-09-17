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
