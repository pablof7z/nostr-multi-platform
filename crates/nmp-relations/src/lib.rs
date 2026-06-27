//! `nmp-relations` — cross-protocol social-relation aggregation for NMP.
//!
//! The base note crate (`nmp-nip01`) owns note/profile/reply primitives and the
//! relation-count vocabulary, but it must not own *cross-protocol* product
//! aggregation (#1728). This crate is the home for that aggregation:
//!
//! - [`DefaultNoteRelationClassifier`] — the concrete
//!   [`nmp_nip01::NoteRelationClassifier`] that tallies reactions (NIP-25),
//!   reposts (NIP-18), zaps (NIP-57), and comments (NIP-22) onto a note. Inject
//!   it via [`default_note_relation_classifier`] into
//!   `nmp_nip01::NoteRelationIndex::new` or
//!   `nmp_nip01::ModularTimelineProjection::with_relation_classifier`.
//! - [`VisibleNoteRelationsModule`] — the `nmp.nip01.visible_note_relations`
//!   action that opens a single tailing interest across every relation kind for
//!   one note.
//!
//! Dependency direction is one-way: `nmp-relations → nmp-nip01` (+ the NIP-18 /
//! NIP-22 / NIP-57 sources). `nmp-nip01` never depends back on this crate.

mod classifier;
mod notifications;
mod visible_relations;
mod wire;

pub use classifier::{default_note_relation_classifier, DefaultNoteRelationClassifier};
pub use notifications::{
    notifications_interest_shape, NotificationKind, NotificationRow, NotificationsProjection,
    NotificationsSnapshot, NOTIFICATIONS_FILE_IDENTIFIER, NOTIFICATIONS_KEY, NOTIFICATIONS_LIMIT,
    NOTIFICATIONS_SCHEMA_ID, NOTIFICATIONS_SCHEMA_VERSION,
};
pub use visible_relations::{
    register_visible_note_relation_actions, visible_note_relations_identity,
    visible_note_relations_interest, visible_note_relations_interest_id,
    VisibleNoteRelationsAction, VisibleNoteRelationsModule, VISIBLE_NOTE_RELATIONS_LIMIT,
    VISIBLE_NOTE_RELATIONS_NAMESPACE,
};
pub use wire::{encode_notifications_snapshot, notifications_file_identifier};
