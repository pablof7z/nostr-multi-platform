//! Typed [`ActionPayload`](nmp_core::substrate::ActionPayload) types for the
//! action namespaces [`register_defaults`](crate::register_defaults) wires.
//!
//! ## Why this lives here (ADR-0064 / Cut-B, #1756)
//!
//! A default-only composition root (e.g. `nmp-app-gallery`) dispatches writes
//! through the typed BYTE doorway
//! [`nmp_ffi::nmp_app_dispatch_action_bytes`], which requires encoding each
//! action's canonical body into its typed `ActionPayload` FlatBuffers bytes.
//! That encode step must name the concrete per-NIP payload type for every
//! namespace — but the D0 boundary forbids a generic showcase app from
//! depending on the per-NIP crates directly (it names only `nmp-defaults`,
//! `nmp-core`, `nmp-ffi`).
//!
//! `nmp-defaults` already depends on exactly the per-NIP crates whose action
//! modules `register_defaults` installs, so it is the single, correct home for
//! the payload-type surface that matches that registration. Re-exporting the
//! types here keeps "which namespaces the default bundle dispatches" and "which
//! typed payloads encode them" as ONE fact, owned by the same crate that wires
//! the modules — a leaf app builds its byte-doorway seam on these names and
//! never reaches past `nmp-defaults`.
//!
//! Only the namespaces `register_defaults` actually registers appear here. A
//! namespace the default bundle does not install (e.g. NIP-29 groups, which
//! `register_defaults` never wires) is deliberately absent: a default-only app
//! cannot dispatch it, so exposing its payload type would be dead surface.

pub use nmp_core::publish::PublishAction;
pub use nmp_nip02::{FollowManyAction, PubkeyAction};
pub use nmp_nip17::{PublishDmRelayListInput, SendDmInput};
pub use nmp_nip18::RepostAction;
pub use nmp_nip22::PostCommentAction;
pub use nmp_nip25::{ReactAction, UnreactAction};
pub use nmp_nip51::BookmarkUpdateInput;
pub use nmp_nip57::ZapInput;
pub use nmp_nip84::PublishHighlightAction;
pub use nmp_router::{BlockRelayInput, PublishRelayListInput, UnblockRelayInput};
