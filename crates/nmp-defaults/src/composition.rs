use nmp_core::substrate::{AppHost, ProtocolDescriptor};

use crate::{runtimes, NmpDefaultRuntimeHandles, SearchDefaults};

pub(crate) fn register_nip50_defaults(app: &mut impl AppHost) {
    nmp_nip50::register_search_scopes(app);
    nmp_nip50::register_input_scopes(app);
}

pub(crate) fn register_social_defaults(
    app: &mut impl AppHost,
    handles: &mut NmpDefaultRuntimeHandles,
    search_defaults: SearchDefaults,
) {
    nmp_nip02::register_follow_actions(app);
    ProtocolDescriptor::register_actions(&nmp_nip25::Nip25Descriptor, app);
    ProtocolDescriptor::register_actions(&nmp_nip18::Nip18Descriptor, app);
    ProtocolDescriptor::register_actions(&nmp_nip84::Nip84Descriptor, app);
    nmp_nip29::register_input_scopes(app);

    handles.wot = nmp_wot::register_runtime(app);
    handles.mute = Some(runtimes::register_mute_runtime(app));
    let _ = runtimes::register_bookmark_runtime(app);
    handles.search_relays = Some(runtimes::register_search_relay_runtime_with(
        app,
        search_defaults,
    ));
    let _ = runtimes::register_comment_runtime(app);
}

pub(crate) fn register_dm_defaults(app: &mut impl AppHost) {
    nmp_nip17::register_actions(app);
    runtimes::register_dm_runtime(app);
}

pub(crate) fn register_zap_defaults(app: &mut impl AppHost) {
    nmp_nip57::register_actions(app);
    runtimes::register_zap_receipts_runtime(app);
}
