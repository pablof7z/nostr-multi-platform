pub trait DomainModule: Send + Sync + 'static {
    const NAMESPACE: &'static str;
    const SCHEMA_VERSION: u32;

    fn migrations() -> Vec<DomainMigration>;
    fn indexes() -> Vec<DomainIndex>;
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

/// In-memory staging buffer for the writes a [`DomainMigration`] wants to
/// apply. It is **not** a transaction against the store and a `MigrationTx`
/// never touches LMDB directly.
///
/// Key-space isolation — why a buggy module cannot corrupt another's records:
/// the `key` passed to [`MigrationTx::put`] is a *module-local* key. The
/// backend's `run_migrations` drains [`MigrationTx::writes`] and, before each
/// row reaches the shared `nmp-domain-data` sub-db, prefixes it with
/// `namespace || 0x00`. That `namespace` is the `&'static str` the kernel
/// supplies into `run_migrations` from [`DomainModule::NAMESPACE`] — a
/// compile-time const tied to the module type, **not** anything the migration
/// closure can choose. The same prefix is applied to runtime
/// `DomainHandle::{put,get,delete,scan_prefix}` (see
/// `store/lmdb/domain.rs::full_key`). A module therefore physically cannot
/// address — or overwrite — another module's key-space.
///
/// One shared sub-db (rather than one named LMDB database per module) is a
/// deliberate choice to avoid exhausting LMDB's `max_dbs`; see the module
/// docs in `crates/nmp-core/src/store/lmdb/domain.rs`.
#[derive(Default)]
pub struct MigrationTx {
    writes: Vec<(Vec<u8>, Vec<u8>)>,
}

impl MigrationTx {
    /// Stage a `key`/`value` write. `key` is the **module-local** key: the
    /// backend prefixes it with the owning module's namespace at flush time
    /// (see the type-level docs on [`MigrationTx`]), so callers must *not*
    /// add a namespace prefix themselves.
    pub fn put(&mut self, key: Vec<u8>, value: Vec<u8>) {
        self.writes.push((key, value));
    }

    /// Staged writes, in insertion order. The backend's `run_migrations`
    /// drains these and applies the `namespace || 0x00` prefix before each
    /// row reaches storage.
    pub fn writes(&self) -> &[(Vec<u8>, Vec<u8>)] {
        &self.writes
    }
}
