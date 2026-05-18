//! Shared test helpers — synthetic `StoredEvent`s without the cost of full
//! Schnorr verification (we use `RawEvent` directly inside an `Arc`).
//!
//! Cargo compiles each test target as its own binary; helpers used by some but
//! not all tests trip `dead_code` when imported per-binary. The blanket allow
//! is the conventional fix for shared test-utility modules
//! (rust-lang/rust#46379).

#![allow(dead_code)]

use nmp_core::store::{RawEvent, StoredEvent};
use std::sync::Arc;

pub fn stored(
    id: &str,
    pubkey: &str,
    kind: u32,
    created_at: u64,
    tags: Vec<Vec<String>>,
    content: &str,
) -> StoredEvent {
    StoredEvent {
        raw: Arc::new(RawEvent {
            id: id.into(),
            pubkey: pubkey.into(),
            created_at,
            kind,
            tags,
            content: content.into(),
            sig: "0".repeat(128),
        }),
        received_at_ms: 0,
    }
}

/// Replaceable list (10000 / 10002 / 10003) — no `d` tag.
pub fn list_event(
    id: &str,
    author: &str,
    kind: u32,
    created_at: u64,
    items: Vec<Vec<String>>,
    content: &str,
) -> StoredEvent {
    stored(id, author, kind, created_at, items, content)
}

/// Parameterized set (30000 / 30002 / 30003) — `d` tag required, prepended.
pub fn set_event(
    id: &str,
    author: &str,
    kind: u32,
    created_at: u64,
    d_tag: &str,
    items: Vec<Vec<String>>,
) -> StoredEvent {
    let mut tags = vec![vec!["d".into(), d_tag.into()]];
    tags.extend(items);
    stored(id, author, kind, created_at, tags, "")
}
