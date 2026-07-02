//! Test suite for the `nmp-nostr-lmdb` fork, split by behavior area
//! (issue #962). Each submodule under `lib_tests/` covers one behavior;
//! shared fixtures (sample event corpus, `TempDatabase` harness) live in
//! `lib_tests/fixtures.rs`.

#[path = "lib_tests/deletion.rs"]
mod deletion;
#[path = "lib_tests/fixtures.rs"]
mod fixtures;
#[path = "lib_tests/nip01_tie_breaking.rs"]
mod nip01_tie_breaking;
#[path = "lib_tests/queries_and_maintenance.rs"]
mod queries_and_maintenance;
#[path = "lib_tests/replaceable_events.rs"]
mod replaceable_events;
#[path = "lib_tests/save_and_query.rs"]
mod save_and_query;
