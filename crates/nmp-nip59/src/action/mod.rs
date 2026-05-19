//! ActionModule impls for nmp-nip59.
//!
//! Currently a single module: `WelcomeWrapModule` for wrapping MLS Welcome
//! rumos in NIP-59 gift-wrap envelopes addressed to a specific recipient.

mod welcome_wrap;

pub use welcome_wrap::{WelcomeWrapInput, WelcomeWrapModule, WelcomeWrapStep, WrapPlan};

use nmp_core::substrate::ModuleRegistry;

pub fn register_all(registry: &mut ModuleRegistry) {
    registry.register_action::<WelcomeWrapModule>();
}
