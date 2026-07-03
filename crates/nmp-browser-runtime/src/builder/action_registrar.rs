use super::BrowserAppBuilder;
use nmp_core::substrate::ActionRegistrar;

impl<S> ActionRegistrar for BrowserAppBuilder<S> {
    fn register_action<M: nmp_core::substrate::ActionModule + 'static>(
        &mut self,
        module: M,
    ) -> Result<(), nmp_core::substrate::RegistrationError> {
        let Ok(mut g) = self.inner.lock() else {
            return Ok(());
        };
        g.action_registry.register_action(module)
    }

    fn register_default_action<M: nmp_core::substrate::ActionModule + 'static>(
        &mut self,
        module: M,
    ) -> bool {
        let Ok(mut g) = self.inner.lock() else {
            return false;
        };
        g.action_registry.register_default_action(module)
    }
}
