mod action;
mod capability;
mod domain;
mod identity;
mod view;

pub use action::{
    ActionContext, ActionId, ActionInput, ActionModule, ActionPlan, ActionRejection, ActionStatus,
    ActionTransition,
};
pub use capability::{CapabilityEnvelope, CapabilityModule, CapabilityRequest};
pub use domain::{DomainIndex, DomainMigration, DomainModule, DomainRegistry, MigrationTx};
pub use identity::{
    BoxFuture, IdentityContext, IdentityError, IdentityId, IdentityModule, IdentityScopeKind,
    SignedEvent, SigningError, UnsignedEvent,
};
pub use view::{EventId, KernelEvent, ProjectionChange, ViewContext, ViewDependencies, ViewModule};

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ModuleDescriptor {
    pub namespace: &'static str,
    pub family: ModuleFamily,
    pub rust_type: &'static str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModuleFamily {
    Domain,
    View,
    Action,
    Capability,
    Identity,
}

#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ModuleRegistry {
    descriptors: Vec<ModuleDescriptor>,
}

impl ModuleRegistry {
    pub fn register_domain<M: DomainModule>(&mut self) {
        self.push::<M>(M::NAMESPACE, ModuleFamily::Domain);
    }

    pub fn register_view<M: ViewModule>(&mut self) {
        self.push::<M>(M::NAMESPACE, ModuleFamily::View);
    }

    pub fn register_action<M: ActionModule>(&mut self) {
        self.push::<M>(M::NAMESPACE, ModuleFamily::Action);
    }

    pub fn register_capability<M: CapabilityModule>(&mut self) {
        self.push::<M>(M::NAMESPACE, ModuleFamily::Capability);
    }

    pub fn register_identity<M: IdentityModule>(&mut self) {
        self.push::<M>(M::NAMESPACE, ModuleFamily::Identity);
    }

    pub fn descriptors(&self) -> &[ModuleDescriptor] {
        &self.descriptors
    }

    fn push<M: 'static>(&mut self, namespace: &'static str, family: ModuleFamily) {
        if self
            .descriptors
            .iter()
            .any(|existing| existing.namespace == namespace && existing.family == family)
        {
            return;
        }
        self.descriptors.push(ModuleDescriptor {
            namespace,
            family,
            rust_type: std::any::type_name::<M>(),
        });
    }
}
