use super::*;

const ROOT: &str = "1111111111111111111111111111111111111111111111111111111111111111";

#[test]
fn accepts_a_bare_hex64_event_id() {
    let target = ReactionTarget::event(ROOT).unwrap();
    assert_eq!(target.as_str(), ROOT);
}

#[test]
fn trims_surrounding_whitespace() {
    let target = ReactionTarget::event(format!("  {ROOT}  ")).unwrap();
    assert_eq!(target.as_str(), ROOT);
}

#[test]
fn rejects_short_ids() {
    assert_eq!(
        ReactionTarget::event("deadbeef"),
        Err(ReactionTargetError::InvalidEventId)
    );
}

#[test]
fn rejects_non_hex_ids() {
    let bad = "g".repeat(64);
    assert_eq!(
        ReactionTarget::event(bad),
        Err(ReactionTargetError::InvalidEventId)
    );
}
