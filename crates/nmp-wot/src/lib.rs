//! Client-side web-of-trust support for NMP apps.
//!
//! The crate has two responsibilities:
//!
//! - score a local follow/mute graph without depending on any relay-side
//!   recommendation protocol;
//! - bootstrap that graph by pushing one exact, replaceable-kind interest for
//!   the active account's follow set.
//!
//! `register_runtime` is wired by `nmp-app-template`, so apps such as Chirp get
//! the bootstrap through the normal `register_defaults` path.

pub mod interest;
pub mod runtime;
pub mod score;

pub use interest::{
    active_follow_graph_interest_id, follow_graph_interest, KIND_CONTACT_LIST, KIND_MUTE_LIST,
    KIND_PROFILE, KIND_RELAY_LIST, WOT_BOOTSTRAP_KINDS,
};
pub use runtime::{register_runtime, WotBootstrapRuntime};
pub use score::{TrustDecision, WotGraph};
