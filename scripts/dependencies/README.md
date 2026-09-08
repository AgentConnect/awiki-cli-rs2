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
