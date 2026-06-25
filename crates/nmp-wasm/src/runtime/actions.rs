use nmp_core::substrate::{ActionModule, ActionRegistrar};

use super::RawWasmAbiAdapter;

/// Lets the per-NIP `register_actions(&mut impl ActionRegistrar)` entry points
/// register straight into the runtime's typed action registry. Internal API.
impl ActionRegistrar for RawWasmAbiAdapter {
    fn register_action<M: ActionModule + 'static>(
        &mut self,
        module: M,
    ) -> Result<(), nmp_core::substrate::RegistrationError> {
        RawWasmAbiAdapter::register_action(self, module)
    }

    fn register_default_action<M: ActionModule + 'static>(&mut self, module: M) -> bool {
        RawWasmAbiAdapter::register_default_action(self, module)
    }
}
