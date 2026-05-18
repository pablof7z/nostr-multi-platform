//! Shared test helpers — synthetic `StoredEvent`s without the cost of full
//! Schnorr verification (we use `RawEvent` directly inside an `Arc`).
//!
//! Cargo compiles each test target as its own binary; helpers used by some but
//! not all tests trip `dead_code` per-binary. The blanket allow is the
//! conventional fix for shared test-utility modules (rust-lang/rust#46379).

#![allow(dead_code)]

use nmp_core::store::{RawEvent, StoredEvent};
use std::sync::Arc;

pub const KIND_REACTION: u32 = 7;
pub const KIND_REPOST: u32 = 6;
pub const KIND_GENERIC_REPOST: u32 = 16;

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

/// A kind:7 reaction on `target_id` (an `e` tag) by `author`.
pub fn reaction(
    id: &str,
    author: &str,
    created_at: u64,
    target_id: &str,
    target_author: &str,
    content: &str,
) -> StoredEvent {
    stored(
        id,
        author,
        KIND_REACTION,
        created_at,
        vec![
            vec!["e".into(), target_id.into()],
            vec!["p".into(), target_author.into()],
        ],
        content,
    )
}

/// A kind:6 repost of `target_id`.
pub fn repost(
    id: &str,
    author: &str,
    created_at: u64,
    target_id: &str,
    target_author: &str,
    embedded: &str,
) -> StoredEvent {
    stored(
        id,
        author,
        KIND_REPOST,
        created_at,
        vec![
            vec!["e".into(), target_id.into()],
            vec!["p".into(), target_author.into()],
        ],
        embedded,
    )
}

/// A kind:16 generic repost of `target_id` carrying original kind `k`.
pub fn generic_repost(
    id: &str,
    author: &str,
    created_at: u64,
    target_id: &str,
    target_author: &str,
    original_kind: u32,
) -> StoredEvent {
    stored(
        id,
        author,
        KIND_GENERIC_REPOST,
        created_at,
        vec![
            vec!["e".into(), target_id.into()],
            vec!["p".into(), target_author.into()],
            vec!["k".into(), original_kind.to_string()],
        ],
        "",
    )
}
