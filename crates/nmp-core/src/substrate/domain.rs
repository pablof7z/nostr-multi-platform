pub trait DomainModule: Send + Sync + 'static {
    const NAMESPACE: &'static str;
    const SCHEMA_VERSION: u32;

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
