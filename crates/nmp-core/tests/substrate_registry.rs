use nmp_core::substrate::*;
use serde::{Deserialize, Serialize};

struct DummyDomain;

impl DomainModule for DummyDomain {
    const NAMESPACE: &'static str = "dummy.domain";
    const SCHEMA_VERSION: u32 = 1;

    fn migrations() -> Vec<DomainMigration> {
        Vec::new()
    }

    fn indexes() -> Vec<DomainIndex> {
        Vec::new()
    }

    fn register(registry: &mut DomainRegistry) {
        registry.register_record::<DummyRecord>();
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct DummyRecord {
    id: String,
}

#[test]
fn domain_registry_records_rust_types() {
    let mut registry = DomainRegistry::default();
    DummyDomain::register(&mut registry);

    assert_eq!(registry.records().len(), 1);
    assert!(registry.records()[0].contains("DummyRecord"));
}

#[test]
fn module_registry_deduplicates_by_family_and_namespace() {
    let mut registry = ModuleRegistry::default();
    registry.register_domain::<DummyDomain>();
    registry.register_domain::<DummyDomain>();

    assert_eq!(registry.descriptors().len(), 1);
    assert_eq!(registry.descriptors()[0].namespace, "dummy.domain");
    assert_eq!(registry.descriptors()[0].family, ModuleFamily::Domain);
}
