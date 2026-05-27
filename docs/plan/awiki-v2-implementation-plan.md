# awiki v2 Implementation Plan Archive

本文保留为历史入口，因为 `awiki-cli docs overview` 仍引用该路径。原始实施计划已完成，不再作为当前开发的事实来源。

当前实现已经从“CLI 重写实施计划”演进为“SDK + CLI thin shell”结构：

```text
crates/im-core       # IM SDK / 产品能力层
crates/awiki-cli     # CLI 产品壳
crates/im-core-dart  # Dart facade
packages/awiki_im_core
                    # Flutter package
```

当前稳定文档：

- `docs/README.md`
- `docs/architecture/awiki-v2-architecture.md`
- `docs/architecture/im-core-sdk-architecture.md`
- `docs/architecture/awiki-command-v2.md`
- `docs/architecture/output-format.md`
- `docs/installation.md`
- `docs/publish.md`

历史 phase、cutover、migration checklist、PR 验收流水和 Go parity 记录已从 `docs/` 主树清理。需要追溯原计划时，请使用 Git 历史。
