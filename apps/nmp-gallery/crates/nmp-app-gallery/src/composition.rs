use nmp_signer_iface::Nip55Permission;

pub fn install_gallery_composition(app: &mut impl nmp_core::substrate::AppHost) {
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

    register_gallery_embed_projection_adapters();
    nmp_nip23::register_longform_projection(app);
}

pub fn register_gallery_embed_projection_adapters() {
    nmp_nip23::register_content_embed_projection_adapter();
}

#[must_use]
pub(crate) fn gallery_nip55_permissions() -> Vec<Nip55Permission> {
    use Nip55Permission;
    vec![
        Nip55Permission::sign_event(0),
        Nip55Permission::sign_event(1),
        Nip55Permission::sign_event(3),
        Nip55Permission::sign_event(5),
        Nip55Permission::sign_event(6),
        Nip55Permission::sign_event(7),
        Nip55Permission::sign_event(13),
        Nip55Permission::sign_event(16),
        Nip55Permission::sign_event(1111),
        Nip55Permission::sign_event(9802),
        Nip55Permission::sign_event(10002),
        Nip55Permission::sign_event(10003),
        Nip55Permission::sign_event(10006),
        Nip55Permission::sign_event(10050),
        Nip55Permission::sign_event(30003),
        Nip55Permission::sign_event(30004),
        Nip55Permission::sign_event(39701),
        Nip55Permission::nip44_encrypt(),
        Nip55Permission::nip44_decrypt(),
    ]
}
