# File Size Exceptions

Current Rust file-size policy:

- Source files should target 2500 non-generated lines by default.
- Test files should target 3000 non-generated lines by default because CLI
  contract fixtures and local mock servers often stay more reviewable when kept
  with their focused scenario.
- Files above the applicable source/test limit are allowed as documented
  exceptions only. Record each exception in this file with a concrete reason
  instead of treating oversized files as the normal module shape.
- Historical verification notes may mention the older 1200-line review target;
  those notes are historical evidence, not the active policy.

| Rust path | Rust lines | Go path | Go lines | Reason |
| --- | ---: | --- | ---: | --- |
