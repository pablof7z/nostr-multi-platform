use std::sync::Arc;

use nmp_core::substrate::{
    HostCapabilities, ObservedProjectionRegistrar, SnapshotProjectionRegistrar,
};

use crate::WotBootstrapRuntime;

#[derive(Clone, Debug, Default)]
pub struct Config;

#[derive(Clone, Default)]
pub struct Handles {
    pub runtime: Option<Arc<WotBootstrapRuntime>>,
}

impl std::fmt::Debug for Handles {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Handles")
            .field("runtime", &self.runtime.is_some())
            .finish()
    }
}

pub fn register(
    app: &(impl HostCapabilities + ObservedProjectionRegistrar + SnapshotProjectionRegistrar),
    _config: Config,
) -> Result<Handles, nmp_core::substrate::RegistrationError> {
    Ok(Handles {
        runtime: crate::runtime::register_runtime(app),
    })
}
