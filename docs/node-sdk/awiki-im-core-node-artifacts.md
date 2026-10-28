# `@awiki/im-core-node` 原生制品

本次协调发布版本为 `0.2.15`，固定使用 IM Core `0.1.12`，native API 为 `21`。wrapper 与五个平台包须共同发布为 `0.2.15`；以下旧版本记录保留历史边界，不能用旧版 addon 替代当前包。
源码消费者通过显式 `--local-candidate`
打包 private tarball；provenance 与 SOURCE 采用实际解析的 ANP/Identity 提交和 dirty 状态，
并记录 Cargo lock 摘要。native API 从当前 Rust facade 读取。该候选验证不替代正式 registry
及平台发布检查，正式配置中的历史 ANP SHA 不可冒充本次本地依赖。
`workflow_dispatch` 默认 `build_only=true`，仅构建和打包，不执行安装测试；全部构建成功后仍聚合完整六包候选。只有用户明确要求安装测试时才在手动触发中选择 `build_only=false`。push/PR CI 仍保留安装测试；未运行测试的手动制品不得标作安装验收通过。

## 第一版发行决策

部署负责人已在 2026-08-15 确认第一版 Tier 1 为以下五个平台，并明确排除 Alpine/musl。
发行模式已批准为 Apache-2.0；每个包都必须携带对应源码定位、license、notices、SBOM、
checksum 和 provenance。

| target | npm optional package | CI runner | 最低边界 |
| --- | --- | --- | --- |
| `linux-x64-gnu` | `@awiki/im-core-node-linux-x64-gnu` | `ubuntu-22.04` + manylinux 2.28 x64 container | kernel 4.18、glibc 2.28 |
| `linux-arm64-gnu` | `@awiki/im-core-node-linux-arm64-gnu` | `ubuntu-22.04-arm` + manylinux 2.28 arm64 container | kernel 4.18、glibc 2.28 |
| `darwin-x64` | `@awiki/im-core-node-darwin-x64` | `macos-15-intel` | macOS 13.5 |
| `darwin-arm64` | `@awiki/im-core-node-darwin-arm64` | `macos-15` | macOS 13.5 |
| `win32-x64-msvc` | `@awiki/im-core-node-win32-x64-msvc` | `windows-2022` | Windows 10 / Server 2016 |

第一版不生成 musl 包。loader 会把 musl 识别为
`linux-<arch>-musl` 并返回 `unsupported_platform`，绝不会错误加载 glibc 包、运行期下载或回退
TypeScript SDK。

registry wrapper/platform package `0.1.5` 的 native contract version 为 `5`，包含 external HTTP
auth、群管理、展示资料、local conversation timeline、realtime 与 mail facade。当前
registry wrapper/platform package `0.1.6` 增加 Profile、完整群成员管理、P9 mention、
Payload send 与 Handle Recovery，native contract 为 `8`；`0.1.7` 增加 Host-only recovery
attestation，native contract 为 `9`。`0.1.8` candidate 增加 DSH Device Join 和设备管理 facade，
native contract 升为 `10`；当时 `release/0815` 的 `0.2.0` candidate 在同一 v10 contract 上继续
增加 External Identity Provider bridge。发布时必须同时发布 wrapper、全部 Tier 1 addon，并让
provenance 与 packed-install 测试统一为 v10，不能混装旧 addon。

最低边界取 Node 24/26 和 Rust 目标共同支持范围中更严格的一侧，并只承诺仍处于厂商支持期的
系统。Node 24/26 的官方平台表把 Linux x64/arm64 基线定为 kernel 4.18、glibc 2.28，把 macOS
x64/arm64 定为 13.5，把 Windows x64 定为 Windows 10 / Server 2016。Linux addon 因此在
manylinux 2.28 容器内从头构建，并拒绝任何高于 `GLIBC_2.28` 的符号；macOS 构建固定
`MACOSX_DEPLOYMENT_TARGET=13.5` 并检查 Mach-O `minos`。Node 26 在 Linux 上还要求系统提供
`libatomic` runtime；这是 Harness Node 运行时自身的前置条件。Windows addon 在官方
`windows-2022` runner 使用 MSVC 构建和加载；Windows 10 / Server 2016 是 Node 与 Rust 的
共同最低运行合同。

上游依据：

