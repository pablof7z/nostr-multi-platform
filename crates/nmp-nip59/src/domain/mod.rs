//! DomainModule impls for nmp-nip59.

mod welcome_unwrap;

pub use welcome_unwrap::{WelcomeRecord, WelcomeUnwrapModule};

use nmp_core::substrate::ModuleRegistry;

pub fn register_all(registry: &mut ModuleRegistry) {
    registry.register_domain::<WelcomeUnwrapModule>();
}
