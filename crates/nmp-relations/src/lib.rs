//! `nmp-relations` — legacy cross-protocol note-relation classifier for NMP.
//!
//! The base note crate (`nmp-nip01`) owns note/profile/reply primitives and the
//! relation-count vocabulary, but it must not own *cross-protocol* product
//! aggregation (#1728). This crate is the temporary home for that classifier:
//!
//! - [`DefaultNoteRelationClassifier`] — the concrete
//!   [`nmp_nip01::NoteRelationClassifier`] that tallies reactions (NIP-25),
//!   reposts (NIP-18), zaps (NIP-57), and comments (NIP-22) onto a note. Inject
//!   it via [`default_note_relation_classifier`] into
//!   `nmp_nip01::NoteRelationIndex::new` or
//!   `nmp_nip01::ModularTimelineProjection::with_relation_classifier`.
//!
//! Dependency direction is one-way: `nmp-relations → nmp-nip01` (+ the NIP-18 /
//! NIP-22 / NIP-57 sources). `nmp-nip01` never depends back on this crate.

mod classifier;

pub use classifier::{default_note_relation_classifier, DefaultNoteRelationClassifier};
