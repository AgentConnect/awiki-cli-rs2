# SDK 依赖来源与消费端构建

统一入口是 `python3 scripts/dependencies/build.py`。默认 Debug 使用正式 registry 锁；
个人联调和跨仓 PR 必须显式选择，不根据平台、当前目录旁是否存在仓库自动切换。
这里的 Release 指交付入口；直接 `cargo build --release` 仍只是本地优化构建。

```bash
# Debug 默认使用已发布 SDK，不要求先提交消费者代码。CLI 与 Daemon 是同一入口。
python3 scripts/dependencies/build.py --check
python3 scripts/dependencies/build.py --package awiki-deamon --check
# 只要把 ANP 改成源码：复制 example 到仓库根目录（路径相对该文件）。
cp scripts/dependencies/local-anp.example.json dependencies.local.json
python3 scripts/dependencies/build.py --deps local --local-config dependencies.local.json --check
# 同时替换 Core / Identity / ANP 时用 local.example.json。
cp scripts/dependencies/local.example.json dependencies.local.json
python3 scripts/dependencies/build.py --deps local --local-config dependencies.local.json --check
# 支持 awiki-cli / awiki-deamon / im-core-dart / awiki-im-core-node。
python3 scripts/dependencies/build.py --package awiki-im-core-node --check
# 正式构建：只允许 registry，要求已提交源码。
python3 scripts/dependencies/build.py --profile release --package awiki-cli
python3 scripts/dependencies/build.py --profile release --package awiki-deamon
```

`--resolve-only` 仅用于 Debug 依赖图诊断，不代替编译检查。编译结果与解析记录保存在
`.artifacts/dependencies/<mode>/`；Debug 包含消费者和选中 SDK 的未提交源码快照，
不会改原工作区的 Cargo.toml / Cargo.lock。个人 `dependencies.local.json` 已忽略。
未选择的 SDK 使用 registry；选中的 SDK 使用其真实包版本（允许下一版本/预发布版本），
在隔离副本中更新精确约束并校验实际 manifest 路径，失败不回退到旧包。

## 未合并的跨仓 PR

提交 `dependencies.source.json`，键只允许 `anp`、`anp-identity`、`awiki-im-core`。
源地址必须是无凭据 HTTPS，commit 必须是 CI 可拉取的完整 40 位 SHA；不使用移动分支名。
每个条目包含 `repository`、`commit`、`pull_request`，例如：

```json
{
  "schema_version": 1,
  "dependencies": {
    "anp-identity": {
      "repository": "https://github.com/your-org/anp-identity.git",
      "commit": "替换为实际可拉取的40位commit SHA",
      "pull_request": "https://github.com/your-org/anp-identity/pull/123"
    }
  }
}
```

先执行 `--deps source --source-manifest dependencies.source.json --refresh-lock`，提交生成的
`dependencies.source.Cargo.lock`；之后执行相同命令并将 `--refresh-lock` 换为 `--check`。
清单和锁文件要随 PR 提交；个人路径配置、缓存、构建结果不提交。按需启动的 CI 将源 SHA
检出到隔离目录，使用联调锁执行 `--locked`，检查实际来源，不能把 registry fallback
当成选中源码。用户于 2026-09-24 决定不因提交、更新或合并 PR 自动执行本仓
Dependency modes、Node CI 与 Tier 1 工作流；显式 `workflow_dispatch` 可选择 registry
或提交的 source 清单。不会通过 `pull_request_target` 执行不受信任源码。

`dependency-check` 按手动选择的 registry/source 来源运行；
`source-integration-check` 在存在清单时另行检查提交的源码来源。
源码联调通过不等于已发布依赖通过。依赖 PR 合并、发布新包后，更新
[registry-dependencies.json](../release/registry-dependencies.json) 和
[registry-Cargo.lock](../release/registry-Cargo.lock)，删除临时 source 清单/锁，
重新通过 registry 检查再合并消费者 PR。正式发布入口拒绝未撤销的 source 清单。

上海集成期间使用过固定 PR 提交的 source 清单和联调锁。相关 SDK 已发布后，
本仓已撤销这两份临时文件；后续源码联调仍使用上面的显式入口，正式 Release 使用 registry。

## 已有发布入口

CLI、Daemon、Node 制品流程继续使用 [registry-build.py](../release/registry-build.py)。
App 打包 worker 已强制 `AWIKI_RELEASE_REGISTRY=1`，Flutter 原生脚本因此使用同一 registry
构建检查；直接运行原生 SDK 开发脚本仍允许源码构建。Dart wrapper 的仓内 path 是宿主源码，
不能作为 Rust SDK 来源证明，必须检查实际 Cargo metadata。

