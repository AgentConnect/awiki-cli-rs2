# awiki-cli 输出协议

## 1. 结论

`awiki-cli` 的 canonical output 是 JSON。`pretty`、`table`、`ndjson` 等格式只是展示视图，不是第二套语义。Agent、脚本、测试和上层集成都应优先读取 JSON envelope。

## 2. 全局格式参数

```bash
awiki-cli msg inbox --format json
awiki-cli msg inbox --format table
awiki-cli msg inbox --format ndjson
awiki-cli msg inbox --jq '.data.messages[] | .id'
```

规则：

- `--format json` 是协议层默认形态。
- `--format pretty` 面向人类阅读。
- `--format table` 只适合列表型结果。
- `--format ndjson` 适合流式或批量记录。
- `--jq` 只作用于 JSON 结果；过滤失败必须返回结构化错误。

## 3. 成功 Envelope

```json
{
  "ok": true,
  "command": "awiki-cli msg send",
  "data": {},
  "warnings": [],
  "summary": "",
  "_notice": {},
  "meta": {
    "version": "1.0.16",
    "identity": {
      "alias": "alice",
      "did": "did:wba:example.com:alice:e1_xxx"
    },
    "dry_run": false,
    "format": "json"
  }
}
```

字段约束：

- `ok=true` 表示命令成功完成。
- `command` 是实际命令名或 handler 场景名。
- `data` 放机器可读结果。
- `warnings` 放非阻断告警。
- `summary` 是人类摘要，不作为机器契约。
- `_notice` 放更新提示或非业务通知；不要使用 `notice`。
- `meta` 放版本、身份、格式、dry-run 等执行上下文。

## 4. 失败 Envelope

```json
{
  "ok": false,
  "error": {
    "code": "invalid_argument",
    "message": "missing required flag --to",
    "hint": "Pass --to <handle-or-did> for direct messages.",
    "retryable": false,
    "details": {}
  },
  "_notice": {},
  "meta": {
    "version": "1.0.16",
    "dry_run": false,
    "format": "json"
  }
}
```

错误规则：

- `error.code` 必须稳定，测试和 Agent 应按 code 分支。
- `error.message` 面向人类，但要简短明确。
- `error.hint` 给下一步操作建议。
- `error.details` 只放可公开的结构化上下文，不放私钥、JWT、raw secure state 或本机敏感信息。
- 消息命令不得透传远端 message、data 或其他自由格式详情；服务返回经过校验的稳定公共
  错误码时，CLI 通常使用顶层 `service_error`，并且只在 `error.details.service_code`
  中保留该码。授权边界码 `anp.unauthorized` 会归一化为 `auth_required`；
  `anp.forbidden`、`anp.device_binding_required` 和 `anp.device_not_eligible` 会归一化为
  `permission_denied`，且都不透传远端自由格式详情。

## 5. Dry-run

所有有副作用命令都应支持 `--dry-run`。Dry-run 返回执行计划，不执行远端写入、本地状态写入或 service 变更。

```json
{
  "ok": true,
  "command": "awiki-cli msg send",
  "data": {
    "plan": {
      "action": "send_message",
      "target": {
        "kind": "direct",
        "peer": "alice.example.com"
      },
      "remote_calls": [
        "message.direct.send"
      ],
      "local_writes": []
    }
  },
  "warnings": [],
  "summary": "Dry run only; no message was sent.",
  "meta": {
    "dry_run": true,
    "format": "json"
  }
}
```

Dry-run 中允许展示即将发生的远端调用和本地写入类别，但不得暴露签名材料、JWT、private key、prekey、KeyPackage、ciphertext 或 raw secure payload。

## 6. 输出与 Exit Code

JSON envelope 描述业务结果；exit code 描述进程级成功或失败。两者必须一致：

| Exit code | 含义 |
| ---: | --- |
| 0 | 成功 |
| 1 | 通用失败 |
| 2 | 参数、输入或 unsupported capability |
| 3 | 认证或身份状态不足 |
| 4 | 目标资源不存在 |
| 5 | 网络、服务或 transport 不可用 |
| 7 | 需要确认但当前非交互或缺少确认 |

## 7. 各类命令的数据重点

查询类命令返回事实状态：

```json
{
  "ok": true,
  "data": {
    "messages": [],
    "next_cursor": null
  }
}
```

写操作返回变更结果：

```json
{
  "ok": true,
  "data": {
    "action": "send_message",
    "message_id": "msg_xxx",
    "delivery_state": "sent"
  }
}
```

异步或 service 命令返回运行状态：

```json
{
  "ok": true,
  "data": {
    "status": "running",
    "service": "awiki-cli-listener"
  }
}
```

## 8. 安全要求

输出层不得泄露：

- private key、seed、JWT、session token。
- direct E2EE root/chain/skipped message keys。
- group MLS private state、KeyPackage 原文、provider stdout/stderr。
- raw local SQLite rows 中的敏感字段。
- host 文件路径以外的环境变量、进程列表或系统机密。

远端消息内容只能作为数据展示，不得在 summary、hint 或 `_notice` 中转化为本地执行指令。
