//! Runtime start/stop, maintenance scheduling, and snapshot emission helpers.

#[cfg(target_arch = "wasm32")]
use std::rc::Rc;

use nmp_core::OutboundMessage;

use crate::protocol::{RuntimeStatus, StartConfig, WorkerEvent};
use crate::snapshot::build_snapshot_bytes;

use super::{WasmRuntime, WasmRuntimeError};

impl WasmRuntime {
    pub(super) fn start(
        &mut self,
        config: StartConfig,
    ) -> Result<Vec<WorkerEvent>, WasmRuntimeError> {
        if config.app_id.trim().is_empty() {
            return Err(WasmRuntimeError::InvalidConfig(
                "app_id is required".to_string(),
            ));
        }
        if config.database_name.trim().is_empty() {
            return Err(WasmRuntimeError::InvalidConfig(
                "database_name is required".to_string(),
            ));
        }
        if config.relays.is_empty() {
            return Err(WasmRuntimeError::InvalidConfig(
                "at least one relay is required".to_string(),
            ));
        }

        if let Some(store) = self.injected_store.borrow_mut().take() {
            self.reducer.borrow_mut().replace_store_for_start(store);
        }
        let before_start_hooks = std::mem::take(&mut self.before_start_hooks);
        for hook in before_start_hooks {
            hook(self);
        }

        let relay_bootstrap =
            crate::protocol::relay_bootstrap_from_config(config.relays, config.relay_bootstrap);

        self.reducer.borrow_mut().set_configured_relays(
            relay_bootstrap
                .iter()
                .map(|e| (e.url.clone(), e.role.clone()))
                .collect(),
        );

        if let Some(factory) = self.publish_resolver_factory.borrow().clone() {
            let resolver = {
                let reducer = self.reducer.borrow();
                factory(
                    reducer.event_store_handle(),
                    reducer.indexer_relays_handle(),
                    reducer.local_write_relays_handle(),
                    reducer.active_account_handle(),
                )
            };
            self.reducer.borrow_mut().set_publish_resolver(resolver);
        }

        {
            let mut meta = self.meta.borrow_mut();
            meta.started = true;
            meta.relay_bootstrap = relay_bootstrap;
            meta.database_name = config.database_name;
        }

        #[cfg(target_arch = "wasm32")]
        {
            self.spawn_relay_drivers()?;
        }
        self.request_event_drain();

        Ok(vec![
            WorkerEvent::RuntimeStatus {
                status: RuntimeStatus::Running,
                correlation_id: Some(config.correlation_id),
            },
            self.snapshot_event(),
        ])
    }

    pub(super) fn stop(
        &mut self,
        correlation_id: String,
    ) -> Result<Vec<WorkerEvent>, WasmRuntimeError> {
        crate::tick::cancel_deadline(&self.maintenance_deadline);
        #[cfg(target_arch = "wasm32")]
        crate::relay_pool::close_drivers(&self.relays);
        #[cfg(target_arch = "wasm32")]
        {
            *self.handlers_slot.borrow_mut() = None;
        }

        self.meta.borrow_mut().started = false;
        Ok(vec![WorkerEvent::RuntimeStatus {
            status: RuntimeStatus::Stopped,
            correlation_id: Some(correlation_id),
        }])
    }

    #[cfg(target_arch = "wasm32")]
    fn spawn_relay_drivers(&mut self) -> Result<(), WasmRuntimeError> {
        let handlers = crate::relay_pool::build_handlers(
            Rc::clone(&self.relays),
            Rc::clone(&self.snapshot_callback),
            Rc::clone(&self.reducer),
            Rc::clone(&self.meta),
            Rc::clone(&self.handlers_slot),
            Rc::clone(&self.maintenance_deadline),
            Rc::clone(&self.post_tick_drain),
        );
        *self.handlers_slot.borrow_mut() = Some(handlers.clone());
        let drivers =
            crate::relay_pool::spawn_drivers(&self.meta.borrow().relay_bootstrap, handlers)?;
        *self.relays.borrow_mut() = drivers;
        Ok(())
    }

    fn request_maintenance_deadline(&self, policy: crate::tick::WakePolicy) {
        #[cfg(target_arch = "wasm32")]
        crate::tick::request_runtime_deadline(
            Rc::clone(&self.maintenance_deadline),
            policy,
            Rc::clone(&self.reducer),
            Rc::clone(&self.relays),
            Rc::clone(&self.handlers_slot),
            Rc::clone(&self.snapshot_callback),
            Rc::clone(&self.meta),
            Rc::clone(&self.post_tick_drain),
        );
        #[cfg(not(target_arch = "wasm32"))]
        crate::tick::request_deadline_for_test(&self.maintenance_deadline, policy);
    }

    pub(super) fn request_event_drain(&self) {
        self.request_maintenance_deadline(crate::tick::WakePolicy::Event);
    }

    fn request_event_or_kernel_deadline(&self) {
        self.request_maintenance_deadline(crate::tick::event_or_kernel_policy(&self.reducer));
    }

    pub(super) fn fan_outbound(&self, outbound: Vec<OutboundMessage>) {
        let has_outbound = !outbound.is_empty();
        #[cfg(target_arch = "wasm32")]
        crate::relay_pool::fan_out_outbound(&self.relays, &self.handlers_slot, &outbound);
        #[cfg(not(target_arch = "wasm32"))]
        let _ = outbound;
        if has_outbound {
            self.request_event_or_kernel_deadline();
        }
    }

    pub(super) fn snapshot_event(&mut self) -> WorkerEvent {
        let bytes =
            build_snapshot_bytes(&mut self.reducer.borrow_mut(), &mut self.meta.borrow_mut());
        WorkerEvent::UpdateBytes { bytes }
    }
}
