# File Size Exceptions

Current Rust file-size policy:

- Source files should target at most 2500 non-generated lines by default.
- Test files should target at most 3000 non-generated lines by default because
  CLI contract fixtures and local mock servers often stay more reviewable when
  kept with their focused scenario.
- These limits are review-size defaults, not hard compiler constraints.
- Files may exceed the applicable source/test limit as documented exceptions.
  Record each exception in this file with a concrete reason. An exception
  records a deliberate review/maintenance tradeoff; it is not a new default for
  nearby files.
- Prefer splitting oversized source files first. Keep test files focused, but
  document intentional aggregation when it remains clearer.
- Historical verification notes may mention the older 1200-line review target;
  those notes are historical evidence, not the active policy. A test file above
  1200 lines does not need an exception unless it exceeds the active 3000-line
  test-file limit.

Record active exceptions below.

| Rust path | Rust lines | Go path | Go lines | Reason |
| --- | ---: | --- | ---: | --- |
