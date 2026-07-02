//! Client-side web-of-trust support for NMP apps.
//!
//! The crate has two responsibilities:
//!
//! - score a local follow/mute graph without depending on any relay-side
//!   recommendation protocol;
//! - bootstrap that graph by pushing one exact, replaceable-kind interest for
//!   the active account's follow set.
//!
//! The crate-level `register` installer is wired by explicit owner composition,
//! so apps get the bootstrap through the same one-call protocol path as other
//! reusable NMP protocol crates.

mod installer;
pub mod interest;
pub mod runtime;
pub mod score;
pub mod wire;

pub use installer::{register, Config, Handles};
pub use interest::{
    active_follow_graph_identity, active_follow_graph_interest_id, follow_graph_interest,
    KIND_CONTACT_LIST, KIND_MUTE_LIST, KIND_PROFILE, KIND_RELAY_LIST, WOT_BOOTSTRAP_KINDS,
};
pub use runtime::{WotBootstrapRuntime, WotBootstrapSnapshot};
pub use score::{
    TrustDecision, WotGraph, WotGraphStats, DEFAULT_AUTO_HIDE_SCORE, DIRECT_FOLLOW_SCORE,
    FOLLOWED_MUTE_SCORE, SECOND_DEGREE_SCORE, SELF_MUTE_SCORE, SELF_SCORE,
};
pub use wire::typed_fb::{
    decode_wot_bootstrap, encode_wot_bootstrap, FILE_IDENTIFIER as WOT_BOOTSTRAP_FILE_IDENTIFIER,
    SCHEMA_ID as WOT_BOOTSTRAP_SCHEMA_ID, SCHEMA_VERSION as WOT_BOOTSTRAP_SCHEMA_VERSION,
};

/// Compiled ownership descriptor for crate-ownership reports.
pub mod ownership;
