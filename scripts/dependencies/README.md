# SDK 依赖来源与消费端构建

统一入口是 `python3 scripts/dependencies/build.py`。默认 Debug 使用正式 registry 锁；
个人联调和跨仓 PR 必须显式选择，不根据平台、当前目录旁是否存在仓库自动切换。
这里的 Release 指交付入口；直接 `cargo build --release` 仍只是本地优化构建。

```bash
# 在修改中的 CLI 工作区使用已发布 SDK，不要求先提交消费者代码。
python3 scripts/dependencies/build.py --check
# 本地路径相对于配置文件目录；把 example 复制到仓库根目录后再修改。
cp scripts/dependencies/local.example.json dependencies.local.json
python3 scripts/dependencies/build.py --deps local --local-config dependencies.local.json --check
# 支持 awiki-cli / awiki-deamon / im-core-dart / awiki-im-core-node。
python3 scripts/dependencies/build.py --package awiki-im-core-node --check
# 正式构建：只允许 registry，要求已提交源码。
python3 scripts/dependencies/build.py --profile release --package awiki-cli
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
清单和锁文件要随 PR 提交；个人路径配置、缓存、构建结果不提交。CI 将源 SHA 检出到隔离目录，
使用联调锁执行 `--locked`，检查实际来源，不能把 registry fallback 当成选中源码。
源 PR 代码只在无发布凭据的普通 pull_request job 中执行，不使用 pull_request_target。

`registry-check` 始终独立运行；`source-integration-check` 在存在清单时执行。
源码联调通过不等于已发布依赖通过。依赖 PR 合并、发布新包后，更新
[registry-dependencies.json](../release/registry-dependencies.json) 和
[registry-Cargo.lock](../release/registry-Cargo.lock)，删除临时 source 清单/锁，
重新通过 registry 检查再合并消费者 PR。正式发布入口拒绝未撤销的 source 清单。

上海集成分支的 `dependencies.source.json` 固定 Identity PR #6 和本仓 Core PR #30 的
已推送提交，具体完整 SHA 以清单为准。Core 固定包含运行时修复的提交，后续清单维护不要求
追逐同仓元数据提交，避免移动分支和自引用来源。
`dependencies.source.Cargo.lock` 由上述 source 入口生成；配套 source check 与
registry check 仍独立执行，不把未发布的 SDK 候选当成已发布依赖。

## 已有发布入口

CLI、Daemon、Node 制品流程继续使用 [registry-build.py](../release/registry-build.py)。
App 打包 worker 已强制 `AWIKI_RELEASE_REGISTRY=1`，Flutter 原生脚本因此使用同一 registry
构建检查；直接运行原生 SDK 开发脚本仍允许源码构建。Dart wrapper 的仓内 path 是宿主源码，
不能作为 Rust SDK 来源证明，必须检查实际 Cargo metadata。

当前 `registry-dependencies.json` 已声明目标候选版本：Identity `0.2.2`、Core `0.1.2`；
`registry-Cargo.lock` 仍保存旧的正式解析结果：Identity `0.2.1`、Core `0.1.1`。
两者尚未完成正式依赖升级，不能将此状态视为 registry 构建通过。旧 Core 不提供当前消费者
需要的 `im_core::compat::identity_index` 与 `inspect_handle_recovery_context` 等接口，
因此不能把目标版本退回旧版来凑过来源检查。

正式交付前先合并并发布含所需 API 的 SDK，再通过现有 registry 入口重新生成匹配的锁，
更新正式 pin/lock、撤销 source 清单并通过 registry 检查。发布前保留真实的旧锁和明确的
阻断状态，不手写未发布包的 checksum，也不将源码联调锁冒充正式锁。

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
当前任务只修复和验证源码，不执行 SDK 发布；正式打包仍须完成上面的依赖交付步骤。

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
