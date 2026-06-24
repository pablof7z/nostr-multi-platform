use nmp_core::substrate::{ActionModule, ActionRegistrar};

use super::WasmRuntime;

/// Lets the per-NIP `register_actions(&mut impl ActionRegistrar)` entry points
/// register straight into the runtime's typed action registry.
impl ActionRegistrar for WasmRuntime {
    fn register_action<M: ActionModule + 'static>(
        &mut self,
        module: M,
    ) -> Result<(), nmp_core::substrate::RegistrationError> {
        WasmRuntime::register_action(self, module)
    }

    fn register_default_action<M: ActionModule + 'static>(&mut self, module: M) -> bool {
        WasmRuntime::register_default_action(self, module)
    }
}
