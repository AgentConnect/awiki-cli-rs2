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

## 已有发布入口

CLI、Daemon、Node 制品流程继续使用 [registry-build.py](../release/registry-build.py)。
App 打包 worker 已强制 `AWIKI_RELEASE_REGISTRY=1`，Flutter 原生脚本因此使用同一 registry
构建检查；直接运行原生 SDK 开发脚本仍允许源码构建。Dart wrapper 的仓内 path 是宿主源码，
不能作为 Rust SDK 来源证明，必须检查实际 Cargo metadata。

截至本轮验证，当前 CLI 需要的 `im_core::compat::identity_index` 尚不在固定的线上
`awiki-im-core 0.1.1` 中。默认 registry 编译会明确报接口缺失；本地 Core 可联调。
需要先发布包含该接口的 Core 版本，再更新 registry pin/lock。不要以本地替换绕过发布门禁。

### 2026-09-08 Recovery 选择性吸收

源码来源锁与 CLI 配置的 Identity 输入统一为 `244e74ce`（`0.2.2`）：保留上海候选运行时，
同时合入 Release 的 SDK 测试路径修复；显式 fixture 目录仍可覆盖默认解析路径。源码树
摘要由 owner 的复制/哈希规则从该提交重新计算。App 打包的 ANP 输入与本仓 `246d69e2`
一致。Identity 的独立测试路径修复不构成新的运行时兼容接口。
这些 Git 来源修正不改变正式 registry 规则，也不表示已发布 SDK 自动包含工作区的新 API。
如固定 registry SDK 不提供当前消费者所需能力，应先发布新版本、更新 registry pin/lock
并重跑来源检查；不得使用源码替换或同版本覆盖发布绕过该门禁。

0714 合成恢复测试的本地 DID domain 与 Handle 保持一致，模拟远端同时提供新增的权威
Handle binding 查询。它保留正式 fixture 摘要、普通数据守恒、旧 E2EE 退役及 exact operation
续跑断言，不通过跳过权威读取或放宽 publication 校验兼容旧测试。

本轮只读核对 crates.io：最新 `awiki-im-core` 仍为 `0.1.1`，该已发布源码不含
`inspect_handle_recovery_context`。因此没有把 registry pin 改成不存在的版本，也没有关闭
正式来源门禁。本机显式 local 构建可验证本次修改，但正式打包仍须先完成 SDK 发布与 pin/lock
更新；当前任务不执行该发布。
