# LMDB tests: migration + domain isolation (§2.12–2.13)

> Sub-file of [`../tests.md`](../tests.md). Covers `DomainModule` migration invariants.

### 2.12 Domain migration success + failure (master doc §6)

File: `crates/nmp-testing/tests/store_domain_migration.rs`

- Register `TestModuleV1` with `SCHEMA_VERSION = 1` and no migrations; open store; assert `_meta.test_module.schema_version == 1`.
- Close store; register `TestModuleV2` with `SCHEMA_VERSION = 2` and one migration v1→v2 that writes one key; open store; assert migration ran and key exists.
- Close; register `TestModuleV3` with `SCHEMA_VERSION = 3` and a deliberately failing migration v2→v3; open store; assert `Effect::DomainSchemaTooNew { namespace: "test_module" }` (under degraded-mode rules) and `_meta` still at v2.
- Close; remove the failing migration; reopen — assert successful catch-up to v3 (idempotent retry).

### 2.12a Migration atomicity / crash-recovery (watermarks.md §4.2)

File: `crates/nmp-testing/tests/store_domain_migration.rs` (extended section)

Tests the single-`RwTxn` atomicity invariant for migration steps: verifies that a simulated crash (i.e., aborting the write transaction mid-migration) leaves the `_meta` version unchanged and the store in a retryable state.

```rust
#[cfg(feature = "lmdb-backend")]
#[test]
fn migration_crash_leaves_version_unchanged() {
    // Use a FailingMigration that panics after writing data but before commit.
    // We catch the unwind and re-open the store.
    let dir = tempfile::tempdir().unwrap();
    let result = std::panic::catch_unwind(|| {
        let store = LmdbEventStore::open(dir.path()).unwrap();
        let migrations = vec![DomainMigration {
            from_version: 0,
            to_version: 1,
            apply: Box::new(|tx| {
                tx.put(b"key", b"value")?;
                panic!("simulated crash after data write, before commit");
            }),
        }];
        let _ = store.run_migrations("test_ns", 1, &migrations);
    });
    assert!(result.is_err(), "expected panic");

    // Re-open: version must still be 0 (data write was not committed).
    let store2 = LmdbEventStore::open(dir.path()).unwrap();
    let version = store2.read_meta_schema_version_raw("test_ns").unwrap().unwrap_or(0);
    assert_eq!(version, 0, "version must not be bumped after a crashed migration");
    // The data write must also be absent (rolled back with the transaction).
    let handle = store2.domain_open("test_ns").unwrap();
    assert!(handle.get(b"key").unwrap().is_none());
}
```

### 2.13 Domain isolation (`kernel-substrate.md` §8)

File: `crates/nmp-testing/tests/store_domain_isolation.rs`

- Open `DomainHandle` for module A; write key `K`.
- Open `DomainHandle` for module B; read key `K` — assert returns `None`.
- Module B's `scan_prefix(b"")` returns only module B's rows.
