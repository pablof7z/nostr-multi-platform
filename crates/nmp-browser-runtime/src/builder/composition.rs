use super::{BrowserAppBuilder, ProvidersDecided};

pub(crate) fn install_browser_production_composition(
    app: &mut BrowserAppBuilder<ProvidersDecided>,
) {
    claim_browser_production_composition(app);

    let _substrate = nmp_substrate::install(app, nmp_substrate::SubstrateConfig::default());

    #[cfg(feature = "search")]
    {
        assert!(
            nmp_nip50::register(app, nmp_nip50::Config::default()).is_ok(),
            "nmp-nip50 registration must not collide"
        );
    }
    assert!(
        nmp_nip02::register(app, nmp_nip02::Config::default()).is_ok(),
        "nmp-nip02 registration must not collide"
    );
    assert!(
        nmp_replies::register(app, nmp_replies::Config::default()).is_ok(),
        "nmp-replies registration must not collide"
    );
    #[cfg(feature = "reactions")]
    {
        assert!(
            nmp_nip25::register(app, nmp_nip25::Config::default()).is_ok(),
            "nmp-nip25 registration must not collide"
        );
    }
    assert!(
        nmp_nip18::register(app, nmp_nip18::Config::default()).is_ok(),
        "nmp-nip18 registration must not collide"
    );
    #[cfg(feature = "bookmarks")]
    {
        assert!(
            nmp_nip84::register(app, nmp_nip84::Config::default()).is_ok(),
            "nmp-nip84 registration must not collide"
        );
    }
    #[cfg(feature = "groups")]
    {
        assert!(
            nmp_nip29::register(app, nmp_nip29::Config::default()).is_ok(),
            "nmp-nip29 registration must not collide"
        );
    }
    assert!(
        nmp_wot::register(app, nmp_wot::Config::default()).is_ok(),
        "nmp-wot registration must not collide"
    );
    assert!(
        nmp_nip51::register(app, nmp_nip51::Config::default()).is_ok(),
        "nmp-nip51 registration must not collide"
    );
    #[cfg(feature = "comments")]
    {
        assert!(
            nmp_nip22::register(app, nmp_nip22::Config::default()).is_ok(),
            "nmp-nip22 registration must not collide"
        );
    }
    assert!(
        nmp_nip17::register(app, nmp_nip17::Config::default()).is_ok(),
        "nmp-nip17 registration must not collide"
    );
    #[cfg(feature = "longform")]
    {
        assert!(
            nmp_nip23::register(app, nmp_nip23::Config::default()).is_ok(),
            "nmp-nip23 registration must not collide"
        );
    }
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
