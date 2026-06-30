//! `nmp-relations` — cross-protocol note-relation composition for NMP.
//!
//! The base note crate (`nmp-nip01`) owns note/profile/reply primitives and the
//! relation-count vocabulary, but it must not own *cross-protocol* product
//! aggregation (#1728). This crate owns the reusable cross-protocol pieces:
//!
//! - [`DefaultNoteRelationClassifier`] — the concrete
//!   [`nmp_nip01::NoteRelationClassifier`] that tallies reactions (NIP-25),
//!   reposts (NIP-18), and comments (NIP-22) onto a note. Inject
//!   it via [`default_note_relation_classifier`] into
//!   `nmp_nip01::NoteRelationIndex::new` or
//!   `nmp_nip01::ModularTimelineProjection::with_relation_classifier`.
//! - [`VisibleNoteRelationsModule`] — the action module a visible note row
//!   claims/releases to keep relation counts live. It opens typed interests for
//!   replies, reactions, reposts, zaps, and NIP-22 comments without app shells
//!   reconstructing protocol filters.
//!
//! Dependency direction is one-way: `nmp-relations → nmp-nip01` (+ the NIP-18 /
//! NIP-22 / NIP-57 sources). `nmp-nip01` never depends back on this crate.

pub mod action;
mod classifier;
pub mod ownership;
mod visible_relations;
mod wire;

pub use action::{
    register_actions, register_visible_note_relation_actions, RelationsDescriptor,
    VisibleNoteRelationsAction, VisibleNoteRelationsLifecycle, VisibleNoteRelationsModule,
    VISIBLE_NOTE_RELATIONS_NAMESPACE,
};
pub use classifier::{default_note_relation_classifier, DefaultNoteRelationClassifier};
pub use visible_relations::{
    validate_visible_note_relations_action, visible_note_relation_interests,
    VisibleNoteRelationInterest,
};
