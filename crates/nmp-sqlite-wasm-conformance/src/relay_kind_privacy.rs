use nmp_sqlite_wasm::{EngineEvent, InsertOutcome, OpfsSqliteStore};

use super::Report;

/// Insert public and private kinds from one relay and prove the SQLite
/// relay-kind projection hides private metadata presence.
pub(super) fn record(report: &mut Report, store: &OpfsSqliteStore) {
    let relay = "privacy-relay";
    let pk = "44".repeat(32);
    let mk = |id_n: u64, kind: u32| EngineEvent {
        id: format!("{id_n:064x}"),
        pubkey: pk.clone(),
        created_at: 10_000 + id_n,
        kind,
        tags: vec![],
        content: String::new(),
        sig: "00".repeat(64),
    };

    report.record(
        "relay_kind_privacy_gate",
        (|| {
            for (id_n, kind) in [
                (0x9001, 1u32),
                (0x9004, 4),
                (0x900d, 13),
                (0x900e, 14),
                (0x900f, 15),
                (0x9423, 1059),
                (0x9424, 1060),
            ] {
                match store.insert(mk(id_n, kind), relay, 1_700_000_001_000) {
                    Ok(InsertOutcome::Inserted { .. }) => {}
                    Ok(other) => {
                        return Err(format!("kind {kind} was not inserted: {other:?}"));
                    }
                    Err(e) => return Err(format!("kind {kind} insert failed: {e}")),
                }
            }

            let coverage = store
                .relay_kind_coverage(relay)
                .map_err(|e| format!("coverage failed: {e}"))?;
            if coverage != vec![1] {
                return Err(format!("coverage leaked private kinds: {coverage:?}"));
            }

            for kind in [4u32, 13, 14, 15, 1059, 1060] {
                let count = store
                    .relay_kind_count(relay, kind)
                    .map_err(|e| format!("count kind {kind} failed: {e}"))?;
                if count != 0 {
                    return Err(format!("private kind {kind} count leaked as {count}"));
                }
            }

            let public_count = store
                .relay_kind_count(relay, 1)
                .map_err(|e| format!("public count failed: {e}"))?;
            if public_count != 1 {
                return Err(format!("public kind count got {public_count}, want 1"));
            }

            Ok("coverage=[1], private counts=0, public count=1".to_owned())
        })(),
    );
}