当前正式 registry 依赖使用已发布的 ANP `1.0.6`、Identity `0.2.5`、Core `0.1.8`。
Core `0.1.8` 包含群附件原始消息 ID 修复；Core Node `0.2.11` 的实际原生构建固定消费
这一组合，native API 为 `20`。临时源码联调清单已撤销，正式消费者必须使用对应
registry lock 和实际解析来源。包已发布不表示后续源码改动已进入同版本 SDK。
2026-09-17 用户授权的两平台 Daemon 临时验证包曾使用显式 `--local-core` 入口，
其独立锁文件不改变 SDK 的公开发布状态。
具体范围和复现步骤见 [Daemon 发布说明](../../docs/publish.md)。

2026-09-09 的历史发布按用户明确要求不运行测试；完成了发布构建、包校验、上传及 registry
版本/来源核验。以下此前执行的测试记录属于发布前的历史证据，不表示本次重新执行。

2026-09-11 SDK 发布已刷新上述精确版本与 registry lock；Node `0.2.6` 和 Dart/FFI
源码消费同一 Core `0.1.4`。用户要求停止后不再运行测试，后续仅执行发布构建、打包及
registry 元数据/校验值核验；Node 制品采用 `build_only=true`。

### 2026-09-08 Recovery 选择性吸收

源码来源锁与 CLI 配置的 Identity 输入统一为 `0f19cc3e`（`0.2.2`）：保留上海候选运行时，
同时合入 Release 的 SDK 测试路径修复；显式 fixture 目录仍可覆盖默认解析路径。源码树
摘要由 owner 的复制/哈希规则从该提交重新计算。App 打包的 ANP 输入与本仓 `246d69e2`
一致。Identity 的独立测试路径修复不构成新的运行时兼容接口。
这些 Git 来源修正不改变正式 registry 规则，也不表示已发布 SDK 自动包含工作区的新 API。
如固定 registry SDK 不提供当前消费者所需能力，应先发布新版本、更新 registry pin/lock
并重跑来源检查；不得使用源码替换或同版本覆盖发布绕过该门禁。

0714 合成恢复测试的本地 DID domain 与 Handle 保持一致，模拟远端同时提供新增的权威
Handle binding 查询。它保留正式 fixture 摘要、普通数据守恒、旧 E2EE 退役及 exact operation
续跑断言，不通过跳过权威读取或放宽 publication 校验兼容旧测试。

本机显式 local/source 构建验证候选源码与真实解析来源，不证明候选 SDK 已发布。
当时任务只修复和验证源码，未执行 SDK 发布；后续发布状态见上面的正式依赖说明。

### 2026-09-09 Review 修复验证

Recovery 的 quarantine 只接受可处理生命周期与未销毁密钥；运行时提前拒绝，SQLite 更新
条件再次保护删除终态。保留已尝试提交的审计、正常 quarantine/替代恢复和删除后新恢复，
不改变协议、公共 DTO、schema 44 或 custody ownership，不恢复已删除密钥，不增加远端调用。

本地 `cargo test -p awiki-im-core --lib --locked 'internal::identity_'`：348 通过；新用例
在修复前确认失败。覆盖完成删除后公开 quarantine API、删除与替代链、终态和已销毁密钥。
System 仓 `uv run pytest tests/non_did/test_handle_recovery_v1_contract.py -q`：24 通过，
失败 0、跳过 0；这是本地契约检查，不连接服务。CLI 发布配置 8 项、依赖入口 6 项检查通过。
Identity fixture 修复已同步到 CLI 配置与 ANP 来源锁，摘要由既有 owner 复制/哈希规则计算。
按本轮本地验证范围，未运行远端 Recovery 产品 E2E，也未发布 SDK；上述结果不替代正式
registry 来源验证或真实账号恢复验收。
更新清单后，按 `--deps source --source-manifest dependencies.source.json --refresh-lock`
重新生成联调锁，解析结果未变化；随后同入口 `--check` 通过，实际使用清单指定的新
Identity/Core 源码完成 CLI 检查，未使用旧 registry SDK 替代。

### 指定已提交消费者源码构建

registry-build.py使用指定提交的Git归档构建输入，不再创建worktree/clone。默认仍
要求tracked源码已提交。主目录存在其他任务修改时，可显式传
`--source-commit <完整40位SHA> -- cargo build --release -p awiki-cli`；仅消费该提交，
不会带入dirty文件，registry pin/lock与实际metadata检查保持。构建输入的
`.awiki-source.json`记录来源，AWIKI_CLI_COMMIT必须与该提交一致。
归档是不可用作开发的构建输入，修改仍只在主目录；这个选项不发布/提升任何下载渠道。

