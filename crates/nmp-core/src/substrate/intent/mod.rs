//! Input-intent recognizer substrate (issue #1804) — the **noun-free** half of
//! the input-intent resolver.
//!
//! # What this is
//!
//! A single-box / paste / search field in a Nostr app receives one untyped
//! `String`. It may be a NIP-19/21 reference (`npub…`, `nostr:nevent…`), a relay
//! URL (`wss://…`), a NIP-05 identifier (`name@domain`), a secret to reject
//! (`nsec…`), or free text to feed to NIP-50 search. The **classification** of
//! that input — which of those it is, and which app-requested scope it satisfies
//! — is a pure, synchronous decision. This module owns the contract for that
//! decision; the orchestrator that runs it lives in the higher-order
//! [`nmp-intent`] crate, and all IO (NIP-05 HTTP, search REQs) lives in the
//! dispatch layer (the FFI / actor).
//!
//! # D0 — protocol-noun-free
//!
//! Like [`crate::substrate::search`], this module is the *registry + trait*
//! half. It MUST NOT name a protocol concept (no "npub", "nip05", "relay" as a
//! NIP noun, no kind numbers). Every protocol-specific target is carried as an
//! opaque [`InputIntentTarget`] variant whose payload is either an already-typed
//! generic field (a decoded entity URI, a normalized URL, a NIP-05-shaped
//! identifier) or opaque JSON. The protocol/app crates that name those nouns
//! implement [`InputScopeRecognizer`] and produce the targets.
//!
//! # Registration house style (ADR-0046 / ADR-0049 — NO linkme/inventory)
//!
//! A crate registers a recognizer through the [`InputScopeRegistrar`] trait on
//! the `AppHost`; the registry records a [`crate::Disposition`] in the
//! **`"input_scope"`** composition-ledger seam, and a duplicate
//! [`InputScopeId`] is a **yielding default** (the first registration keeps the
//! slot; the later one is recorded as `YieldedToExisting`, never silently
//! replaced).
//!
//! This is a **separate** registry from the FTS [`SearchScopeRegistry`]
//! (#1811): the two share only the namespaced-label vocabulary convention, not
//! the registry instance. FTS scopes declare *indexable storage*; input scopes
//! declare *what a recognizer can turn raw input into*.

mod id;
mod recognizer;
mod registry;
mod target;

pub use id::InputScopeId;
pub use recognizer::{InputScopeRecognizer, ResolvedInput, ResolvedInputKind, TextSearchTargets};
pub use registry::{
    InputScopeDisposition, InputScopeRegistrar, InputScopeRegistry, INPUT_SCOPE_LEDGER_SEAM,
};
pub use target::{
    InputIntentCandidate, InputIntentClassification, InputIntentRejection, InputIntentRequest,
    InputIntentTarget,
};