- Node 24：<https://github.com/nodejs/node/blob/v24.x/BUILDING.md#platform-list>
- Node 26：<https://github.com/nodejs/node/blob/v26.x/BUILDING.md#platform-list>
- Rust Windows MSVC：<https://doc.rust-lang.org/stable/rustc/platform-support/windows-msvc.html>
- GitHub runner labels：<https://github.com/actions/runner-images>

## 包边界

root wrapper 是纯 ESM 包，只包含编译后的 JS、类型声明和合规元数据，不内嵌 `.node`。每个
平台包只包含一个目标 `.node`，以及：

- `LICENSE`、`COMMERCIAL-LICENSING.md`、`NOTICE.md`；
- 对应源码 commit 和 ANP commit 的 `SOURCE.md`；
- `provenance.json`；
- CycloneDX 1.6 `sbom.cdx.json`；
- 包内逐文件 `checksums.json`，以及 tarball 外部 `.sha256`。

所有包都没有 `preinstall`、`install` 或 `postinstall`。安装和运行阶段不会调用 Cargo、Rust、
编译器、下载脚本或 sibling checkout。

`0.1.8` 及 `0.2.0` 历史 wrapper 与五个平台包共同实现 native API v10；provenance 中的
`nativeApiVersion` 必须为 `10`，旧 addon 必须由 loader 明确拒绝。

## Apache-2.0 artifact 构建

自 2026-09-07 起，CI 通过 `scripts/release/registry-build.py --prepare` 创建独立的 registry
构建工作区，使用 `scripts/release/registry-dependencies.json` 和 `registry-Cargo.lock`
锁定已发布的 ANP、Identity、IM Core。二进制输出仍进入原 checkout 的 target 目录，
原 checkout 用于对应源码定位和打包审核。普通本地源码构建流程保持可用。
Cargo 的精确约束（例如 `=1.0.1`）在 provenance 中记为实际版本 `1.0.1`。


`.github/workflows/im-core-node-artifacts.yml` 使用 Rust 1.88、Node 22.19 和 pnpm 10.27 构建
五个平台包，并在各自真实架构 runner 上用 Node 22.19、24、26 做以下验证：

1. 只安装 wrapper tarball 和当前平台 tarball，且使用 `--offline --ignore-scripts`；
2. ESM import；
3. 创建空 state root client；
4. 调用 `getDefaultIdentity()` fixture operation；
5. 显式 `close()` 并让 Node 自然退出。

本机 Linux x64 测试包可这样生成：

```bash
docker run --rm \
  --user "$(id -u):$(id -g)" \
  -e HOME="$HOME" -e CARGO_HOME="$HOME/.cargo" -e RUSTUP_HOME="$HOME/.rustup" \
  -e CARGO_TARGET_DIR="$PWD/target/manylinux-2-28" \
  -v "$PWD/../../..:$PWD/../../.." -v "$HOME/.cargo:$HOME/.cargo" \
  -v "$HOME/.rustup:$HOME/.rustup:ro" -w "$PWD" \
  quay.io/pypa/manylinux_2_28_x86_64 \
  bash -c '"$CARGO_HOME/bin/cargo" build --locked --release \
    -p awiki-im-core-node --target x86_64-unknown-linux-gnu'
pnpm --filter @awiki/im-core-node run build:typescript
node scripts/release/node-sdk/stage-package.mjs \
  --kind platform \
  --package-dir packages/awiki-im-core-node-platforms/linux-x64-gnu \
  --target linux-x64-gnu \
  --binary target/manylinux-2-28/x86_64-unknown-linux-gnu/release/libawiki_im_core_node.so \
  --output dist/node-sdk/staged/linux-x64-gnu
node scripts/release/node-sdk/pack-audit.mjs \
  --package-dir dist/node-sdk/staged/linux-x64-gnu \
  --destination dist/node-sdk/tarballs
```

wrapper 使用同一脚本的 `--kind wrapper`。`pack-audit.mjs` 会拒绝源码、测试、构建脚本、
内嵌于 wrapper 的 `.node`、缺失的 license/SBOM/provenance/checksum，以及任何安装 hook。

## Apache-2.0 artifact channel

