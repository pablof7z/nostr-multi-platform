use nmp_core::substrate::{AppHost, ProtocolDescriptor};

use crate::{runtimes, NmpDefaultRuntimeHandles, SearchDefaults};

/// Register reusable NIP-50 search and input scopes.
///
/// This is a named protocol installer for ADR-0069 explicit composition roots.
pub fn register_nip50_protocol_defaults(app: &mut impl AppHost) {
    nmp_nip50::register_search_scopes(app);
    nmp_nip50::register_input_scopes(app);
}

/// Register reusable social protocol defaults and return their read handles.
///
/// Includes NIP-02, NIP-18, NIP-25, NIP-29 input scopes, NIP-51/NIP-84
/// actions, and the host-side WOT, mute, bookmark, search-relay, and comment
/// runtimes. Leaf apps still own operator policy such as relay URLs.
pub fn register_social_protocol_defaults(
    app: &mut impl AppHost,
    search_defaults: SearchDefaults,
) -> NmpDefaultRuntimeHandles {
    let mut handles = NmpDefaultRuntimeHandles::default();
    nmp_nip02::register_follow_actions(app);
    ProtocolDescriptor::register_actions(&nmp_nip25::Nip25Descriptor, app);
    ProtocolDescriptor::register_actions(&nmp_nip18::Nip18Descriptor, app);
    ProtocolDescriptor::register_actions(&nmp_nip84::Nip84Descriptor, app);
    nmp_nip29::register_input_scopes(app);

    handles.wot = nmp_wot::register_runtime(app);
    handles.mute = Some(runtimes::register_mute_runtime(app));
    let _ = runtimes::register_bookmark_runtime(app);
    runtimes::register_bookmark_set_runtime(app);
    runtimes::register_web_bookmark_runtime(app);
    handles.search_relays = Some(runtimes::register_search_relay_runtime_with(
        app,
        search_defaults,
    ));
    let _ = runtimes::register_comment_runtime(app);
    handles
}

/// Register reusable DM protocol defaults: NIP-17 actions and DM runtime.
pub fn register_dm_protocol_defaults(app: &mut impl AppHost) {
    nmp_nip17::register_actions(app);
    runtimes::register_dm_runtime(app);
}
