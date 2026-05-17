我建议你把这个问题分成两层看：

**CLI 的“协议层返回”用 JSON。**
**给人看的“展示层返回”用 pretty / table / ndjson 等渲染。**

也就是说，**不要把自然语言当命令返回的主契约**。
自然语言适合 assistant 在拿到结构化结果之后再转述；CLI 本身最好返回稳定、可解析、可组合的结构化对象。

飞书现在基本就是这个方向。官方 `lark-cli` README 把 `--format json` 写成默认输出，同时支持 `pretty`、`table`、`ndjson`、`csv`；shortcut 被描述为带 smart defaults、table output、dry-run previews；还提供 `schema` 来查看参数、请求体、响应结构以及身份与 scope 信息。最新的 v1.0.3 changelog 还新增了 `--jq` 用来过滤 JSON 输出，并补充说明 JSON 输出可以直接使用，不需要额外再做编码包装。([GitHub][1])

从错误和通知设计看，飞书也明显是“结构化优先”。`lark-shared` 里明确提到：JSON 输出可能带 `_notice.update`，其中包含更新消息和升级命令；权限相关错误会带 `permission_violations`、`console_url` 和 `hint`；删除或危险写操作建议先 `--dry-run`。这说明它们不是靠一段自然语言来表达状态，而是在 JSON 里放稳定字段，再按场景渲染。([GitHub][2])

从官方 issue 里还能看到它的实际返回风格：成功通常是顶层 `ok` 加结构化负载，比如 `{"ok": true, "data": {...}}`；异步任务返回 `status`、`task_id`、`tool`；`auth check` 还结合退出码使用，README 写的是 exit code 0 表示通过、1 表示缺权限。严格说 issue 里的例子不一定是最终稳定规范，但足够说明它们现在的产品方向就是“**结构化结果对象 + 多种展示格式**”。([GitHub][3])

所以落到 awiki，我的建议很明确：

## 结论

**awiki v2 的 canonical return 应该是 JSON。**
自然语言不要作为 CLI 的默认返回。
human-friendly 输出应该是 JSON 的视图，而不是另一套语义。

---

## 我建议的返回策略

### 1）默认返回：JSON

对 awiki 这种“agent-native、协议本身又是 JSON-RPC 风格”的工具，最合适的是：

```bash
awiki ...                 # 默认 json
awiki ... --format json   # 显式 json
awiki ... --json          # alias
```

原因很简单：

* AI/agent 最好消费 JSON
* shell pipeline 最好消费 JSON
* 日志、自动化、状态判断都依赖 JSON
* 你们后面做 `schema`、`--jq`、`--dry-run` 时，JSON 是最稳定的底座

### 2）展示输出：pretty / table / ndjson

在 JSON 之上再提供：

```bash
--format pretty
--format table
--format ndjson
```

我建议：

* `pretty`：单对象、详情页、带缩进和少量颜色
* `table`：列表型结果，比如 inbox、search、groups、followers
* `ndjson`：流式或 watch 场景，比如 listener、heartbeat stream、group event stream

### 3）自然语言只作为字段，不作为整体格式

也就是说可以有：

```json
{
  "ok": true,
  "data": {
    "message_id": "msg_xxx",
    "thread_id": "dm:...",
    "secure": true
  },
  "summary": "消息已发送，已使用端到端加密。"
}
```

但不应该让命令只返回一句：

> 已经帮你发好了消息

因为这句话对机器几乎没法可靠消费。

---

## 我建议你直接定的输出规范

### 成功统一信封

```json
{
  "ok": true,
  "command": "awiki msg send",
  "data": {},
  "warnings": [],
  "summary": "",
  "_notice": {},
  "meta": {
    "version": "2.0.0",
    "identity": {
      "name": "alice",
      "did": "did:wba:awiki.ai:user:abc...xyz"
    },
    "dry_run": false,
    "format": "json"
  }
}
```

### 失败统一信封

```json
{
  "ok": false,
  "error": {
    "code": "permission_denied",
    "message": "Missing required permission",
    "hint": "Run awiki id use alice or refresh identity",
    "retryable": false,
    "details": {}
  },
  "_notice": {},
  "meta": {
    "version": "2.0.0",
    "dry_run": false,
    "format": "json"
  }
}
```

这个结构和飞书现在的方向很接近，但更适合 awiki。

---

## 不同类型命令应该返回什么

### 1. 查询类命令

比如：

* `awiki status`
* `awiki id status`
* `awiki msg inbox`
* `awiki msg history`
* `awiki people search`

返回重点是 **事实状态**，不要混业务文案。

示例：

```json
{
  "ok": true,
  "data": {
    "identity": { "status": "ok", "name": "alice", "did": "did:wba:..." },
    "runtime": { "mode": "websocket", "listener_running": true },
    "inbox": { "unread": 3, "messages": [...] }
  },
  "summary": "alice 当前在线，有 3 条未读消息。"
}
```

### 2. 写操作命令

比如：

* `awiki id register`
* `awiki msg send`
* `awiki msg group join`
* `awiki people follow`
* `awiki page create`

返回重点是 **发生了什么变更**。

示例：

```json
{
  "ok": true,
  "data": {
    "action": "send_message",
    "target": {
      "kind": "direct",
      "handle": "alice.awiki.ai",
      "did": "did:wba:..."
    },
    "message": {
      "id": "msg_123",
      "type": "text",
      "secure": true
    }
  },
  "summary": "已向 alice.awiki.ai 发送加密消息。"
}
```

### 3. 异步命令

