use super::*;

// -- check(): banned authority shapes fire ------------------------------

#[test]
fn flags_oncelock_of_runtime_handle() {
    let hits = check(
        "static ACTIVE_WALLET_RUNTIME: OnceLock<WalletRuntimeHandle> = OnceLock::new();",
        false,
        false,
    );
    assert_eq!(hits.len(), 1, "OnceLock<Handle> must fire");
    assert!(hits[0].1.contains("D21"));
    assert!(hits[0].1.contains("ACTIVE_WALLET_RUNTIME"));
}

#[test]
fn flags_oncelock_of_arc() {
    let hits = check(
        "static GLOBAL_BROKER: OnceLock<Arc<BunkerBroker>> = OnceLock::new();",
        false,
        false,
    );
    assert_eq!(hits.len(), 1, "OnceLock<Arc<…>> must fire");
}

#[test]
fn flags_oncelock_of_rwlock_hook() {
    let hits = check(
        "static HOOK: OnceLock<RwLock<Option<BunkerHookFn>>> = OnceLock::new();",
        false,
        false,
    );
    assert_eq!(hits.len(), 1, "the bunker/NIP-55 HOOK shape must fire");
}

#[test]
fn flags_oncelock_of_mutex_registry() {
    let hits = check(
        "static SESSIONS: OnceLock<Mutex<HashMap<u64, Arc<Session>>>> = OnceLock::new();",
        false,
        false,
    );
    assert_eq!(hits.len(), 1, "OnceLock<Mutex<…>> registry must fire");
}

#[test]
fn flags_pub_static() {
    let hits = check(
        "pub static GLOBAL_DRIVER: OnceLock<Arc<Nip55Driver>> = OnceLock::new();",
        false,
        false,
    );
    assert_eq!(hits.len(), 1, "a `pub static` authority global must fire");
}

#[test]
fn flags_pub_crate_static() {
    let hits = check(
        "    pub(crate) static DRIVER: OnceLock<Arc<Nip55Driver>> = OnceLock::new();",
        false,
        false,
    );
    assert_eq!(
        hits.len(),
        1,
        "a `pub(crate) static` authority global must fire"
    );
}

#[test]
fn flags_bare_mutex_with_state() {
    let hits = check(
        "static STORE: Mutex<Option<Session>> = Mutex::new(None);",
        false,
        false,
    );
    assert_eq!(hits.len(), 1, "a bare Mutex<…> holding state must fire");
}

#[test]
fn flags_bare_rwlock_with_sender() {
    let hits = check(
        "static SINK: RwLock<Option<Sender<Frame>>> = RwLock::new(None);",
        false,
        false,
    );
    assert_eq!(hits.len(), 1, "a bare RwLock<…> holding a Sender must fire");
}

#[test]
fn flags_lazy_static_ref() {
    // The `lazy_static! { static ref … }` body form.
    let hits = check(
        "    static ref DRIVER: Arc<Nip55Driver> = Arc::new(Nip55Driver::new());",
        false,
        false,
    );
    assert_eq!(
        hits.len(),
        1,
        "lazy_static `static ref` of Arc<…> must fire"
    );
}

#[test]
fn flags_lazy_of_mutex() {
    let hits = check(
        "static BROKER2: Lazy<Mutex<BunkerBroker>> = Lazy::new(|| Mutex::new(BunkerBroker::new()));",
        false,
        false,
    );
    assert_eq!(hits.len(), 1, "Lazy<Mutex<…>> must fire");
}

#[test]
fn flags_atomic_ptr() {
    let hits = check(
        "static SLOT: AtomicPtr<Session> = AtomicPtr::new(std::ptr::null_mut());",
        false,
        false,
    );
    assert_eq!(hits.len(), 1, "AtomicPtr<…> must fire");
}

// -- check(): allowed shapes do NOT fire --------------------------------

#[test]
fn does_not_flag_oncelock_bool() {
    // The wire_log.rs ENABLED residual — read-once Copy config.
    let hits = check(
        "    static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();",
        false,
        false,
    );
    assert!(
        hits.is_empty(),
        "OnceLock<bool> read-once config must NOT fire"
    );
}

#[test]
fn does_not_flag_oncelock_option_pathbuf() {
    // The socket_io.rs LOG_PATH residual — read-once config.
    let hits = check(
        "    static LOG_PATH: OnceLock<Option<PathBuf>> = OnceLock::new();",
        false,
        false,
    );
    assert!(
        hits.is_empty(),
        "OnceLock<Option<PathBuf>> read-once config must NOT fire"
    );
}

#[test]
fn does_not_flag_oncelock_regex() {
    let hits = check(
        "    static R: OnceLock<Regex> = OnceLock::new();",
        false,
        false,
    );
    assert!(
        hits.is_empty(),
        "OnceLock<Regex> compiled-config cache must NOT fire"
    );
}

