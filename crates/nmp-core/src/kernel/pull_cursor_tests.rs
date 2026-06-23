//! Unit tests for pull-cursor registry types and `PullCursorSpec` validation.
//! Extracted from `pull_cursor.rs` to keep that file under the LOC ceiling.

use std::num::NonZeroUsize;

use super::*;
use crate::kernel::pull::{PullLimits, PullScope};

fn limits(max_entries: usize, max_scan: usize) -> PullLimits {
    PullLimits {
        max_entries: NonZeroUsize::new(max_entries).unwrap(),
        max_scan_entries: NonZeroUsize::new(max_scan).unwrap(),
    }
}

fn spec(max_entries: usize, max_scan: usize) -> PullCursorSpec {
    PullCursorSpec {
        consumer_id: PullConsumerId("test".into()),
        scope: PullScope::GlobalLog,
        mode: PullCursorMode::GapAllowed,
        after_seq: 0,
        limits: limits(max_entries, max_scan),
    }
}

#[test]
fn validate_ok_when_entries_le_scan() {
    assert!(spec(64, 256).validate().is_ok());
    assert!(spec(256, 256).validate().is_ok(), "equal is valid");
}

#[test]
fn validate_err_when_entries_gt_scan() {
    let err = spec(257, 256).validate().unwrap_err();
    assert_eq!(
        err,
        InvalidCursorSpec::LimitsOutOfOrder { max_entries: 257, max_scan_entries: 256 }
    );
}

#[test]
fn alloc_handle_yields_sequential_nonzero_ids() {
    let mut reg = PullCursorRegistry::new();
    let h1 = reg.alloc_handle();
    let h2 = reg.alloc_handle();
    assert_ne!(h1, h2, "each alloc yields a distinct handle");
    assert_ne!(h1.id().0, 0, "id 0 is never allocated");
    assert_ne!(h2.id().0, 0);
    assert!(h2.id().0 > h1.id().0, "ids are strictly increasing");
}

#[test]
fn pull_consumer_id_display_and_from() {
    let id = PullConsumerId::from("mirror");
    assert_eq!(id.to_string(), "mirror");
    let id2: PullConsumerId = "feed".to_string().into();
    assert_eq!(id2.0, "feed");
}
