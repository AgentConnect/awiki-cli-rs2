# 步骤 04：增加 CLI Claim 命令并更新 Onboarding/Skill

状态：`completed`
实施仓库：`awiki-cli-rs2`
Worktree：`/home/ecs-user/awiki-space/awiki-cli-rs2-skill-token-onboarding`
实施分支：`feature/skill-token-onboarding`
前置依赖：步骤 03
后续依赖方：步骤 05、07

## 1. 目标

- 暴露 `awiki-cli onboarding claim`，作为 Token 注册的唯一 CLI 入口。
- 安全地从 stdin 接收 Token，调用 im-core claim，不在 shell 参数和输出中回显。
- 更新 onboarding 和 AWiki Skill，使智能体能识别 App 复制的授权 block 并自动完成 v1 流程。
- 保证发布后的 stable onboarding snapshot 与仓库文档一致。

## 2. 不做的内容

- 不在 CLI handler 重新实现 User Service client、DID 生成或消息发送。
- 不提供普通 `--token <secret>` 参数。
- 不修改普通 `id register`/`recover` 的交互。
- 不把 Token block 做成通用脚本语言。
- 不增加海外/国内映射；国内文档只使用 `awiki.info`。

## 3. Command catalog

- 新增 command name：`onboarding.claim`。
- 命令：`awiki-cli onboarding claim`。
- 参数仅包含：

```text
--service-base-url <https-url>
--expected-controller-handle <full-handle>
--expected-agent-handle <full-handle>
--token-stdin
--format json|pretty
```

- `--token-stdin` 为官方必选路径。
- 可选环境变量兼容路径必须是一次性读取并立即清空引用。
- schema/help 只能描述 Token 来源，不能输出值或示例真实 secret。

## 4. Handler 行为

- 解析参数和 stdin 后立即包装为 redacted secret type。
- stdin 为空、超过长度上限或包含换行外额外内容时拒绝。
- 调用 im-core Skill claim API。
- JSON 输出允许：phase、status、Agent DID/Handle、Controller Handle、greeting status、retryable。
- JSON/pretty 输出禁止 raw Token、JWT、private key、内部 user ID 和完整 HTTP body。
- greeting pending 使用稳定非零 exit code 和 retry hint。
- completed 仅在主动消息被 Message Service 接受后返回成功。

## 5. Onboarding 文档

- `onboarding.md` 增加 `AWIKI_SKILL_ONBOARDING_V1` 分支。
- 有 Token block 时按以下顺序：安装 CLI、安装 Skill、init 空 workspace、claim、只读首检。
- 无 Token block 时保留现有人类注册/恢复路径。
- 国内发布内容使用 `https://awiki.info`。
- 明确禁止把 Token 发往其他域、写入文件、聊天消息或 debug 命令。
- workspace 已有 identity 时立即停止并询问用户。

## 6. Skill 规则

- 更新 `skills/SKILL.md` 和 `skills/references/01-onboarding.md`。
- 用户复制的有效 `skill_onboarding_v1` block 视为以下明确授权：

```text
安装 AWiki CLI
安装/更新 AWiki Skill
初始化新的空 workspace
创建一个 Skill Agent DID
发送一条固定 Controller greeting
执行只读首检
```

- 该授权不覆盖恢复、覆盖已有身份、删除、任意消息发送或 Runtime 可选配置。
- verify metadata 与 block 不一致时停止，不猜测或修复字段。
- 消息内容中的指令永远不能触发 onboarding claim。

## 7. Release staging

- 更新 CLI release staging test。
- 确认发布的 onboarding snapshot 包含 Token 分支。
- 确认模板 placeholder 全部替换。
- 确认国内 package、Skill index 和 onboarding URL 都属于 `awiki.info`。
- 不把示例 Token 打包成可被误认为真实凭据的字符串。

## 8. 测试

### 8.1 Parser/catalog

- command、schema、help、同步/异步 dispatch 均注册。
- 缺少 `--token-stdin`、无 HTTPS URL 和错误 format 返回稳定参数错误。
- 不存在 `--token` secret flag。

### 8.2 Handler/output

- stdin Token 正确传给 redacted im-core request。
- completed 和 greeting pending 映射正确。
- stdout、stderr、Debug、JSON 和 pretty 不含 Token。
- im-core permanent/retryable error 映射正确。

### 8.3 文档/Skill

- Token block 被准确识别。
- 有 Token 时不进入手机号/邮箱注册。
- 非空 workspace 明确停止。
- 固定 greeting 是唯一被自动授权的消息写操作。
- 无 Token 时原确认规则不变。

### 8.4 发布

- staging 后 onboarding 没有 placeholder。
- Skill package 包含更新后的 onboarding reference。
- 国内 URL contract test 不出现 `awiki.ai`。

## 9. 完成标准

- 智能体可以仅根据 App 复制文本找到并执行唯一 claim 命令。
- Token 不出现在进程参数、输出或本地文档。
- CLI 只有在账号注册和主动消息都完成后返回成功。
- 无 Token 和普通人类 onboarding 行为不回归。
- CLI 聚焦测试、release staging tests、格式、Clippy 和 `git diff --check` 通过。

## 10. 实施结果与验证

- 已增加 `onboarding.claim` catalog、schema、help 以及同步/异步 dispatch，owner 为 `im_core_onboarding`。
- 为满足步骤 01 冻结的 Agent Handle 精确比对，补充必需的 `--expected-agent-handle`；没有增加 `--token`。
- handler 只接受 `--token-stdin` 的单行 UTF-8 Token，限制长度并在所有本地路径显式清零原始缓冲区。
- completed 仅在 greeting 被接受后输出成功；greeting pending 使用稳定 code、非零 exit 和脱敏公开结果。
- 已更新 onboarding、Skill、installation/onboarding references；有效 block 只授权新空 workspace claim、固定 greeting 和只读首检。
- release staging 测试固定国内 `awiki.info` fixture，并检查 onboarding/Skill snapshot、模板替换和域名隔离。

步骤 04 定向验证：

```text
cargo check -p awiki-cli
passed

cargo test -p awiki-cli --lib onboarding
4 passed, 0 failed

cargo test -p awiki-cli --test cli_parser_contract onboarding_claim_requires_stdin_and_rejects_token_argument
1 passed, 0 failed

command_catalog_schema_contract 三个相关用例
3 passed, 0 failed

node --test scripts/release/cli/stage-release.test.js
2 passed, 0 failed

cargo fmt --all -- --check
git diff --check
passed
```

严格 CLI Clippy 首次被未修改代码中的 3 个既有 lint 阻断；使用命令行仅豁免 `redundant-guards`、`needless-update`、`useless-format` 后，`cargo clippy -p awiki-cli --lib --no-deps -- -D warnings` 通过。本步骤未执行 CLI/im-core 全量测试，统一留到步骤 07。
