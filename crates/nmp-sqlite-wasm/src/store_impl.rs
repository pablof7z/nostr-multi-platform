//! `OpfsSqliteStore` inherent impl (open, txn helpers) + the single scoped
//! `unsafe impl Send + Sync` (ADR-0054 §3). The `EventStore` trait impl lives
//! in `nmp-store`, not here. PR-2 adds the async `open` entry point + the
//! `unsafe impl`; the txn/query helpers arrive in PR-3 (#1007).

#[cfg(target_arch = "wasm32")]
mod wasm_impl {
    use crate::shim::{SqliteConn, SqliteWasmError};
    use crate::OpfsSqliteStore;
    use core::cell::RefCell;

    impl OpfsSqliteStore {
        /// Open the OPFS-SQLite store named `database_name`.
        ///
        /// This is the one-time async pool-open entry point: it initialises the
        /// SQLite WASM module, installs the opfs-sahpool VFS, and opens the
        /// database on it (all per ADR-0054 §1). Every later store operation is
        /// synchronous over the returned handle. No schema is created here — DDL
        /// is PR-3's concern; this returns a bare connected store.
        pub async fn open(database_name: &str) -> Result<Self, SqliteWasmError> {
            let conn = SqliteConn::open(database_name).await?;
            Ok(Self {
                db: RefCell::new(conn),
            })
        }

        /// Borrow the underlying connection cell.
        ///
        /// PR-3's `nmp-store` `EventStore` wrapper runs statements through this;
        /// the `RefCell` enforces single-borrow discipline at runtime within the
        /// owning Worker actor (ADR-0054 §3 — `RefCell`, not `Mutex`).
        pub fn conn(&self) -> &RefCell<SqliteConn> {
            &self.db
        }
    }

    // SAFETY: `OpfsSqliteStore` wraps a `RefCell<SqliteConn>` whose `JsValue`
    // engine handle is `!Send + !Sync`. The store is constructed inside, and
    // only ever observed by, the single Web Worker event loop that opened its
    // OPFS SyncAccessHandle pool (ADR-0047 §1: the Worker IS the actor; D4
    // single writer). No other thread ever obtains a reference to the handle.
    // The `target_feature = "atomics"` `compile_error!` guard in the crate root
    // forbids the only build configuration (wasm threads) that could introduce
    // a second thread and make this impl unsound. This is the ONLY `unsafe` in
    // the crate and is forbidden anywhere outside it (ADR-0054 §3).
    unsafe impl Send for OpfsSqliteStore {}
    // SAFETY: see the `unsafe impl Send` justification directly above — the same
    // single-Worker-actor ownership invariant makes `&OpfsSqliteStore` safe to
    // share, vacuously, because no second thread can ever exist to share it.
    unsafe impl Sync for OpfsSqliteStore {}
}
