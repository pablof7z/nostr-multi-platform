//! §2.12 Domain migration success + failure tests.
//!
//! See `docs/design/lmdb/tests/migration.md` §2.12.

use nmp_core::store::StoreError;
use nmp_core::substrate::{DomainMigration, MigrationTx};
use nmp_testing::for_each_backend;
use nmp_testing::store_harness::StoreHarness;

for_each_backend!(domain_open_returns_handle, |h: &mut StoreHarness| {
    let handle = h
        .store
        .domain_open("test-ns")
        .expect("domain_open should succeed");
    handle.put(b"hello", b"world").expect("put should succeed");
    let val = handle.get(b"hello").expect("get should succeed");
    assert_eq!(val, Some(b"world".to_vec()));
});

for_each_backend!(domain_delete_removes_key, |h: &mut StoreHarness| {
    let handle = h.store.domain_open("test-ns-del").unwrap();
    handle.put(b"k", b"v").unwrap();
    let removed = handle.delete(b"k").unwrap();
    assert!(removed, "delete should return true for existing key");
    let val = handle.get(b"k").unwrap();
    assert!(val.is_none(), "key should be absent after delete");
    let removed2 = handle.delete(b"k").unwrap();
    assert!(!removed2, "delete of missing key should return false");
});

for_each_backend!(
    domain_scan_prefix_returns_matching,
    |h: &mut StoreHarness| {
        let handle = h.store.domain_open("test-ns-scan").unwrap();
        handle.put(b"prefix:a", b"1").unwrap();
        handle.put(b"prefix:b", b"2").unwrap();
        handle.put(b"other:c", b"3").unwrap();

        let results: Vec<_> = handle
            .scan_prefix(b"prefix:")
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(
            results.len(),
            2,
            "scan_prefix should return only matching keys"
        );
    }
);

for_each_backend!(domain_namespaces_are_isolated, |h: &mut StoreHarness| {
    let ns_a = h.store.domain_open("ns-a").unwrap();
    let ns_b = h.store.domain_open("ns-b").unwrap();

    ns_a.put(b"key", b"from-a").unwrap();
    ns_b.put(b"key", b"from-b").unwrap();

    assert_eq!(ns_a.get(b"key").unwrap(), Some(b"from-a".to_vec()));
    assert_eq!(ns_b.get(b"key").unwrap(), Some(b"from-b".to_vec()));
});

for_each_backend!(run_migrations_v0_to_v1, |h: &mut StoreHarness| {
    let migrations = vec![DomainMigration {
        from_version: 0,
        to_version: 1,
        apply: |tx: &mut MigrationTx| {
            tx.put(b"schema".to_vec(), b"v1".to_vec());
            Ok(())
        },
    }];

    h.store.run_migrations("migtest", 1, &migrations).unwrap();

    // Migration should have written the schema key.
    let handle = h.store.domain_open("migtest").unwrap();
    let val = handle.get(b"schema").unwrap();
    assert_eq!(val, Some(b"v1".to_vec()), "migration write should persist");
});

for_each_backend!(run_migrations_idempotent, |h: &mut StoreHarness| {
    let migrations = vec![DomainMigration {
        from_version: 0,
        to_version: 1,
        apply: |tx: &mut MigrationTx| {
            tx.put(b"idem".to_vec(), b"yes".to_vec());
            Ok(())
        },
    }];

    // Run twice — should not error on second run.
    h.store.run_migrations("idem-ns", 1, &migrations).unwrap();
    h.store.run_migrations("idem-ns", 1, &migrations).unwrap();
});

for_each_backend!(
    run_migrations_schema_too_new_rejected,
    |h: &mut StoreHarness| {
        // Get to version 2 first.
        let m_to_v2 = vec![
            DomainMigration {
                from_version: 0,
                to_version: 1,
                apply: |_tx: &mut MigrationTx| Ok(()),
            },
            DomainMigration {
                from_version: 1,
                to_version: 2,
                apply: |_tx: &mut MigrationTx| Ok(()),
            },
        ];
        h.store.run_migrations("new-ns", 2, &m_to_v2).unwrap();

        // Now try to migrate to version 1 (older than current 2).
        let m_to_v1 = vec![DomainMigration {
            from_version: 0,
            to_version: 1,
            apply: |_tx: &mut MigrationTx| Ok(()),
        }];
        let err = h.store.run_migrations("new-ns", 1, &m_to_v1);
        assert!(
            matches!(err, Err(StoreError::SchemaTooNew { .. })),
            "running older migration target should fail with SchemaTooNew, got {err:?}"
        );
    }
);
