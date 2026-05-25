# Legacy Path Baseline

This file is the F0 allowlist for the final CLI shell cutover. It records current legacy-path offenders that are still present while Workstream A burns them down. The static gate in `crates/awiki-cli/tests/legacy_path_cutover_contract.rs` compares this table with a live source scan:

- A new offender outside this table fails the gate.
- Removing an offender requires reducing or deleting the matching row here.
- This baseline is not a license to add new default-path legacy behavior.

| Area | File | Needle | Count | Reason | Removal PR |
| --- | --- | --- | ---: | --- | --- |
