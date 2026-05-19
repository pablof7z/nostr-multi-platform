//! Shared fixtures for LMDB-backend parity tests.
//!
//! Lives outside the `#[cfg(test)]` partition only because two sibling test
//! files (`tests.rs`, `tests_kind5.rs`) share the same builders. Kept under
//! `#![cfg(feature = "lmdb-backend")]` so it never compiles without the
//! feature.

#![cfg(all(test, feature = "lmdb-backend"))]

use tempfile::tempdir;

use crate::store::types::{RawEvent, VerifiedEvent};
use crate::store::LmdbEventStore;

pub(super) fn open_tmp() -> (LmdbEventStore, tempfile::TempDir) {
    let dir = tempdir().expect("tempdir");
    let store = LmdbEventStore::open(dir.path()).expect("open");
    (store, dir)
}

pub(super) fn signed_event(
    kind: u32,
    created_at: u64,
    content: &str,
    d_tag: Option<&str>,
) -> RawEvent {
    use nostr::prelude::*;
    let keys = Keys::generate();
    let mut b = EventBuilder::new(Kind::from(kind as u16), content)
        .custom_created_at(Timestamp::from_secs(created_at));
    if let Some(d) = d_tag {
        b = b.tag(Tag::identifier(d));
    }
    let ev = b.sign_with_keys(&keys).expect("sign");
    let json = ev.try_as_json().expect("json");
    serde_json::from_str(&json).expect("parse")
}

pub(super) fn signed_event_with_keys(
    keys: &nostr::Keys,
    kind: u32,
    created_at: u64,
    content: &str,
    d_tag: Option<&str>,
) -> RawEvent {
    use nostr::prelude::*;
    let mut b = EventBuilder::new(Kind::from(kind as u16), content)
        .custom_created_at(Timestamp::from_secs(created_at));
    if let Some(d) = d_tag {
        b = b.tag(Tag::identifier(d));
    }
    let ev = b.sign_with_keys(keys).expect("sign");
    let json = ev.try_as_json().expect("json");
    serde_json::from_str(&json).expect("parse")
}

pub(super) fn verified(raw: RawEvent) -> VerifiedEvent {
    VerifiedEvent::from_raw_unchecked(raw)
}