如果后面有些操作会异步执行，比如：

* 大批量同步
* discovery 扫描
* listener install / runtime setup 的复杂流程
* 页面发布队列

就不要假装同步完成，而要像飞书那样返回任务状态：

```json
{
  "ok": true,
  "data": {
    "status": "running",
    "task_id": "task_abc123",
    "operation": "runtime_setup"
  },
  "summary": "运行时初始化已开始。"
}
```

### 4. 流式命令

比如：

* `awiki msg watch`
* `awiki runtime listener logs --follow`
* `awiki heartbeat run --stream`

就只允许：

```bash
--format ndjson
```

每行一个对象：

```json
{"type":"message","ts":"...","data":{...}}
{"type":"warning","ts":"...","data":{...}}
{"type":"status","ts":"...","data":{...}}
```

---

## `--dry-run` 应该怎么返回

这个点飞书很值得参考：**有副作用的命令先给 plan / preview**。([GitHub][1])

awiki 我建议 dry-run 一律返回 `plan`，而不是返回“不会真的执行”这种描述。

示例：

```json
{
  "ok": true,
  "data": {
    "plan": {
      "action": "send_message",
      "target": {
        "kind": "direct",
        "input": "alice",
        "resolved_handle": "alice.awiki.ai",
        "resolved_did": "did:wba:..."
      },
      "security": {
        "requested": "on",
        "mode": "e2ee",
        "session": "missing",
        "will_init": true
      },
      "mutations": [
        "remote:e2ee_init",
        "remote:e2ee_msg"
      ]
    }
  },
  "summary": "预演完成：将先建立 E2EE 会话，再发送消息。",
  "meta": {
    "dry_run": true
  }
}
```

这对 AI 特别重要，因为它可以先预览，再决定是否真的执行。

---

## `schema` 应该怎么配合返回

如果你做 `awiki schema`，那命令返回就应该是**命令元数据本身**，而不是说明文字。

示例：

```json
{
  "ok": true,
  "data": {
    "name": "awiki msg send",
    "summary": "Send a direct or group message",
    "side_effect": true,
    "dry_run_supported": true,
    "output_formats": ["json", "pretty", "table", "ndjson"],
    "args": [...],
    "returns": {
      "success": {...},
      "error": {...}
    }
  }
}
```

这样 assistant、MCP bridge、自动文档生成器都可以直接用。

---

## 要不要支持 `--jq`

我建议**要**。
飞书最新版本已经加了 `--jq`，这个非常值得直接抄思路。([GitHub][4])

比如：

```bash
awiki status --jq '.data.inbox.unread'
awiki msg inbox --jq '.data.messages[] | {from, text}'
awiki people search "agent" --jq '.data.items[0]'
```

这会极大提升：

* AI 调试效率
* shell 自动化能力
* 人类用户的 grep/jq 工作流体验

---

## exit code 也要一起设计

不要只设计 stdout，不设计退出码。

我建议：

* `0`：成功
* `1`：一般错误
* `2`：参数错误
* `3`：身份/认证缺失
* `4`：权限不足
* `5`：资源不存在
* `6`：部分成功
* `7`：需要确认但未提供 `--yes` / 非交互上下文不允许执行

飞书现在也明显不是只靠文本判断，而是把 exit code 和结构化输出一起用。([GitHub][1])

---

## shortcut 要不要影响返回格式

我建议可以，但要谨慎：

### canonical command

默认始终 JSON：

```bash
awiki msg send ...
awiki status
awiki msg inbox
```

### shortcut

可以偏人类友好，但最好仍支持显式切回 JSON：

```bash
awiki inbox                # 默认 pretty/table
awiki dm alice "hello"     # 默认 pretty
awiki inbox --format json
awiki dm alice "hello" --json
```

也就是说：

* **canonical 命令**：面向机器和 AI，默认 JSON
* **shortcut**：面向人，默认 pretty/table

这样既能借飞书 shortcut 的体验，又不会让协议层混乱。

---

## 最终建议

如果你要一个最明确的结论，我建议你这样定：

**1. awiki CLI 的标准返回是 JSON。**
**2. 自然语言不是返回协议，只是 `summary/message` 字段。**
**3. `pretty/table/ndjson` 都是 JSON 的视图。**
**4. 所有有副作用命令必须支持 `--dry-run`，并返回 `plan`。**
**5. 所有命令都支持 `--jq`。**
**6. shortcut 可以默认 pretty，但 canonical command 必须默认 JSON。**

一句话概括：

**飞书给你的最佳参考不是“返回自然语言”，而是“结构化结果对象 + 多种格式渲染 + dry-run + schema + jq”。awiki 最适合沿着这条路走。**

你要的话，我下一条可以直接把 **awiki v2 输出协议规范** 单独起草出来，包括字段定义、错误码、不同命令的 `data` 结构，以及 human / pretty / table / ndjson 的渲染规则。

[1]: https://github.com/larksuite/cli/blob/main/README.md?utm_source=chatgpt.com "cli/README.md at main · larksuite/cli"
[2]: https://github.com/larksuite/cli/blob/main/skills/lark-shared/SKILL.md "cli/skills/lark-shared/SKILL.md at main · larksuite/cli · GitHub"
[3]: https://github.com/larksuite/cli/issues/127?utm_source=chatgpt.com "Bug: 飞书文档写入存在问题 docs +update/+create loses ..."
[4]: https://github.com/larksuite/cli/blob/main/CHANGELOG.md "cli/CHANGELOG.md at main · larksuite/cli · GitHub"
