//! Static concept-read rows for app-local facade generation (#2899).
//!
//! This table intentionally names concept crate paths as text. `nmp-codegen`
//! must not depend on concept crates; the generated file is compiled inside the
//! app facade crate that already composes the listed concept.

/// Target input shape for a generated concept-read `open_*` method.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TargetInput {
    /// The generated method receives JSON, decodes it through the concept
    /// crate's FFI-marshalable target decoder, then calls `open_*`.
    Json {
        /// Generated method argument name.
        arg_name: &'static str,
        /// Concept crate decoder function to import and call.
        decoder_fn: &'static str,
    },
    /// The generated method receives a plain event id string and passes it
    /// directly to the concept crate's `open_*` door.
    PlainString {
        /// Generated method argument name.
        arg_name: &'static str,
    },
}

/// One concept-owned read door that can be generated into an app facade.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConceptRead {
    /// App registry key.
    pub id: &'static str,
    /// Rust concept crate to import from, using Rust module syntax.
    pub rust_crate: &'static str,
    /// Concept crate `open_*` function.
    pub open_fn: &'static str,
    /// Concept crate `close_*` function.
    pub close_fn: &'static str,
    /// Concept crate read handle type with `into_parts` / `from_parts`.
    pub handle_type: &'static str,
    /// Generated target argument shape.
    pub target_input: TargetInput,
}

/// Default NMP concept-read rows. These are codegen facts only, not runtime
/// dependencies.
pub const CONCEPT_READS: &[ConceptRead] = &[
    ConceptRead {
        id: "replies",
        rust_crate: "nmp_replies",
        open_fn: "open_replies",
        close_fn: "close_replies",
        handle_type: "RepliesReadHandle",
        target_input: TargetInput::Json {
            arg_name: "target_json",
            decoder_fn: "decode_and_validate_reply_target",
        },
    },
    ConceptRead {
        id: "reactions",
        rust_crate: "nmp_reactions",
        open_fn: "open_reactions",
        close_fn: "close_reactions",
        handle_type: "ReactionsReadHandle",
        target_input: TargetInput::PlainString {
            arg_name: "target_event_id",
        },
    },
    ConceptRead {
        id: "reposts",
        rust_crate: "nmp_reposts",
        open_fn: "open_reposts",
        close_fn: "close_reposts",
        handle_type: "RepostsReadHandle",
        target_input: TargetInput::PlainString {
            arg_name: "target_event_id",
        },
    },
    ConceptRead {
        id: "zaps",
        rust_crate: "nmp_zaps",
        open_fn: "open_zaps",
        close_fn: "close_zaps",
        handle_type: "ZapsReadHandle",
        target_input: TargetInput::PlainString {
            arg_name: "target_event_id",
        },
    },
];

/// Resolve a registered concept-read row by app-registry key.
#[must_use]
pub fn concept_read_for(id: &str) -> Option<&'static ConceptRead> {
    CONCEPT_READS.iter().find(|read| read.id == id)
}
