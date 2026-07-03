//! `nmp.wallet.select_backend` — W5 (#2908, epic #2864).
//!
//! Sets the [`WalletBackendSelector`]'s preferred backend for the (today
//! unreachable — see `selector::tests::nwc_and_cashu_capabilities_never_overlap_today`)
//! case where more than one registered backend could satisfy the same
//! capability.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use nmp_core::actor::ActorCommand;
use nmp_core::substrate::{ActionContext, ActionModule, ActionRejection, DeclaredActionNamespace};

use crate::backend::WalletBackendId;
use crate::selector::{SelectorError, WalletBackendSelector};
use crate::ui_codes;
use crate::ACTION_SELECT_BACKEND;

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct SelectBackendAction {
    pub backend_id: String,
}

pub struct SelectBackendModule {
    selector: Arc<WalletBackendSelector>,
}

impl SelectBackendModule {
    #[must_use]
    pub fn new(selector: Arc<WalletBackendSelector>) -> Self {
        Self { selector }
    }
}

impl ActionModule for SelectBackendModule {
    const NAMESPACE: DeclaredActionNamespace = DeclaredActionNamespace::framework(
        ACTION_SELECT_BACKEND,
        "action.nmp.wallet.select_backend",
    );

    type Action = SelectBackendAction;

    /// Validate `backend_id` up front so a bad selection never reaches
    /// `execute()` — an unregistered id is a structured rejection, not a
    /// silently-ignored no-op.
    fn start(&self, _ctx: &mut ActionContext, action: Self::Action) -> Result<(), ActionRejection> {
        if action.backend_id.trim().is_empty() {
            return Err(ActionRejection::Invalid(
                "select_backend requires a non-empty backend_id".to_string(),
            ));
        }
        if !self
            .selector
            .has_backend(&WalletBackendId::new(action.backend_id.clone()))
        {
            return Err(ActionRejection::InvalidCoded {
                code: ui_codes::UNKNOWN_BACKEND,
                message: format!("no registered wallet backend named {}", action.backend_id),
            });
        }
        Ok(())
    }

    fn execute(
        &self,
        _ctx: &ActionContext,
        action: Self::Action,
        _correlation_id: &str,
        _send: &dyn Fn(ActorCommand),
    ) -> Result<(), String> {
        // `start()` already validated the id is registered; a race with a
        // backend being unregistered between `start` and `execute` cannot
        // happen today (backends are registered once, at composition time,
        // and never removed) but `set_preferred` still fails closed rather
        // than assume.
        match self
            .selector
            .set_preferred(WalletBackendId::new(action.backend_id))
        {
            Ok(()) => Ok(()),
            Err(SelectorError::UnknownBackend(id)) => {
                Err(format!("unknown wallet backend: {}", id.as_str()))
            }
            Err(other) => Err(format!("{other:?}")),
        }
    }
}

#[cfg(test)]
#[path = "select_backend_tests.rs"]
mod tests;
