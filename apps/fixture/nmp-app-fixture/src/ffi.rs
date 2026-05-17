use crate::{AppAction, AppUpdate};

#[derive(Default)]
pub struct FfiApp {
    rev: u64,
}

impl FfiApp {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn app_name(&self) -> &'static str {
        "fixture"
    }

    pub fn dispatch(&mut self, action: AppAction) -> AppUpdate {
        self.rev = self.rev.saturating_add(1);
        AppUpdate::Kernel(nmp_core::KernelUpdate::Diagnostics {
            summary: format!("dispatched {} at rev {}", action.namespace(), self.rev),
        })
    }
}
