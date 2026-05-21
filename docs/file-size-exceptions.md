# File Size Exceptions

Rust source files should target 2500 non-generated lines by default. Rust test files should target 3000 non-generated lines by default because CLI contract fixtures and local mock servers often stay more reviewable when kept with their focused scenario. Files above the applicable source/test limit are allowed as documented exceptions only and require a reason instead of becoming the normal module shape.

| Rust path | Rust lines | Go path | Go lines | Reason |
| --- | ---: | --- | ---: | --- |
