# Hermes Host Notify V1 代码改动评审说明

**文档作用**
- 这份文档给 reviewer 快速理解“这次到底改了什么、风险在哪里、如何验收”。
- 面向代码评审，不替代架构与部署文档。

---

## 1. 变更目标

在不影响现有 OpenClaw 功能的前提下，新增 Hermes host_notify 接入能力，并保留必要兼容能力。

目标分解：
1. 新增 `hermes` sink 与适配逻辑。
2. 保留旧 `webhook` 配置/命令兼容路径。
3. 文档与契约落地（schema/openapi + 联调文档）。

---

## 2. 代码改动范围

### 2.1 Runtime / Listener
- 新增 Hermes sink 实现：
  - `internal/runtime/listener/hermes_host_notify.go`
  - `internal/runtime/listener/hermes_host_notify_test.go`
- host_notify 分发增加 Hermes 分支，并保留 `webhook` alias：
  - `internal/runtime/listener/host_notify.go`
- 状态结构增加 `notify_url` 展示字段：
  - `internal/runtime/listener/types.go`

### 2.2 Config 解析与写入
- host_notify 配置新增 `hermes` 节点，同时保留 legacy `webhook` 节点读取：
  - `internal/config/config.go`
- 新增 Hermes 配置写入函数（并双写到 legacy webhook 字段）：
  - `internal/config/write.go`
- 对应测试更新：
  - `internal/config/config_test.go`
  - `internal/config/write_test.go`

### 2.3 CLI / 命令目录
- 新增 `runtime host-notify hermes ...` 命令路径：
  - `internal/cmdmeta/catalog.go`
  - `internal/cli/runtime.go`
  - `internal/cli/root.go`
- 兼容恢复：`runtime host-notify webhook ...` 作为 `hermes` alias 保留。
- 新增更适合 onboarding 的 Hermes 命令：
  - `runtime host-notify hermes guide`
  - `runtime host-notify hermes setup`
  - `runtime host-notify hermes status`
- `runtime host-notify hermes setup` 现已扩展为一键式本地接入：
  - 写 awiki-cli 自己的 host-notify 配置
  - 合并本地 `~/.hermes/config.yaml` 的 notify route
  - 去掉固定 `deliver_extra.chat_id`，改为依赖 Hermes home channel
  - 启动或重启本地 Hermes bridge service

### 2.4 辅助脚本
- 新增 Hermes 通知 adapter：
  - `scripts/hermes_notify_adapter.py`
- 保留 OpenClaw/回调辅助 server 的原命名，并兼容旧状态文件：
  - `scripts/host_notify_webhook_server.py`

---

## 3. 兼容性策略（重点）

### 3.1 对 OpenClaw 的兼容
- OpenClaw sink 分支仍独立，配置项仍走 `runtime.host_notify.openclaw.*`。
- OpenClaw route/token 读取与发送逻辑保持原行为。

### 3.2 对历史 webhook 命名的兼容
- `runtime.host_notify.sink=webhook` 仍接受，内部归一化到 `hermes`。
- CLI 旧入口 `runtime host-notify webhook ...` 仍可用（alias）。
- Hermes secret 支持新环境变量：`AWIKI_HOST_NOTIFY_HERMES_SECRET`，并兼容旧变量：`AWIKI_HOST_NOTIFY_WEBHOOK_SECRET`。

### 3.3 对脚本状态文件的兼容
- 默认优先使用旧文件：`host-notify-webhook-callbacks.json`。
- 若旧文件不存在且新文件存在，会自动回退读取新文件：`host-notify-hermes-callbacks.json`。

---

## 4. 风险评估

低风险点：
1. 旧命令/旧配置路径由 alias + fallback 覆盖，迁移阻力低。
2. Hermes 新增逻辑主要是新增分支，不是替换 OpenClaw 主链路。

潜在关注点：
1. 文档命名与命令命名已转向 `hermes`，外部脚本如果硬编码旧文案需确认。
2. 跨机联调依赖网络与时钟同步，易出现签名时效问题（非代码缺陷）。

---

## 5. 已执行验证

建议 reviewer 关注这几组测试：

```bash
go test -count=1 -v ./internal/runtime/listener -run 'Hermes|OpenClaw'
go test -count=1 -v ./internal/runtime/openclawnotify
go test -count=1 -v ./internal/cli -run 'Hermes|OpenClaw|HostNotify'
go test -count=1 -v ./internal/config -run 'Hermes|Webhook|OpenClaw'
python3 -m unittest discover -s scripts -p 'test_hermes_notify_adapter.py'
python3 -m py_compile scripts/hermes_notify_adapter.py scripts/host_notify_webhook_server.py
```

---

## 6. 建议评审清单

1. 行为一致性：`openclaw` sink 现网配置是否零回归。
2. 兼容入口：`runtime host-notify webhook ...` 是否可正常调用。
3. 安全性：签名校验、时间窗、去重是否符合预期。
4. 可运维性：`config show` 的 secret 是否脱敏，日志是否可排障。
5. 文档一致性：命令、配置、契约字段是否一致。

---

## 7. 关联文档

- 架构与契约：`docs/architecture/hermes-host-notify-v1.md`
- 部署联调：`docs/architecture/hermes-host-notify-v1-runbook.md`
- 契约：
  - `docs/architecture/contracts/notification-surface-v1.schema.json`
  - `docs/architecture/contracts/notify-hermes-v1.openapi.yaml`