`provenance.json` 强制记录 `apache-2.0`。workflow 先产出五个平台包，
再产出同版本 wrapper；构建与打包审计通过后聚合上传名为
`im-core-node-apache-artifacts-<run-id>` 的 GitHub Actions artifact，保留 30 天。只有本轮
明确选择安装测试时，聚合还要求全部 Node/平台 packed-install 通过；默认手动构建的制品
没有这项测试证据。聚合包包含六个 tarball 及各自 SHA-256，可供 Step 04 的 `dsh-awiki`
按显式测试要求做安装验证。

仓库不包含自动 npm publish job。若后续需要正式 npm registry，必须把同一组已验证 tarball
按“全部平台包 → root wrapper”的顺序发布，再从该 registry 做一次 clean install；不能重建
另一组无对应 provenance 的二进制。


## 2026-10-13 补齐 npm 发布

`@awiki/im-core-node@0.2.10` 与五个同版本平台包使用
[owner workflow 38376037118](https://github.com/AgentConnect/awiki-cli-rs2/actions/runs/38376037118)
的完整候选发布。绑定源码为 `6b7eea16e098ab8d663dceb651aa8b26a6b10a3f`，
构建选择 `dependency_source=registry`、`build_only=true`，native API 为 `20`。
实际 Rust 运行时依赖仍为已发布的 Core `0.1.7`（源码 `de7d138ca0c8d9b9fa26fe0345da71b5523ec055`）、
ANP `1.0.6` 和 Identity `0.2.5`。绑定源码提交不能替代这些 registry SDK 的源码定位；
本次没有重新发布 Rust SDK，也不将后续 Core 源码改动宣称为已进入旧版 crate。

发布复用 `awiki-harness/scripts/publish-packages.py` 的 `im-core-npm` 目标。
首次 `--npm-stage all` 已上传五个平台包，但 npm 异步处理尚未完成，平台可见性门禁
阻止了主包上传。核对五包均可见且 SHA-512 与原候选相同后，使用同一 run 的
`--npm-stage wrapper` 续跑，未重复上传平台包。

发布边界复核沿用[合并 PR #58](https://github.com/AgentConnect/awiki-cli-rs2/pull/58)
的授权、根签名、CAS、精确资源范围及不确定结果拒绝边界记录。本次绑定代码与上一轮
候选相比只更新版本和 Core registry pin；`check-node-identity-boundary.mjs` 通过，
未扩大密钥导出或身份存储权限。六包来源、外部与内部摘要、完整平台集合、SBOM、许可证
及无安装 hook 的制品校验均由原脚本执行。

本次未运行 SDK 测试套件、packed-install、产品 E2E 或安装运行测试；五个平台的构建
和打包成功不表述为跨平台产品验收。未发布 CLI/Daemon/App 下载通道或部署服务。


## 2026-10-13 群附件修复制品发布

本次发布 Core Rust `0.1.8` 和 Core Node `0.2.11`，将原始协议消息 ID 的群附件票据修复
纳入正式 registry 运行时，而非只更新 Node 绑定源码。

- Core Rust 源码：`325bb6c8e5d01a56040d69b0b71041e3721fed44`；新建不可变标签
  `im-core-v0.1.8`，包含修复 `6b7eea16e098ab8d663dceb651aa8b26a6b10a3f`。
- crates.io crate SHA-256：`c19e17218e115a861791c7a404b2c478ef570527c0b8558a0cc0014dc1864c9a`。
  实际下载的 crate 与 registry checksum、本地发布包、`.cargo_vcs_info.json` 及修复文件内容一致。
- Node 构建源码：`924fa8b4cf825031ee2d1a98ebee8653568b4ad9`，
  [workflow 38389493885](https://github.com/AgentConnect/awiki-cli-rs2/actions/runs/38389493885)。
  五个平台的日志分别确认 registry 解析和实际编译均为 `awiki-im-core v0.1.8`。
- 六包 provenance 均为同一构建 SHA、Core `0.1.8`、native API `20`；tarball、包内文件和
  原生二进制校验通过。平台先发布并确认 registry 完整性后，才发布主包。
- npm 新增 `release-0.2.11` 标签；既有 `latest` 仍为 `0.2.10`，其他既有标签也保持原值。
  后续消费者使用精确版本 `@awiki/im-core-node@0.2.11`，本次没有切换产品推荐政策。

完整的 crate 来源、registry lock 摘要、五个平台的构建证据、六包 SHA-256、npm SHA-512、
原生二进制 SHA-256、registry 下载地址及标签核对信息见
[发布回执](releases/core-0.1.8-node-0.2.11.json)。

发布复用统一脚本的 `im-core-rust` 和 `im-core-npm` 目标；未重发 ANP、Identity 或既有版本，
未移动旧 Git 标签。本次只执行构建、打包、来源与完整性校验，`build_only=true`，
未运行 SDK 测试、packed-install 或产品 E2E。CLI、Daemon、App、DSH 产品插件发布和
生产推荐政策切换由后续产品发布任务负责。


## 独立产品标识的验证与并行约束

Core 0.1.9 / Core Node 0.2.12 的标识适配仅扩展允许的产品字符串，不改变鉴权、身份角色、
版本格式或请求头的同源／产品路径判断。非法标识与调用方覆盖受管 header 的拒绝边界保留；
公共 ANP、公开发现和对象存储等外部请求不附带产品版本信息。

定向验证使用临时状态和 loopback 假服务，不访问生产数据库或真实账号：Core 测试捕获产品
HTTP 请求以及同步／异步 WebSocket 握手；真实 Node 原生契约验证五个产品、七种非法值、
五次产品 HTTP 请求和五次无产品头的公开发现。`verify-packed-install.mjs` 在安装候选包后
执行同一原生契约，registry 发布后还须独立安装并核对实际加载的原生二进制来源和摘要。

本批重型任务上限为 6。构建和安装矩阵分别设置 `max-parallel: 5`，为发布侧或本地任务
保留一个槽位；阶段由 `needs` 串行衔接，矩阵运行时不叠加额外本地重型构建。
本次显式运行安装测试，Actions 选择 `dependency_source=registry`、`build_only=false`。

## 2026-10-13 独立产品标识正式发布

Core Rust `0.1.9` 与 Core Node 主包及五个平台包 `0.2.12` 已正式发布并完成 registry 校验。
Core 新增 `awiki-dsh`、`awiki-gateway`，保留原有三个产品；DSH 使用插件包版本，
Guest 使用 Gateway 包版本。Node 类型声明同步扩展，桥接继续复用 Core 的严格校验。

- Core 源码：`375acee14bb47e4fb7f773c3a4c437ac084ff58e`，新标签 `im-core-v0.1.9`。
  crate SHA-256：`bd9324062c4c36fc28eef8a531ed6ac0e32aeb77bd478e68b8c8eae4ac26d2e3`。
  从 registry 下载后核对来源与 checksum；114 个群附件及身份运行时相关文件与上一版
  `0.1.8` 制品逐字节一致，保留已有修复。
- Node 构建源码：`83091560165a291d443d6a8b14156747103c944f`；
  [Actions 38410019338](https://github.com/AgentConnect/awiki-cli-rs2/actions/runs/38410019338)
  使用 registry 模式成功完成五个平台构建和 15/15 packed-install。
  五个平台日志均确认解析及编译 `awiki-im-core v0.1.9`；native API 保持 `20`。
- 本地 Core 定向测试 11/11、真实 Node native 契约测试 1/1、TypeScript 正反类型断言通过。
  覆盖五个合法产品、七种非法输入、产品 HTTP、同步／异步 WebSocket，以及公共 ANP、
  公开发现和外部请求不附带产品头。所有请求使用临时状态和测试服务，无真实账号操作。
- npm 先上传平台包，等待异步处理完成并逐一下载核验后再发布主包。六包的 tarball 摘要、
  包内 checksum、provenance、精确依赖和五个原生二进制摘要一致。
  仅新增 `release-0.2.12`；`latest=0.2.10`、`release-0.2.11=0.2.11` 及其他旧标签保持原值。
- 发布后在工作区之外的 Linux x64 临时目录从 npm 安装精确版本，禁用安装脚本，令 Rust
  构建工具不可用。实际加载平台包 `.node`，核对其 SHA-256、native API 和 lock integrity，
  再验证五种产品、七种非法值、五次产品 HTTP 和五次无产品头的公开发现，全部通过。

完整源码、crate checksum、六个 npm integrity、五个 native SHA-256、构建与安装测试证据见
[发布回执](releases/core-0.1.9-node-0.2.12.json)。跨平台证据来自 Actions 安装矩阵；
正式 registry 的实际 native 安装验证平台为 Linux x64。本次未执行全量系统测试或产品 E2E，
未发布消费者产品、修改下载／最低版本政策或部署服务，也未重发 Identity SDK。
