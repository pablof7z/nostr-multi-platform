//! `nmp-relations` — legacy compatibility note-relation classifier for NMP.
//!
//! This crate keeps the old injected classifier compiling, but it is not the
//! canonical owner of engagement semantics. Reply, reaction, repost, zap,
//! bookmark, and mute concepts belong to the crates/modules that know those
//! protocol shapes; apps should ask those concept owners for the active reads
//! they need instead of routing through a global relation summary.
//!
//! - [`DefaultNoteRelationClassifier`] — the concrete
//!   [`nmp_nip01::NoteRelationClassifier`] that tallies reactions (NIP-25),
//!   reposts (NIP-18), and comments (NIP-22) onto a note. Inject
//!   it via [`default_note_relation_classifier`] into
//!   `nmp_nip01::NoteRelationIndex::new` or
//!   `nmp_nip01::ModularTimelineProjection::with_relation_classifier`.
//!
//! Dependency direction is one-way: `nmp-relations → nmp-nip01` (+ the NIP-18 /
//! NIP-22 sources). `nmp-nip01` never depends back on this crate.

mod classifier;

pub use classifier::{default_note_relation_classifier, DefaultNoteRelationClassifier};

nmp_ownership::declare_crate_ownership! {
    owner_id: "nmp.relations",
    crate_name: "nmp-relations",
    summary: "Legacy compatibility adapter for the old injected note-relation classifier. It declares no protected semantics.",
    claims: [
    ],
    notes: [
    ],
}
