# `@awiki/im-core-node` 原生制品

## 当前决策状态

以下五个平台只是实施方案的候选矩阵，不代表部署负责人已经确认 Tier 1：

当前 DeepSeek Harness 的 Python runtime release 列出 Linux x64、Linux arm64、macOS arm64
三项，但 Harness 主 CI 还覆盖 macOS 与 Windows。这里保留主方案的五项候选，不静默把“已有
CI 覆盖”解释成“正式部署 Tier 1”；负责人需要明确选择三平台发行集合或五平台发行集合。

| target | npm optional package | CI runner | 最低边界 |
| --- | --- | --- | --- |
| `linux-x64-gnu` | `@awiki/im-core-node-linux-x64-gnu` | `ubuntu-22.04` + manylinux 2.28 x64 container | glibc symbol `<= 2.28` |
| `linux-arm64-gnu` | `@awiki/im-core-node-linux-arm64-gnu` | `ubuntu-22.04-arm` + manylinux 2.28 arm64 container | glibc symbol `<= 2.28` |
| `darwin-x64` | `@awiki/im-core-node-darwin-x64` | `macos-15-intel` | `MACOSX_DEPLOYMENT_TARGET=11.0` |
| `darwin-arm64` | `@awiki/im-core-node-darwin-arm64` | `macos-15` | `MACOSX_DEPLOYMENT_TARGET=11.0` |
| `win32-x64-msvc` | `@awiki/im-core-node-win32-x64-msvc` | `windows-2022` | MSVC ABI |

当前 Harness 是否正式支持 Alpine 尚未确认，因此不生成 musl 包。loader 会把 musl 识别为
`linux-<arch>-musl` 并返回 `unsupported_platform`，绝不会错误加载 glibc 包、运行期下载或回退
TypeScript SDK。

本机实际部署环境为 Ubuntu 22.04 / glibc 2.35，但 Harness 的 Linux runtime 发行边界是
manylinux 2.28。候选 Linux addon 因此在 manylinux 2.28 容器内从头构建，并拒绝任何高于
`GLIBC_2.28` 的符号；不能把普通 Ubuntu 22.04 构建冒充通用 Harness 制品。Tier 1 最低系统
由部署负责人确认后，仍须重新评审这个边界。

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

## 临时 artifact 构建

`.github/workflows/im-core-node-artifacts.yml` 使用 Rust 1.88、Node 22.19 和 pnpm 10.27 构建
五个平台候选包，并在各自真实架构 runner 上用 Node 22.19、24、26 做以下验证：

1. 只安装 wrapper tarball 和当前平台 tarball，且使用 `--offline --ignore-scripts`；
2. ESM import；
3. 创建空 state root client；
4. 调用 `getDefaultIdentity()` fixture operation；
5. 显式 `close()` 并让 Node 自然退出。

本机 Linux x64 临时包可这样生成；输出只能用于测试：

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

## 发行门禁

当前 `provenance.json` 强制记录
`temporary-test-artifact-only-license-approval-not-recorded`，CI 只上传三天保留期的临时测试
artifact。仓库没有 npm publish job。

只有以下条件全部完成后，才允许另行增加发布流程：

1. 部署负责人书面确认 Tier 1、Alpine/musl 范围和最低 glibc/macOS/Windows 基线；
2. 产品/法务选择 AGPL 发行并确认对应源码提供方式，或提供覆盖实际分发方式的商业授权；
3. 候选矩阵全部成功，并复核每个 tarball 的 SBOM、checksum 和 provenance；
4. 先发布全部平台包，再发布同版本 wrapper，最后用实际 registry 做 packed install；
5. 上述批准和验证记录落库后，才允许 `dsh-awiki` 引用该发行 channel。
