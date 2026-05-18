//! Shared test helpers — synthetic `StoredEvent`s without the cost of full
//! Schnorr verification (we use `RawEvent` directly inside an `Arc`).
//!
//! Cargo compiles each test target as its own binary; helpers used by some
//! but not all tests trip `dead_code` when imported per-binary. The blanket
//! allow on this module is the conventional fix for shared test-utility
//! modules (rust-lang/rust#46379).

#![allow(dead_code)]

use nmp_core::store::{RawEvent, StoredEvent};
use std::sync::Arc;

pub const KIND_ARTICLE: u32 = 30023;

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

pub fn article(
    id: &str,
    author: &str,
    created_at: u64,
    d_tag: &str,
    title: Option<&str>,
    published_at: Option<u64>,
    content: &str,
) -> StoredEvent {
    let mut tags = vec![vec!["d".into(), d_tag.into()]];
    if let Some(t) = title {
        tags.push(vec!["title".into(), t.into()]);
    }
    if let Some(ts) = published_at {
        tags.push(vec!["published_at".into(), ts.to_string()]);
    }
    stored(id, author, KIND_ARTICLE, created_at, tags, content)
}
