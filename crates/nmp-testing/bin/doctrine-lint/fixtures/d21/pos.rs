// D21 positive fixture — ambient-authority process-global statics that MUST
// fire (K2 / ADR-0072 §D6 regression gate). Each banned declaration below is a
// module-level `static`/`OnceLock`/`Lazy`/`lazy_static!` holding NON-CONST,
// mutable or interior-mutable, process-wide state that confers authority — the
// exact shape of the five globals K2 rungs 5.2/5.3/5.5 deleted
// (`ACTIVE_WALLET_RUNTIME`, `GLOBAL_BROKER`, `GLOBAL_DRIVER`, and the two
// bunker / NIP-55 hook statics).

use std::sync::{Mutex, OnceLock, RwLock};

// (1) `OnceLock` wrapping a runtime handle — the `ACTIVE_WALLET_RUNTIME` shape.
static ACTIVE_WALLET_RUNTIME: OnceLock<WalletRuntimeHandle> = OnceLock::new();

// (2) `OnceLock<Arc<...>>` — the `GLOBAL_BROKER` / `GLOBAL_DRIVER` shape: a
// process-wide shared handle that confers authority on whoever reads it.
static GLOBAL_BROKER: OnceLock<Arc<BunkerBroker>> = OnceLock::new();

// (3) `OnceLock<RwLock<Option<...>>>` — the bunker / NIP-55 `HOOK` shape: a
// process-global mutable hook slot.
static HOOK: OnceLock<RwLock<Option<BunkerHookFn>>> = OnceLock::new();

// (4) `OnceLock<Mutex<...>>` — a process-global mutable registry / handle table.
static SESSIONS: OnceLock<Mutex<Vec<Session>>> = OnceLock::new();

// (5) Bare `static ... : Mutex<...>` holding real state (NOT a `Mutex<()>`
// serialization token) — interior-mutable process-wide authority.
static STORE: Mutex<Option<Session>> = Mutex::new(None);

// (6) Bare `static ... : RwLock<...>` holding a sender — authority by shape.
static SINK: RwLock<Option<Sender<Frame>>> = RwLock::new(None);

// (7) `lazy_static!` of an authority-shaped value.
lazy_static::lazy_static! {
    static ref DRIVER: Arc<Nip55Driver> = Arc::new(Nip55Driver::new());
}

// (8) `Lazy<...>` (once_cell) of an authority-shaped value.
static BROKER2: Lazy<Mutex<BunkerBroker>> = Lazy::new(|| Mutex::new(BunkerBroker::new()));
