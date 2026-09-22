# 通用 SDK HTTP 入口

`awiki-cli --format json --identity <alias> http request` 从 stdin 读取一个闭合 JSON：

```json
{"origin":"https://admin.example.org","method":"POST","path":"/v1/hosting/personal/resolve","headers":[{"name":"content-type","value":"application/json"}],"include_client_metadata":true,"body_base64":"e30="}
```

origin必须是无路径/用户信息的HTTPS origin，path不能改变authority。支持GET/POST/PUT/
DELETE/PATCH/HEAD/OPTIONS。body由base64还原后交给已发布Core ExternalHttpAuth，保持
4MiB上限；这是小控制请求入口，不承接静态包大文件上传。HTTP认证、凭据缓存、身份恢复
和签名仍由SDK负责，宿主不读取/导出私钥，不实现新的DID算法。

普通headers不能提供Authorization、签名、Cookie、Host、Content-Length、代理认证或
转发authority。include_client_metadata使用当前二进制的真实版本信息，不接受调用方
伪造版本；未嵌入版本的dev构建明确拒绝该模式。认证挑战由SDK处理，最多4次尝试；不
跟随重定向，不自动重试不确定的网络写入。调用方应保存operation_id并查询服务端状态。

成功的传输输出 `{status, content_type, body_base64}`，不输出认证响应头或密钥。非2xx
HTTP响应也保留状态/正文供上层解析；网络结果不确定则命令失败，不能据此推断服务端
未执行。响应正文可能包含业务秘密，上层宿主须捕获并按该业务保护，不作为诊断日志。

个人/自定义tenant的任务workspace不自动发现全局旧迁移凭据。仅原默认中国tenant保留
原legacy迁移入口；不修改共享Agent配置或现有身份文件。

构建入口仍为 scripts/release/registry-build.py，固定registry SDK与锁保持不变。它从
已提交Git归档准备构建输入，不创建worktree/clone；默认仍拒绝dirty源码。显式
`--source-commit <完整SHA>`可在保留其他任务修改时选择已提交消费者源码，输入不含
未提交修改，并记录.awiki-source.json。该选项不支持refresh-lock。所有业务代码修复
仍在主目录进行，归档仅是构建制品输入；CLI版本/渠道和实际来源须在交付中明确。
