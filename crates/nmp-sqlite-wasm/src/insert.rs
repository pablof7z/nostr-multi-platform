//! Transactional insert path for the OPFS-SQLite engine (#1007 PR-3).
//!
//! Mirrors the §7.1 / ADR-0012 insert order of the LMDB backend
//! (`nmp-store/src/lmdb/insert.rs`), scoped to the index families PR-3 owns
//! (primary row + every secondary tag row + provenance + ingest-seq + kind:5
//! tombstones). The whole thing runs inside one SQLite transaction
//! ([`crate::store_impl::with_txn`]) — OPFS supplies no write atomicity on its
//! own, so the `BEGIN`/`COMMIT` is what makes the `EventStore::insert` "all
//! secondaries land together or none do" contract honest.
//!
//! Out of PR-3 scope (later PRs): LRU stamping, FTS,
//! freshness, coverage ledger, and ingest-log retention trimming.

// The `insert` method is added to `OpfsSqliteStore` by the inherent `impl` inside
// `wasm_impl`; there is no free item to re-export.
#[cfg(target_arch = "wasm32")]
mod wasm_impl {
    use crate::conv::EngineEvent;
    use crate::delete;
    use crate::error::SqliteWasmError;
    use crate::outcome::{EventId, InsertOutcome, PubKey, RejectReason, TombstoneOrigin};
    use crate::shim::SqliteConn;
    use crate::store_impl::{exec_write, with_txn, SqlVal};
    use crate::{conv, ingest_log, provenance, tombstones, OpfsSqliteStore};

    impl OpfsSqliteStore {
        /// The single insert path. `source` is the relay that delivered this copy.
        ///
        /// The caller MUST have verified the event's signature before calling
        /// (this engine assumes a `VerifiedEvent`-equivalent gate upstream — the
        /// structural check here only rejects malformed wire shape). Applies the
        /// tombstone / ephemeral / expiry / replaceable invariants and writes the
        /// primary row + secondaries + provenance + ingest-log entry atomically.
        pub fn insert(
            &self,
            event: EngineEvent,
            source: &str,
            received_at_ms: u64,
        ) -> Result<InsertOutcome, SqliteWasmError> {
            // 1. Structural validity (cheap pre-filter; not a signature check).
            if !event.is_structurally_valid() {
                let id = event.id_bytes().unwrap_or([0u8; 32]);
                return Ok(InsertOutcome::Rejected {
                    id,
                    reason: RejectReason::Malformed(
                        "invalid id/pubkey/sig length or non-hex".into(),
                    ),
                });
            }
            // `is_structurally_valid` guarantees both decode; the `else` arm is a
            // defensive D6 no-panic fallback, not a reachable state.
            let (Some(id), Some(pubkey)) = (event.id_bytes(), event.pubkey_bytes()) else {
                return Ok(InsertOutcome::Rejected {
                    id: [0u8; 32],
                    reason: RejectReason::Malformed("id/pubkey hex decode".into()),
                });
            };

            // 2. Ephemeral kinds — never stored.
            if event.is_ephemeral() {
                return Ok(InsertOutcome::Ephemeral { id });
            }

            // 3. NIP-40 expiration already in the past on arrival.
            if let Some(exp) = event.expiration() {
                if exp <= received_at_ms / 1000 {
                    return Ok(InsertOutcome::Rejected {
                        id,
                        reason: RejectReason::ExpiredOnArrival,
                    });
                }
            }

            let blob = conv::encode_blob(&event)?;
            let conn = self.db.borrow();
            with_txn(&conn, |c| {
                insert_in_txn(c, &event, &id, &pubkey, &blob, source, received_at_ms)
            })
        }
    }

    fn insert_in_txn(
        c: &SqliteConn,
        event: &EngineEvent,
        id: &EventId,
        pubkey: &PubKey,
        blob: &[u8],
        source: &str,
        received_at_ms: u64,
    ) -> Result<InsertOutcome, SqliteWasmError> {
        // 4. Per-id tombstone suppression.
        if let Some(tomb) = tombstones::get(c, id)? {
            let applies = match tomb.origin {
                TombstoneOrigin::Kind5 => tomb.deleter_pubkey == Some(*pubkey),
                TombstoneOrigin::NIP40Expiry | TombstoneOrigin::AdminPurge => true,
            };
            if applies {
                return Ok(InsertOutcome::Tombstoned {
                    id: *id,
                    kind5_event_id: tomb.kind5_event_id,
                    origin: tomb.origin,
                });
            }
            // Foreign pre-tombstone — drop it and let the event in.
            tombstones::delete(c, id)?;
        }

        // 5. Addressable (param-replaceable) coordinate tombstone suppression.
        if event.is_param_replaceable() {
            let d = event.d_tag().unwrap_or_default();
            if let Some((deleted_at, kind5_event_id)) =
                tombstones::addr_deleted_at(c, event.kind, pubkey, &d)?
            {
                if deleted_at >= event.created_at {
                    return Ok(InsertOutcome::Tombstoned {
                        id: *id,
                        kind5_event_id,
                        origin: TombstoneOrigin::Kind5,
                    });
                }
            }
        }

        // 6. Duplicate id (any kind) — bump provenance, leave the primary alone.
        if event_exists(c, id)? {
            let sources_after = provenance::upsert(c, id, source, received_at_ms)?;
            return Ok(InsertOutcome::Duplicate {
                id: *id,
                sources_after,
            });
        }

        // 7. kind:5 — apply self-deletes, then store the kind:5 event itself.
        if event.kind == 5 {
            delete::apply_kind5(c, event, source, received_at_ms)?;
            store_primary(c, event, id, pubkey, blob, received_at_ms)?;
            let sources_after = provenance::upsert(c, id, source, received_at_ms)?;
            ingest_log::append_inserted(c, id, blob, source, received_at_ms)?;
            return Ok(InsertOutcome::Inserted {
                id: *id,
                sources_after,
            });
        }

        // 8. Replaceable / addressable supersession.
        if event.is_replaceable() || event.is_param_replaceable() {
            if let Some((existing_id, existing_created_at)) = find_replaceable(c, event, pubkey)? {
                // Newer wins; on a created_at tie the lexicographically lower id
                // is retained (NIP-01).
                let incoming_wins = event.created_at > existing_created_at
                    || (event.created_at == existing_created_at && *id < existing_id);
                if !incoming_wins {
                    return Ok(InsertOutcome::Superseded {
                        id: *id,
                        current_id: existing_id,
                    });
                }
                delete::remove_event(c, &existing_id)?;
                store_primary(c, event, id, pubkey, blob, received_at_ms)?;
                let _sources_after = provenance::upsert(c, id, source, received_at_ms)?;
                ingest_log::append_replaced(c, id, &existing_id, blob, source, received_at_ms)?;
                return Ok(InsertOutcome::Replaced {
                    new_id: *id,
                    replaced_id: existing_id,
                });
            }
        }

        // 9. Fresh insert (non-replaceable, or replaceable with no prior).
        store_primary(c, event, id, pubkey, blob, received_at_ms)?;
        let sources_after = provenance::upsert(c, id, source, received_at_ms)?;
        ingest_log::append_inserted(c, id, blob, source, received_at_ms)?;
        Ok(InsertOutcome::Inserted {
            id: *id,
            sources_after,
        })
    }

