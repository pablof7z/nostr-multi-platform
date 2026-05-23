//! Value types passed to `EventStore::run_migrations`: a list of
//! [`DomainMigration`] steps and the [`MigrationTx`] staging buffer each
//! migration writes through.
//!
//! These are consumed directly by the store backends (`store/lmdb/domain.rs`
//! and `store/mem/domain.rs`) and by `nmp-testing/tests/store_domain_migration.rs`.
//! There is no enclosing trait — every caller constructs a `Vec<DomainMigration>`
//! inline and passes it to `EventStore::run_migrations(namespace, target_version,
//! &migrations)`.

pub struct DomainMigration {
    pub from_version: u32,
    pub to_version: u32,
    pub apply: fn(&mut MigrationTx) -> Result<(), String>,
}

/// In-memory staging buffer for the writes a [`DomainMigration`] wants to
/// apply. It is **not** a transaction against the store and a `MigrationTx`
/// never touches LMDB directly.
///
/// Key-space isolation — why a buggy migration cannot corrupt another
/// namespace's records: the `key` passed to [`MigrationTx::put`] is a
/// *namespace-local* key. The backend's `run_migrations` drains
/// [`MigrationTx::writes`] and, before each row reaches the shared
/// `nmp-domain-data` sub-db, prefixes it with `namespace || 0x00`. That
/// `namespace` is the `&'static str` the caller supplies into
/// `run_migrations`, NOT anything the migration closure can choose. The same
/// prefix is applied to runtime `DomainHandle::{put,get,delete,scan_prefix}`
/// (see `store/lmdb/domain.rs::full_key`). A migration therefore physically
/// cannot address — or overwrite — another namespace's key-space.
///
/// One shared sub-db (rather than one named LMDB database per namespace) is a
/// deliberate choice to avoid exhausting LMDB's `max_dbs`; see the module
/// docs in `crates/nmp-core/src/store/lmdb/domain.rs`.
#[derive(Default)]
pub struct MigrationTx {
    writes: Vec<(Vec<u8>, Vec<u8>)>,
}

impl MigrationTx {
    /// Stage a `key`/`value` write. `key` is the **namespace-local** key: the
    /// backend prefixes it with the owning namespace at flush time (see the
    /// type-level docs on [`MigrationTx`]), so callers must *not* add a
    /// namespace prefix themselves.
    pub fn put(&mut self, key: Vec<u8>, value: Vec<u8>) {
        self.writes.push((key, value));
    }

    /// Staged writes, in insertion order. The backend's `run_migrations`
    /// drains these and applies the `namespace || 0x00` prefix before each
    /// row reaches storage.
    #[must_use] 
    pub fn writes(&self) -> &[(Vec<u8>, Vec<u8>)] {
        &self.writes
    }
}
