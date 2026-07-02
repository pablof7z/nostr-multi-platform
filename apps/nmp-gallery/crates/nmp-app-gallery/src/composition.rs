use nmp_signer_iface::Nip55Permission;

pub fn install_gallery_composition(app: &mut impl nmp_core::substrate::AppHost) {
    let _substrate = nmp_substrate::install(app, nmp_substrate::SubstrateConfig::default());

    let _nip50 = nmp_nip50::register(app, nmp_nip50::Config::default())
        .expect("nmp-nip50 registration must not collide");
    let _nip02 = nmp_nip02::register(app, nmp_nip02::Config::default())
        .expect("nmp-nip02 registration must not collide");
    let _replies = nmp_replies::register(app, nmp_replies::Config::default())
        .expect("nmp-replies registration must not collide");
    let _nip25 = nmp_nip25::register(app, nmp_nip25::Config::default())
        .expect("nmp-nip25 registration must not collide");
    let _nip18 = nmp_nip18::register(app, nmp_nip18::Config::default())
        .expect("nmp-nip18 registration must not collide");
    let _nip84 = nmp_nip84::register(app, nmp_nip84::Config::default())
        .expect("nmp-nip84 registration must not collide");
    let _nip29 = nmp_nip29::register(app, nmp_nip29::Config::default())
        .expect("nmp-nip29 registration must not collide");
    let _wot = nmp_wot::register(app, nmp_wot::Config::default())
        .expect("nmp-wot registration must not collide");
    let _nip51 = nmp_nip51::register(
        app,
        nmp_nip51::Config {
            search_fallback_relays: nmp_nip50::SearchFallbackRelays::default(),
        },
    )
    .expect("nmp-nip51 registration must not collide");
    let _comments = nmp_nip22::register(app, nmp_nip22::Config::default())
        .expect("nmp-nip22 registration must not collide");
    let _nip17 = nmp_nip17::register(app, nmp_nip17::Config::default())
        .expect("nmp-nip17 registration must not collide");
    let _nip23 = nmp_nip23::register(app, nmp_nip23::Config::default())
        .expect("nmp-nip23 registration must not collide");
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
