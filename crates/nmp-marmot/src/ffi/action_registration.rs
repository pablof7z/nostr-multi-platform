use std::sync::Arc;

use nmp_ffi::NmpApp;

use crate::projection::action::MarmotActionModule;
use crate::projection::state::MarmotProjection;

pub(super) fn register_marmot_action_module(app: &mut NmpApp, projection: Arc<MarmotProjection>) {
    match app.register_action(MarmotActionModule::new(projection)) {
        Ok(()) => {}
        Err(_replaced) => {
            // ActionRegistry::register installs the replacement before returning
            // Err. Marmot re-registration is same-app account lifecycle
            // replacement, not competing app modules claiming the namespace.
        }
    }
}