#[test]
fn does_not_flag_oncelock_string() {
    let hits = check(
        "static BUILD: OnceLock<String> = OnceLock::new();",
        false,
        false,
    );
    assert!(hits.is_empty(), "OnceLock<String> config must NOT fire");
}

#[test]
fn does_not_flag_mutex_unit_serialization_token() {
    let hits = check("static SERIAL: Mutex<()> = Mutex::new(());", false, false);
    assert!(
        hits.is_empty(),
        "Mutex<()> serialization token must NOT fire"
    );
}

#[test]
fn does_not_flag_const() {
    let hits = check("const MAX_IN_FLIGHT: usize = 8;", false, false);
    assert!(hits.is_empty(), "a const must NOT fire");
}

#[test]
fn does_not_flag_instance_field() {
    // The K2 goal pattern: per-app state as a struct field, not a static.
    let hits = check(
        "    wallet_runtime: Arc<WalletRuntimeHandle>,",
        false,
        false,
    );
    assert!(hits.is_empty(), "an instance field must NOT fire");
}

#[test]
fn does_not_flag_comment_line() {
    let hits = check(
        "// static GLOBAL_BROKER: OnceLock<Arc<BunkerBroker>> = OnceLock::new();",
        true,
        false,
    );
    assert!(hits.is_empty(), "comment lines must not be flagged");
}

#[test]
fn does_not_flag_in_test_cfg() {
    let hits = check(
        "static MOCK: OnceLock<Arc<BunkerBroker>> = OnceLock::new();",
        false,
        true,
    );
    assert!(
        hits.is_empty(),
        "#[cfg(test)] bodies must not be flagged — test doubles never ship"
    );
}

#[test]
fn does_not_misfire_on_lazy_static_macro_invocation_line() {
    // The `lazy_static::lazy_static! {` opener has no `static ` keyword at a
    // word boundary preceded by whitespace — only `lazy_static`. Must not fire.
    let hits = check("lazy_static::lazy_static! {", false, false);
    assert!(
        hits.is_empty(),
        "the lazy_static! macro opener must NOT fire"
    );
}

// -- doctrine-allow: D21 requires a reason ------------------------------

#[test]
fn allow_requires_reason() {
    // Bare allow → still fires (driver checks line_allows_d21 → false).
    assert!(!line_allows_d21(
        "static FOO: OnceLock<Arc<X>> = OnceLock::new(); // doctrine-allow: D21"
    ));
    // With a reason → silenced.
    assert!(line_allows_d21(
        "static FOO: OnceLock<Arc<X>> = OnceLock::new(); // doctrine-allow: D21 — justified residual, tracked in #999"
    ));
    // ASCII-hyphen separator also accepted.
    assert!(line_allows_d21(
        "static FOO: OnceLock<Arc<X>> = OnceLock::new(); // doctrine-allow: D21 - ascii reason"
    ));
    // Reason that is only whitespace → NOT silenced.
    assert!(!line_allows_d21(
        "static FOO: OnceLock<Arc<X>> = OnceLock::new(); // doctrine-allow: D21 —   "
    ));
}

// -- col reporting ------------------------------------------------------

#[test]
fn col_is_1_indexed_at_static_keyword() {
    let hits = check(
        "    static HOOK: OnceLock<RwLock<Option<Hook>>> = OnceLock::new();",
        false,
        false,
    );
    assert_eq!(hits.len(), 1);
    // `static` begins at byte offset 4 (after four spaces) → column 5.
    assert_eq!(
        hits[0].0, 5,
        "column must be 1-indexed at the `static` keyword"
    );
}

// -- file_in_scope ------------------------------------------------------

#[test]
fn k2_crates_are_in_scope() {
    for c in K2_CRATES {
        let p = format!("crates/{}/src/lib.rs", c);
        assert!(file_in_scope(Path::new(&p)), "{} src must be in scope", c);
    }
    assert!(file_in_scope(Path::new(
        "/abs/crates/nmp-ffi/src/signer_broker.rs"
    )));
}

#[test]
fn non_k2_crates_are_out_of_scope() {
    // nmp-marmot, nmp-store, apps/chirp etc. are outside the K2 blast radius.
    assert!(!file_in_scope(Path::new("crates/nmp-marmot/src/lib.rs")));
    assert!(!file_in_scope(Path::new("crates/nmp-store/src/lib.rs")));
    assert!(!file_in_scope(Path::new(
        "apps/chirp/crates/nmp-app-chirp/src/lib.rs"
    )));
    // The out-of-workspace android FFI crate is not in the gate scope.
    assert!(!file_in_scope(Path::new(
        "apps/chirp/crates/nmp-chirp-android-ffi/src/session.rs"
    )));
}

#[test]
fn doctrine_lint_source_is_out_of_scope() {
    assert!(!file_in_scope(Path::new(
        "crates/nmp-testing/bin/doctrine-lint/rules/d21.rs"
    )));
}
