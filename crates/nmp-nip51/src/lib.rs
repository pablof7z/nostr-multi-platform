//! `nmp-nip51` — NIP-51 mute-list adapter for the NMP substrate.
//!
//! # Scope
//!
//! NIP-51 specifies a family of "curated set" event kinds. This crate
//! implements the **kind:10000 public mute list** only — the v1 safety
//! requirement. Post-v1 NIP-51 kinds (kind:10001 pin list, kind:10003
//! bookmarks, etc.) are out of scope.
//!
//! | Wire kind | Name          | NIP    | Status   |
//! |-----------|---------------|--------|----------|
//! | 10000     | Public mute   | NIP-51 | Shipped  |
//! | 10001+    | Other lists   | NIP-51 | Post-v1  |
//!
//! # Architecture
//!
//! The crate exposes one type, [`MuteListProjection`], which is both a
//! [`nmp_core::KernelEventObserver`] (the write side — ingest kind:10000
//! events) and a [`nmp_core::substrate::SuppressionLookup`] implementation
//! (the read side — answer "is this author/event muted?" queries from the
//! timeline projection).
//!
//! The substrate-generic [`SuppressionLookup`] trait lives in `nmp-core` so
//! `nmp-nip01`'s `ModularTimelineProjection` can depend on it without creating
//! a `nmp-nip01 → nmp-nip51` edge (which would be a Layer-4 sibling
//! dependency, forbidden by the crate-boundary spec). At composition time the
//! host wires:
//!
//! ```text
//! let mute = Arc::new(MuteListProjection::new(Arc::clone(&active_pubkey_slot)));
//! app.register_event_observer(Arc::clone(&mute) as Arc<dyn KernelEventObserver>);
//! timeline.set_suppression(Arc::clone(&mute) as Arc<dyn SuppressionLookup>);
//! ```
//!
//! # D0 — namespace hygiene
//!
//! `nmp-core` sees no NIP-51 nouns. The substrate trait is `SuppressionLookup`
//! with methods `is_suppressed_author` / `is_suppressed_event`. The NIP-51
//! kind number (10000) is a constant local to this crate.
//!
//! # Public tags only
//!
//! NIP-51 allows private mutes in the NIP-44 encrypted `content` field. This
//! crate only parses public `p` and `e` tags. Private-mute decryption requires
//! the active signer and is post-v1.
//!
//! # Relationship to `nmp-wot`
//!
//! `nmp-wot` also ingests kind:10000 events to populate its `WotGraph` for
//! follow-graph scoring. The two crates serve different consumers:
//! `nmp-nip51` serves the **timeline suppression** (hard mute — hide the card
//! entirely), `nmp-wot` serves **trust scoring** (soft signal — deprioritize
//! in a ranked feed). The duplication of the kind:10000 `p`-tag parse is an
//! acknowledged overlap tracked as a BACKLOG follow-up (see V-42 note in
//! `docs/BACKLOG.md`). Consolidating both onto `nmp-nip51`'s decode would
//! require `nmp-wot` to depend on `nmp-nip51` — a legal Layer-4 sibling edge
//! per the spec. That consolidation is a future clean-up step, not v1 scope.

pub mod projection;

pub use projection::{MuteListProjection, MuteListSnapshot};
