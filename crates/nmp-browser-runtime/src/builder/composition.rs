use super::{BrowserAppBuilder, ProvidersDecided};

pub(crate) fn install_browser_production_composition(
    app: &mut BrowserAppBuilder<ProvidersDecided>,
) {
    claim_browser_production_composition(app);

    let _substrate = nmp_substrate::install(app, nmp_substrate::SubstrateConfig::default());

    nmp_nip50::register_search_scopes(app);
    nmp_nip50::register_input_scopes(app);

    nmp_nip02::register_follow_actions(app);
    nmp_replies::register_actions(app);
    nmp_core::substrate::ProtocolDescriptor::register_actions(&nmp_nip25::Nip25Descriptor, app);
    nmp_core::substrate::ProtocolDescriptor::register_actions(&nmp_nip18::Nip18Descriptor, app);
    nmp_core::substrate::ProtocolDescriptor::register_actions(&nmp_nip84::Nip84Descriptor, app);
    nmp_nip29::register_input_scopes(app);

    let _wot = nmp_wot::register_runtime(app);
    let _mute = nmp_nip51::register_mute_runtime(app);
    let _bookmarks = nmp_nip51::register_bookmark_runtime(app);
    nmp_nip51::register_bookmark_set_runtime(app);
    nmp_nip51::register_web_bookmark_runtime(app);
    let _search_relays = nmp_nip51::register_search_relay_runtime_with_fallbacks(
        app,
        nmp_nip50::SearchFallbackRelays::default(),
    );
    let _comments = nmp_nip22::register_runtime(app);

    nmp_nip17::register_actions(app);
    nmp_nip17::register_runtime(app);

    nmp_nip23::register_longform_projection(app);
}

fn claim_browser_production_composition(app: &BrowserAppBuilder<ProvidersDecided>) {
    let Ok(mut inner) = app.inner.lock() else {
        return;
    };
    assert!(
        !inner.production_composition_installed,
        "BrowserAppBuilder production composition must be installed exactly once"
    );
    inner.production_composition_installed = true;
}
