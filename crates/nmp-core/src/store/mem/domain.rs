//! Domain rows and migration support for `MemEventStore`.
//!
//! D0: domain isolation — each module gets its own namespace handle.
//! One `DomainHandle` cannot read another module's namespace.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use super::MemEventStore;
use crate::store::events::{DomainHandle, DomainHandleInner};
use crate::store::StoreError;
use crate::substrate::DomainMigration;

pub(super) fn domain_open(
    store: &MemEventStore,
    namespace: &'static str,
) -> Result<DomainHandle, StoreError> {
    let mut st = store.lock()?;
    let data = st
        .domain_data
        .entry(namespace)
        .or_insert_with(|| Arc::new(Mutex::new(HashMap::new())))
        .clone();
    Ok(DomainHandle {
        inner: DomainHandleInner::Mem { namespace, data },
    })
}

pub(super) fn run_migrations(
    store: &MemEventStore,
    namespace: &'static str,
    target_version: u32,
    migrations: &[DomainMigration],
) -> Result<(), StoreError> {
    let mut st = store.lock()?;
    let current = *st.domain_versions.get(namespace).unwrap_or(&0);

    if current > target_version {
        return Err(StoreError::SchemaTooNew {
            namespace: namespace.to_string(),
            on_disk: current,
            expected: target_version,
        });
    }

    if current == target_version {
        return Ok(());
    }

    // Get or create domain data arc.
    let data_arc = st
        .domain_data
        .entry(namespace)
        .or_insert_with(|| Arc::new(Mutex::new(HashMap::new())))
        .clone();

    // Apply migrations in order.
    for m in migrations {
        if m.from_version < current || m.from_version >= target_version {
            continue;
        }
        let mut tx = crate::substrate::MigrationTx::default();
        (m.apply)(&mut tx).map_err(|reason| StoreError::MigrationFailed { // doctrine-allow: D15 — `Migration::apply` is an internal substrate-registered migration closure run at store init, not a host FFI seam; loud panic here is the intended bug-surfacing behaviour (mirrors the actor command drain allowlist rationale)
            namespace: namespace.to_string(),
            from: m.from_version,
            to: m.to_version,
            reason,
        })?;
        let mut data = data_arc
            .lock()
            .map_err(|e| StoreError::Io(e.to_string()))?;
        for (k, v) in tx.writes() {
            data.insert(k.clone(), v.clone());
        }
    }

    st.domain_versions.insert(namespace, target_version);
    Ok(())
}