### 2026-09-23 注册与 Notify 基线源码联调

本注册 PR 已合入 `release/0910` 的 Notify Core 变更。临时
`dependencies.source.json` 固定 Core `090d2cda`（包含先前的注册 Core
`4023161e`）、ANP `6bd11e06` 和 Identity `65a79d26`，并配套提交联调锁。
ANP 与 Identity 的 review 链接是协调集成记录，不表示各自有独立 PR。

此组合保留 Notify 能力和注册所需的 DID Web / 邀请码能力。使用上文的
`--deps source --source-manifest dependencies.source.json --check` 验证真实解析和编译。
它只证明未发布源码集成；registry 门禁、Apple 原生制品、设备验收及正式发布
仍须各自验证。正式 registry 来源不得使用此临时清单，发布需单独授权。

### 2026-09-24 Handle 连续恢复修复联调

本次 source 清单固定 Core 修复提交 `d3ad7075`、ANP `1.0.4-rc.1`
标签提交 `4208d1ca`、Identity `0b6369fe`，对应 PR #48 与配套联调锁。
ANP/Identity 沿用本次手工验收的已提交源码，没有为恢复问题新增修改；
其 review 链接指向组合来源记录，不表示存在新的独立依赖 PR。
固定 Core 修复提交可避免消费端从旧的 source 清单构建出不含本次修复的 SDK。
APP 的 wrapper 来源应再固定到包含此清单与锁的 Core PR 提交。
本次仅提交修复与源码联调输入，公共 SDK 和正式 registry pin 未发布或升级。


### 本地同源 macOS 测试候选

`AWIKI_APPLE_LOCAL_CONFIG=/绝对路径/dependencies.local.json scripts/flutter/build-sdk-native.sh --macos-only --macos-arch arm64`
使用现有隔离 local 构建机制选择当前 Core、ANP 与 Identity 源码。必须先提交消费者和
所选依赖；制品核验拒绝未提交来源、变更的提交/配置/解析锁或原生库摘要。
解析锁及每架构构建证据保存在 `.artifacts/dependencies/local/`，不修改正式 manifest/lock。
此入口只允许 Debug 候选的优化原生库，Flutter APP 保持 Debug；与 `AWIKI_RELEASE_REGISTRY=1`
及 `AWIKI_APPLE_SOURCE_INTEGRATION=1` 互斥，不授予正式发布资格。

### 2026-10-11 加密、头像与系统验证汇总

当前 source 清单固定 Core 实现 `867c15707c0977b9d9b667af15a0ce2703c53626`
（[Core PR #57](https://github.com/AgentConnect/awiki-cli-rs2/pull/57)）及 ANP
`fb99a08072ae68e5f6cc49cfef379d3cc2ded807`
（[ANP PR #67](https://github.com/agent-network-protocol/anp/pull/67)）。
实现冻结后再提交依赖元数据，Core pin 不引用元数据提交本身，避免自引用循环。
Identity 仍为 `794f914fd651a66f9f517411e00f0bdb99962201`；其原 Core #56
组合审查链接保持不变，不表示新建了 Identity PR。

本轮通过 owning `--refresh-lock` 重新解析，配套 source locks 内容没有变化；
源码模式仍须以 frozen/locked 入口验证。此组合包含加密、头像和群生命周期修复，
不改变 registry 版本、最低版本或发布状态。正式消费仍须先发布兼容 SDK，更新
registry pin/lock 并撤销临时 source 清单；本轮不发布或部署。


源码模式同时支持只读 `tree --locked` 审计；使用与构建相同的冻结清单和锁文件，不能通过命令参数改用其他 workspace。

### 20261013 Release 整合组合

本次固定源码与关联 PR 以根目录 `dependencies.source.json` 为准；它同时包含新加坡发布修复与隔离系统测试修复。配套锁文件与清单一同提交。该清单仅用于显式源码联调；正式 registry 发布仍须先发布上游 SDK、更新固定版本与锁文件，再移除临时源码清单。旧测试制品、历史 PR 与批次下载地址不能作为此组合的替代来源。

## 2026-10-13 合并依赖收敛

本批 PR 评审后协调发布 ANP `1.0.6`、Identity Rust `0.2.5`、Core Rust `0.1.7` 与 Core Node `0.2.10`。正式消费者以 `registry-dependencies.json` 和 `registry-Cargo.lock` 为准；本批临时 source manifest/lock 已撤销。测试没有因发布而自动运行，发布阶段只保留构建、打包、来源与完整性检查；产品 E2E、安装包和人工验收另行执行。
