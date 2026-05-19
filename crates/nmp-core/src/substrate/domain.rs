pub trait DomainModule: Send + Sync + 'static {
    const NAMESPACE: &'static str;
    const SCHEMA_VERSION: u32;

    /// Kinds this module wants to see at ingest. Empty (the default) means
    /// "no Nostr ingest" — useful for pure domain-store modules (e.g. the
    /// fixture-todo crate) that materialise records from app-local writes
    /// rather than relay traffic.
    ///
    /// Protocol-module crates override this to declare ownership of the
    /// kinds they decode (`nmp-nip23` returns `&[30023]`, etc.). The kernel
    /// dispatch table — landing in Phase 1 per
    /// `docs/design/kind-wrappers.md` §6 + §8 — reads this slice to build
    /// `kind → Vec<ModuleId>` routes; per D4 each `(kind, optional
    /// discriminator)` pair has exactly one owning module.
    ///
    /// The default body keeps every existing impl (the 13 `nmp-nip29`
    /// modules, the fixture-todo module, etc.) source-compatible — they
    /// inherit `&[]` and stay opted-out of ingest dispatch until they
    /// explicitly opt in.
    fn ingest_kinds() -> &'static [u32] {
        &[]
    }

    fn migrations() -> Vec<DomainMigration>;
    fn indexes() -> Vec<DomainIndex>;
    fn register(registry: &mut DomainRegistry);
}

pub struct DomainMigration {
    pub from_version: u32,
    pub to_version: u32,
    pub apply: fn(&mut MigrationTx) -> Result<(), String>,
}

pub struct DomainIndex {
    pub name: &'static str,
    pub key_fn: fn(&[u8]) -> Option<Vec<u8>>,
}

#[derive(Default)]
pub struct MigrationTx {
    writes: Vec<(Vec<u8>, Vec<u8>)>,
}

impl MigrationTx {
    pub fn put(&mut self, key: Vec<u8>, value: Vec<u8>) {
        self.writes.push((key, value));
    }

    pub fn writes(&self) -> &[(Vec<u8>, Vec<u8>)] {
        &self.writes
    }
}

#[derive(Default)]
pub struct DomainRegistry {
    records: Vec<&'static str>,
}

impl DomainRegistry {
    pub fn register_record<T: 'static>(&mut self) {
        self.records.push(std::any::type_name::<T>());
    }

    pub fn records(&self) -> &[&'static str] {
        &self.records
    }
}
