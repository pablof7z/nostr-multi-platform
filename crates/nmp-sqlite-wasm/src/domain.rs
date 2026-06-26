//! NMP domain rows + schema migrations for the OPFS-SQLite engine (#1007 PR-5).
//!
//! Mirrors `nmp-store/src/lmdb/domain.rs`. One shared `domain_data` table keyed
//! `(namespace, user_key)` (the relational form of LMDB's single
//! `nmp-domain-data` sub-db), and a `domain_versions` table for per-namespace
//! schema versions. [`OpfsDomainHandle`] is the module-scoped handle
//! [`OpfsSqliteStore::domain_open`] returns; the data ops are inherent methods
//! keyed by namespace so the future `nmp-store` wrapper can build a
//! `DomainHandle` that delegates to them.
//!
//! The handle type is pure (just a namespace token), so it compiles on native;
//! the storage ops are wasm-gated.

/// Module-scoped domain handle (mirror of the role `nmp_store::DomainHandle`
/// plays). Carries only the namespace; all storage flows through the owning
/// [`crate::OpfsSqliteStore`] passed back in by the wrapper.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OpfsDomainHandle {
    /// The `'static` namespace this handle scopes reads/writes to.
    pub namespace: &'static str,
}

#[cfg(target_arch = "wasm32")]
mod wasm_impl {
    use super::OpfsDomainHandle;
    use crate::error::SqliteWasmError;
    use crate::store_impl::{exec_write, with_txn, SqlVal};
    use crate::types::{DomainMigration, MigrationTx};
    use crate::OpfsSqliteStore;

    impl OpfsSqliteStore {
        /// Open a module-scoped domain handle for `namespace`.
        pub fn domain_open(
            &self,
            namespace: &'static str,
        ) -> Result<OpfsDomainHandle, SqliteWasmError> {
            Ok(OpfsDomainHandle { namespace })
        }

        /// Upsert one namespace-scoped `(key, value)` row.
        pub fn domain_put(
            &self,
            namespace: &str,
            key: &[u8],
            value: &[u8],
        ) -> Result<(), SqliteWasmError> {
            let conn = self.db.borrow();
            exec_write(
                &conn,
                "INSERT INTO domain_data (namespace, user_key, value) VALUES (?1, ?2, ?3)
                 ON CONFLICT(namespace, user_key) DO UPDATE SET value = excluded.value",
                &[SqlVal::Text(namespace), SqlVal::Blob(key), SqlVal::Blob(value)],
            )
        }

        /// Read one namespace-scoped row, or `None`.
        pub fn domain_get(
            &self,
            namespace: &str,
            key: &[u8],
        ) -> Result<Option<Vec<u8>>, SqliteWasmError> {
            let conn = self.db.borrow();
            let stmt = conn
                .prepare("SELECT value FROM domain_data WHERE namespace = ?1 AND user_key = ?2")?;
            stmt.bind_text(1, namespace)?;
            stmt.bind_blob(2, key)?;
            if stmt.step()? {
                Ok(Some(stmt.column_blob(0)?))
            } else {
                Ok(None)
            }
        }

        /// Delete one namespace-scoped row; returns whether a row was present.
        pub fn domain_delete(
            &self,
            namespace: &str,
            key: &[u8],
        ) -> Result<bool, SqliteWasmError> {
            let conn = self.db.borrow();
            let existed = self.domain_get(namespace, key)?.is_some();
            if existed {
                exec_write(
                    &conn,
                    "DELETE FROM domain_data WHERE namespace = ?1 AND user_key = ?2",
                    &[SqlVal::Text(namespace), SqlVal::Blob(key)],
                )?;
            }
            Ok(existed)
        }

        /// Every `(user_key, value)` in `namespace` whose key starts with
        /// `user_prefix` (empty prefix = the whole namespace). The per-namespace
        /// set is small, so the prefix filter runs in Rust after a namespace scan.
        pub fn domain_scan_prefix(
            &self,
            namespace: &str,
            user_prefix: &[u8],
        ) -> Result<Vec<(Vec<u8>, Vec<u8>)>, SqliteWasmError> {
            let conn = self.db.borrow();
            let stmt = conn.prepare(
                "SELECT user_key, value FROM domain_data WHERE namespace = ?1 ORDER BY user_key ASC",
            )?;
            stmt.bind_text(1, namespace)?;
            let mut out = Vec::new();
            while stmt.step()? {
                let k = stmt.column_blob(0)?;
                if k.starts_with(user_prefix) {
                    out.push((k, stmt.column_blob(1)?));
                }
            }
            Ok(out)
        }

        /// Run schema migrations for `namespace` up to `target_version`.
        ///
        /// Errors [`SqliteWasmError::Migration`] if the on-disk version is newer
        /// than the target (the wrapper maps this to `StoreError::SchemaTooNew`)
        /// or a step's `apply` closure fails. A no-op when already at the target.
        pub fn run_migrations(
            &self,
            namespace: &str,
            target_version: u32,
            migrations: &[DomainMigration],
        ) -> Result<(), SqliteWasmError> {
            let conn = self.db.borrow();
            let current = read_version(&conn, namespace)?;

            if current > target_version {
                return Err(SqliteWasmError::Migration(format!(
                    "namespace '{namespace}' on-disk schema {current} is newer than target {target_version}"
                )));
            }
            if current == target_version {
                return Ok(());
            }

            with_txn(&conn, |c| {
                for m in migrations {
                    if m.from_version < current || m.from_version >= target_version {
                        continue;
                    }
                    let mut tx = MigrationTx::default();
                    (m.apply)(&mut tx).map_err(|reason| {
                        SqliteWasmError::Migration(format!(
                            "namespace '{namespace}' step {}→{}: {reason}",
                            m.from_version, m.to_version
                        ))
                    })?;
                    for (k, v) in tx.writes() {
                        exec_write(
                            c,
                            "INSERT INTO domain_data (namespace, user_key, value) VALUES (?1, ?2, ?3)
                             ON CONFLICT(namespace, user_key) DO UPDATE SET value = excluded.value",
                            &[SqlVal::Text(namespace), SqlVal::Blob(k), SqlVal::Blob(v)],
                        )?;
                    }
                }
                exec_write(
                    c,
                    "INSERT INTO domain_versions (namespace, version) VALUES (?1, ?2)
                     ON CONFLICT(namespace) DO UPDATE SET version = excluded.version",
                    &[SqlVal::Text(namespace), SqlVal::Int(i64::from(target_version))],
                )
            })
        }
    }

    /// Read the stored schema version for `namespace`, or `0` if never migrated.
    fn read_version(
        conn: &crate::shim::SqliteConn,
        namespace: &str,
    ) -> Result<u32, SqliteWasmError> {
        let stmt = conn.prepare("SELECT version FROM domain_versions WHERE namespace = ?1")?;
        stmt.bind_text(1, namespace)?;
        if stmt.step()? {
            Ok(stmt.column_int64(0)? as u32)
        } else {
            Ok(0)
        }
    }
}
