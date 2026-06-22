//! WRITE-direction typed FlatBuffers action payload codecs for `nmp-relations`
//! (ADR-0064 / Cut-B producer gap #1756).
//!
//! [`action_payload`] holds the `ActionPayload` impl for
//! [`crate::VisibleNoteRelationsAction`] (`nmp.nip01.visible_note_relations`).
//!
//! The generated module below is intrinsically `unsafe` (every accessor reads a
//! raw `Table`); only the generated module opts back into `unsafe`. The
//! hand-written codec uses none.

macro_rules! generated_action_module {
    ($module:ident, $file:literal) => {
        #[allow(
            clippy::all,
            dead_code,
            deprecated,
            missing_docs,
            non_camel_case_types,
            non_snake_case,
            unsafe_code,
            unused_imports
        )]
        #[path = $file]
        pub mod $module;
    };
}

generated_action_module!(
    visible_note_relations_action_generated,
    "generated/visible_note_relations_action_generated.rs"
);

pub mod action_payload;
