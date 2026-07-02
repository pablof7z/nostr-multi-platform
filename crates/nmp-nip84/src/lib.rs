//! `nmp-nip84` — NIP-84 kind:9802 highlight publish action for NMP apps.
//!
//! This crate owns the public `nmp.nip84.publish_highlight` action surface: a
//! user intent to author a kind:9802 highlight event (NIP-84) over a source
//! Nostr event, an addressable event, or an external NIP-73 content identifier
//! (a podcast clip, a web page, …). The action threads the highlighted text,
//! optional surrounding context, attribution tags, and any number of NIP-73 `i`
//! and `k` tags into a single kind:9802 publish through the one-door publish
//! engine.

mod action;
mod external_id;
mod wire;

pub use action::{
    highlight_projection_from_event, HighlightProjection, PublishHighlightAction,
    PublishHighlightCommand, PublishHighlightModule, KIND_HIGHLIGHT,
};

/// Compiled ownership descriptor for crate-ownership reports.
pub mod ownership;

#[derive(Clone, Debug, Default)]
pub struct Config {}

#[derive(Clone, Debug, Default)]
pub struct Handles {}

pub fn register(
    app: &mut impl nmp_core::substrate::ActionRegistrar,
    _config: Config,
) -> Result<Handles, nmp_core::substrate::RegistrationError> {
    action::register_actions(app);
    Ok(Handles {})
}
