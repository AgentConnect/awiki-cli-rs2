use super::records::{merge_hydration_state, normalize_hydration_state};

#[test]
fn recovered_identity_merge_preserves_hydration_gap_state() {
    assert_eq!(normalize_hydration_state(""), "hydrated");
    assert_eq!(normalize_hydration_state("invalid"), "discovered");
    assert_eq!(
        merge_hydration_state("legacy_probe", "discovered"),
        "discovered"
    );
    assert_eq!(merge_hydration_state("discovered", "hydrated"), "hydrated");
    assert_eq!(
        merge_hydration_state("legacy_probe", "legacy_probe"),
        "legacy_probe"
    );
}