    /// Write the primary `events` row plus one `event_tags` row per single-letter
    /// tag (the tci / atci / ktci index source).
    fn store_primary(
        c: &SqliteConn,
        event: &EngineEvent,
        id: &EventId,
        pubkey: &PubKey,
        blob: &[u8],
        received_at_ms: u64,
    ) -> Result<(), SqliteWasmError> {
        // `d_tag` is meaningful only for addressable (param-replaceable) events;
        // it is NULL otherwise. A param-replaceable with no `d` tag stores `""`
        // (NIP-01 treats a missing `d` as the empty identifier).
        let d_col: Option<Vec<u8>> = if event.is_param_replaceable() {
            Some(event.d_tag().unwrap_or_default())
        } else {
            None
        };
        let expires_at = event.expiration();

        exec_write(
            c,
            "INSERT INTO events
                 (id, pubkey, kind, created_at, d_tag, expires_at, raw, received_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            &[
                SqlVal::Blob(id),
                SqlVal::Blob(pubkey),
                SqlVal::Int(i64::from(event.kind)),
                SqlVal::Int(event.created_at as i64),
                match &d_col {
                    Some(d) => SqlVal::Blob(d),
                    None => SqlVal::Null,
                },
                match expires_at {
                    Some(e) => SqlVal::Int(e as i64),
                    None => SqlVal::Null,
                },
                SqlVal::Blob(blob),
                SqlVal::Int(received_at_ms as i64),
            ],
        )?;

        for (name, value) in event.single_letter_tags() {
            let mut name_buf = [0u8; 4];
            let name_str = name.encode_utf8(&mut name_buf);
            exec_write(
                c,
                "INSERT INTO event_tags
                     (event_id, tag_name, tag_value, pubkey, kind, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                &[
                    SqlVal::Blob(id),
                    SqlVal::Text(name_str),
                    SqlVal::Text(value),
                    SqlVal::Blob(pubkey),
                    SqlVal::Int(i64::from(event.kind)),
                    SqlVal::Int(event.created_at as i64),
                ],
            )?;
        }
        Ok(())
    }

    fn event_exists(c: &SqliteConn, id: &EventId) -> Result<bool, SqliteWasmError> {
        let stmt = c.prepare("SELECT 1 FROM events WHERE id = ?1")?;
        stmt.bind_blob(1, id)?;
        stmt.step()
    }

    /// The current replaceable/addressable for this event's coordinate, as
    /// `(id, created_at)`, or `None`. Index-served by `idx_events_akci` (regular)
    /// or `idx_events_dtag` (addressable).
    fn find_replaceable(
        c: &SqliteConn,
        event: &EngineEvent,
        pubkey: &PubKey,
    ) -> Result<Option<(EventId, u64)>, SqliteWasmError> {
        let stmt = if event.is_param_replaceable() {
            let d = event.d_tag().unwrap_or_default();
            let s = c.prepare(
                "SELECT id, created_at FROM events
                 WHERE pubkey = ?1 AND kind = ?2 AND d_tag = ?3
                 ORDER BY created_at DESC, id ASC LIMIT 1",
            )?;
            s.bind_blob(1, pubkey)?;
            s.bind_int64(2, i64::from(event.kind))?;
            s.bind_blob(3, &d)?;
            s
        } else {
            let s = c.prepare(
                "SELECT id, created_at FROM events
                 WHERE pubkey = ?1 AND kind = ?2
                 ORDER BY created_at DESC, id ASC LIMIT 1",
            )?;
            s.bind_blob(1, pubkey)?;
            s.bind_int64(2, i64::from(event.kind))?;
            s
        };
        if stmt.step()? {
            let id_bytes = stmt.column_blob(0)?;
            let created_at = stmt.column_int64(1)? as u64;
            match <[u8; 32]>::try_from(id_bytes.as_slice()) {
                Ok(id) => Ok(Some((id, created_at))),
                Err(_) => Err(SqliteWasmError::Column(
                    "replaceable id not 32 bytes".into(),
                )),
            }
        } else {
            Ok(None)
        }
    }
}
