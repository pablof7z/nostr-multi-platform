use super::*;

const VALID: &str = "1111111111111111111111111111111111111111111111111111111111111111";

#[test]
fn accepts_a_64_hex_event_id() {
    let target = ZapTarget::event(VALID).expect("valid hex64 id");
    assert_eq!(target.event_id(), VALID);
}

#[test]
fn trims_whitespace() {
    let target = ZapTarget::event(format!("  {VALID}  ")).expect("trims whitespace");
    assert_eq!(target.event_id(), VALID);
}

#[test]
fn rejects_short_ids() {
    let err = ZapTarget::event("deadbeef").unwrap_err();
    assert_eq!(err, ZapTargetError::InvalidEventId);
}

#[test]
fn rejects_non_hex_ids() {
    let bad = "g".repeat(64);
    let err = ZapTarget::event(bad).unwrap_err();
    assert_eq!(err, ZapTargetError::InvalidEventId);
}

#[test]
fn rejects_empty_ids() {
    let err = ZapTarget::event("").unwrap_err();
    assert_eq!(err, ZapTargetError::InvalidEventId);
}

#[test]
fn error_carries_a_stable_code() {
    assert_eq!(ZapTargetError::InvalidEventId.code(), "invalid_event_id");
}
