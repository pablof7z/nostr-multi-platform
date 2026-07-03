use std::sync::Arc;

use nmp_core::substrate::{
    ActionRegistrar, HostCapabilities, IdentityChangeRegistrar, ObservedProjectionRegistrar,
    PublishPolicyRegistrar, SnapshotProjectionRegistrar,
};
use nmp_nip50::SearchFallbackRelays;

use crate::{BookmarkListProjection, MuteListProjection, SearchRelayListProjection};

#[derive(Clone, Debug)]
pub struct Config {
    pub search_fallback_relays: SearchFallbackRelays,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            search_fallback_relays: SearchFallbackRelays::default(),
        }
    }
}

impl Config {
    #[must_use]
    pub fn new(search_fallback_relays: SearchFallbackRelays) -> Self {
        Self {
            search_fallback_relays,
        }
    }
}

#[derive(Clone)]
pub struct Handles {
    pub mute: Arc<MuteListProjection>,
    pub bookmarks: Arc<BookmarkListProjection>,
    pub search_relays: Arc<SearchRelayListProjection>,
}

impl std::fmt::Debug for Handles {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Handles")
            .field("mute", &"MuteListProjection")
            .field("bookmarks", &"BookmarkListProjection")
            .field("search_relays", &"SearchRelayListProjection")
            .finish()
    }
}

pub fn register(
    app: &mut (impl ActionRegistrar
              + ObservedProjectionRegistrar
              + HostCapabilities
              + SnapshotProjectionRegistrar
              + PublishPolicyRegistrar
              + IdentityChangeRegistrar),
    config: Config,
) -> Result<Handles, nmp_core::substrate::RegistrationError> {
    crate::declare_publish_policy(app).map_err(|_| nmp_core::substrate::RegistrationError {
        namespace: "publish_policy",
        prior_provider: "nmp-core::publish",
        new_provider: "nmp-nip51",
    })?;
    let mute = crate::runtime::register_mute_runtime(app);
    let bookmarks = crate::runtime::register_bookmark_runtime(app);
    crate::runtime::register_bookmark_set_runtime(app);
    crate::runtime::register_web_bookmark_runtime(app);
    let search_relays = crate::runtime::register_search_relay_runtime_with_fallbacks(
        app,
        config.search_fallback_relays,
    );

    Ok(Handles {
        mute,
        bookmarks,
        search_relays,
    })
}
