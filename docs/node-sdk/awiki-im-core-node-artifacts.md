# `@awiki/im-core-node` 原生制品

## 第一版发行决策

部署负责人已在 2026-08-15 确认第一版 Tier 1 为以下五个平台，并明确排除 Alpine/musl。
发行模式已批准为 AGPL-3.0-only；每个包都必须携带对应源码定位、license、notices、SBOM、
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

registry wrapper/platform package `0.1.3` 的 native contract version 为 `2`，包含 opaque
single-use external HTTP auth attempt。当前源码 candidate 增加 local conversation timeline，
native contract 升为 `3`；下一次正式 patch 必须同时发布 wrapper、全部 Tier 1 addon，并让
provenance `nativeApiVersion` 与 packed-install 测试统一为 v4，不能混装旧 addon 与 v4
wrapper。

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

## AGPL artifact 构建

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

## AGPL test channel

`provenance.json` 强制记录 `agpl-3.0-only-approved-test-channel`。workflow 先产出五个平台包，
再产出同版本 wrapper；全部 Node/平台 packed-install 验证通过后，才聚合上传名为
`im-core-node-agpl-test-channel-<run-id>` 的 GitHub Actions artifact，保留 30 天。聚合包包含
六个 tarball 及各自 SHA-256，是 Step 04 供 `dsh-awiki` 安装验证的批准 channel。

仓库不包含自动 npm publish job。若后续需要正式 npm registry，必须把同一组已验证 tarball
按“全部平台包 → root wrapper”的顺序发布，再从该 registry 做一次 clean install；不能重建
另一组无对应 provenance 的二进制。
