use std::cell::RefCell;
use std::rc::Rc;

use nmp_core::{default_registry, KernelReducer};

use super::{RuntimeMeta, WasmRuntime};

impl Default for WasmRuntime {
    fn default() -> Self {
        Self {
            reducer: Rc::new(RefCell::new(KernelReducer::new())),
            meta: Rc::new(RefCell::new(RuntimeMeta::new())),
            snapshot_callback: Rc::new(RefCell::new(None)),
            post_tick_drain: Rc::new(RefCell::new(None)),
            #[cfg(target_arch = "wasm32")]
            relays: Rc::new(RefCell::new(Vec::new())),
            #[cfg(target_arch = "wasm32")]
            handlers_slot: Rc::new(RefCell::new(None)),
            maintenance_deadline: Rc::new(RefCell::new(crate::tick::RuntimeDeadline::default())),
            action_registry: default_registry(),
        }
    }
}
