use super::{BrowserAppBuilder, ProvidersDecided};

pub(crate) fn install_browser_runtime_floor(app: &mut BrowserAppBuilder<ProvidersDecided>) {
    claim_browser_runtime_floor(app);
    let _substrate = nmp_substrate::install(app, nmp_substrate::SubstrateConfig::default());
}

fn claim_browser_runtime_floor(app: &BrowserAppBuilder<ProvidersDecided>) {
    let Ok(mut inner) = app.inner.lock() else {
        return;
    };
    assert!(
        !inner.runtime_floor_installed,
        "BrowserAppBuilder runtime floor must be installed exactly once"
    );
    inner.runtime_floor_installed = true;
}
