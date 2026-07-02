//! Typestate transitions and lifecycle handoff for [`NmpAppBuilder`].

use nmp_core::substrate::ActionRegistrar;

use crate::{relay_config, NmpApp, NmpConfigStatus};

use super::{NmpAppBuilder, ProjectionsDeclared, RelaysDeclared, RunConfig, StorageSet, Unstarted};

// ── Storage-selection transitions (Unstarted → StorageSet) ──────────────────

impl NmpAppBuilder<Unstarted> {
    /// Use a persistent LMDB store at `path`.
    ///
    /// Transitions to `NmpAppBuilder<StorageSet>`. A projection-consumption
    /// decision (`.declare_consumed_projections` /
    /// `.consume_all_builtin_projections`) is still required before `start()`.
    ///
    /// In practice `path` is the host-provided application-support directory
    /// (iOS) or files directory (Android). A `NULL` or empty `path` passed to
    /// the runtime falls back to the `NMP_LMDB_PATH` env var, then
    /// the in-memory store.
    ///
    /// # Panics
    ///
    /// Does not panic; an empty or invalid path is silently treated as "unset"
    /// by the native runtime setter.
    pub fn storage_path(self, path: impl Into<String>) -> NmpAppBuilder<StorageSet> {
        let path_string = path.into();
        // The runtime owns the storage-path slot; C ABI wrappers call the same
        // native method.
        set_storage_path(self.app, &path_string);
        // Transfer ownership to the new builder WITHOUT running our own Drop.
        // `*mut NmpApp` is `Copy`, but `user_relays` (a `Vec`) is not — a plain
        // field move would hit E0509 ("cannot move out of type which implements
        // Drop"). `ptr::read` byte-copies the non-Copy field out, then
        // `mem::forget(self)` suppresses the destructor so neither the
        // `NmpApp` is freed nor `user_relays` is double-dropped. Ownership of
        // both transfers to the returned builder.
        let app = self.app;
        let user_relays = unsafe { std::ptr::read(&self.user_relays) };
        std::mem::forget(self);
        NmpAppBuilder {
            app,
            user_relays,
            _state: std::marker::PhantomData,
        }
    }

    /// Use an ephemeral in-memory store (explicit opt-in).
    ///
    /// This transitions to `NmpAppBuilder<StorageSet>`. A projection-consumption
    /// decision is still required before `start()` (see
    /// `.declare_consumed_projections` / `.consume_all_builtin_projections`).
    /// An in-memory store loses all events when the process exits — this opt-in
    /// makes that choice explicit and visible in code, unlike the old silent
    /// default where omitting a storage path gave in-memory
    /// storage without any declaration.
    ///
    /// Suitable for tests and short-lived tools. For production apps use
    /// `.storage_path(p)` instead.
    pub fn in_memory(self) -> NmpAppBuilder<StorageSet> {
        // Leave the storage-path slot at `None` (its default from
        // `new_app`). The actor thread then falls back to the in-memory
        // `EventStore` — same behaviour as before, but now the caller has
        // explicitly opted in.
        //
        // Transfer ownership WITHOUT running Drop (same pattern as
        // `storage_path` and `start` — see `storage_path` for the rationale,
        // including why `user_relays` needs `ptr::read`).
        let app = self.app;
        let user_relays = unsafe { std::ptr::read(&self.user_relays) };
        std::mem::forget(self);
        NmpAppBuilder {
            app,
            user_relays,
            _state: std::marker::PhantomData,
        }
    }
}

// ── Projection-consumption decision (StorageSet → ProjectionsDeclared) ───────
//
// ADR-0070 DEBT 2: the host MUST make an explicit decision about which Tier-2
// kernel built-in projections it consumes before `start()`. Forgetting is a
// compile error (start() only exists on ProjectionsDeclared). Two ways to
// decide — both advance the typestate:
//   * `.declare_consumed_projections(keys)` — narrow to the declared set.
//   * `.consume_all_builtin_projections()`  — explicit "I want everything".

impl NmpAppBuilder<StorageSet> {
    /// Declare the static set of Tier-2 kernel built-in projection keys this
    /// app consumes, and advance to `ProjectionsDeclared` (unlocking `start()`).
    ///
    /// The kernel then serializes ONLY these built-ins (plus any Tier-1
    /// host-registered projections, which self-gate by registration) into each
    /// pushed `SnapshotFrame` — the ADR-0070 narrowing optimization. Keys
    /// accumulate with any added earlier via the `AppHost` trait method (e.g.
    /// by a protocol crate during `register`), since the underlying
    /// `SnapshotRegistry` declaration is additive.
    ///
    /// This is the narrowing path. To opt into the full firehose explicitly
    /// (e.g. a TUI/desktop diagnostic shell that genuinely reads everything),
    /// call [`Self::consume_all_builtin_projections`] instead.
    #[must_use]
    pub fn declare_consumed_projections<I, K>(self, keys: I) -> NmpAppBuilder<ProjectionsDeclared>
    where
        I: IntoIterator<Item = K>,
        K: Into<String>,
    {
        // SAFETY: `self.app` non-null (builder invariant); not yet started.
        let app: &NmpApp = unsafe { &*self.app };
        NmpApp::declare_consumed_projections(app, keys);
        self.into_projections_declared()
    }

    /// Explicitly opt into receiving ALL Tier-2 kernel built-in projections
    /// (the full firehose — no narrowing), and advance to `ProjectionsDeclared`
    /// (unlocking `start()`).
    ///
    /// This is the **visible, greppable** "I want everything" choice. It sets
    /// the kernel's declared set to the explicit `DeclaredProjections::All`
    /// state (ADR-0070 / Workstream-E4: `All` is the ONE non-footgun way to mean
    /// "every Tier-2 built-in"). Use it only when the host genuinely consumes
    /// the full set (diagnostics shells, TUIs, tests). Production app shells
    /// should prefer [`Self::declare_consumed_projections`] to avoid serializing
    /// built-ins no screen reads (e.g. `relay_diagnostics`).
    ///
    /// The distinction from the old silent default is the whole point: omission
    /// no longer compiles (the typestate), and "everything" is now an explicit
    /// `All`, not a silent empty/undeclared set.
    #[must_use]
    pub fn consume_all_builtin_projections(self) -> NmpAppBuilder<ProjectionsDeclared> {
        // SAFETY: `self.app` non-null (builder invariant); not yet started.
        let app: &NmpApp = unsafe { &*self.app };
        // Explicit `All` — NOT a silent empty set. An undeclared app is the loud
        // forgotten-wiring footgun at runtime start; this records the
        // deliberate "I consume everything" intent.
        app.consume_all_builtin_projections();
        self.into_projections_declared()
    }

    /// Internal: move ownership of the inner `NmpApp` + relays into a
    /// `ProjectionsDeclared` builder WITHOUT running `Drop` (same `ptr::read` +
    /// `mem::forget` rationale as the storage transitions — see `storage_path`).
    fn into_projections_declared(self) -> NmpAppBuilder<ProjectionsDeclared> {
        let app = self.app;
        let user_relays = unsafe { std::ptr::read(&self.user_relays) };
        std::mem::forget(self);
        NmpAppBuilder {
            app,
            user_relays,
            _state: std::marker::PhantomData,
        }
    }
}

// ── Initial-relay decision (ProjectionsDeclared → RelaysDeclared) ────────────
//
// #1493: operator relay URLs are leaf-app policy, never an NMP default — not in
// `nmp-core`, not in `explicit composition`. The app MUST decide its initial relay set
// before `start()`. Forgetting is a compile error (`start()` only exists on
// `RelaysDeclared`). Two ways to decide — both advance the typestate:
//   * `.with_relays(iter)`        — declare the relay set the app starts with.
//   * `.without_initial_relays()` — explicit "this app ships no built-in relays".

impl NmpAppBuilder<ProjectionsDeclared> {
    /// Declare the initial relay set the app starts with, and advance to
    /// `RelaysDeclared` (unlocking `start()`).
    ///
    /// Each item is a `(url, role)` pair where `role` is a relay-role string
    /// (`"read"`, `"write"`, `"both"`, `"indexer"`, or a composite like
    /// `"both,indexer"`); the kernel canonicalizes it when the row is seeded.
    /// These values are leaf-app policy (#1493) — NMP supplies no default, so
    /// what the app declares here is exactly what the kernel starts with.
    ///
    /// # Panics
    ///
    /// Panics if `relays` is empty — that is the `.without_initial_relays()`
    /// case, which must be chosen explicitly so a no-relay start is never a
    /// silent accident.
    #[must_use]
    pub fn with_relays(
        mut self,
        relays: impl IntoIterator<Item = (impl Into<String>, impl Into<String>)>,
    ) -> NmpAppBuilder<RelaysDeclared> {
        self.user_relays = relays
            .into_iter()
            .map(|(url, role)| (url.into(), role.into()))
            .collect();
        assert!(
            !self.user_relays.is_empty(),
            "with_relays called with an empty set — use .without_initial_relays() \
             to start with no relays explicitly"
        );
        self.into_relays_declared()
    }

    /// Explicitly start with NO initial relays, and advance to `RelaysDeclared`
    /// (unlocking `start()`).
    ///
    /// This is the visible, greppable opt-out for offline/test/local apps. The
    /// kernel starts with an empty `configured_relays`; network operations
    /// fail-closed (`NoTargets`) until relays are added at runtime via
    /// `NmpApp::add_relay`. Use it only when the app genuinely ships no
    /// built-in relays — otherwise declare them with [`Self::with_relays`].
    #[must_use]
    pub fn without_initial_relays(mut self) -> NmpAppBuilder<RelaysDeclared> {
        self.user_relays = Vec::new();
        self.into_relays_declared()
    }

    /// Internal: move ownership into a `RelaysDeclared` builder WITHOUT running
    /// `Drop` (same `ptr::read` + `mem::forget` rationale as the other
    /// transitions — see `storage_path`).
    fn into_relays_declared(self) -> NmpAppBuilder<RelaysDeclared> {
        let app = self.app;
        let user_relays = unsafe { std::ptr::read(&self.user_relays) };
        std::mem::forget(self);
        NmpAppBuilder {
            app,
            user_relays,
            _state: std::marker::PhantomData,
        }
    }
}

// ── Terminal transition: start (RelaysDeclared only) ─────────────────────────

impl NmpAppBuilder<RelaysDeclared> {
    /// Consume the builder and start the NMP kernel.
    ///
    /// This is the **only** path from `NmpAppBuilder<ProjectionsDeclared>` to a
    /// live `*mut NmpApp`. It:
    ///
    /// 1. Calls [`NmpApp::start_runtime`] with the given `RunConfig`.
    /// 2. Releases ownership of the `NmpApp` pointer to the caller.
    ///
    /// `start()` is reachable ONLY after a projection-consumption decision
    /// (`.declare_consumed_projections` or `.consume_all_builtin_projections`)
    /// — ADR-0070 DEBT 2's compile-time enforcement. After this call, the
    /// builder is gone — no setter is reachable (compile error). The returned
    /// pointer is owned by the caller.
    ///
    /// # Safety
    ///
    /// The returned pointer is a valid, non-null `*mut NmpApp`. The caller is
    /// responsible for eventually dropping the returned handle.
    pub fn start(self, config: RunConfig) -> *mut NmpApp {
        let app = self.app;
        // Move the non-Copy `user_relays` out before forgetting `self` (same
        // E0509 rationale as the storage transitions).
        let user_relays = unsafe { std::ptr::read(&self.user_relays) };
        // Prevent `Drop` from double-freeing: consume `self` without running
        // the drop glue. The caller takes ownership of `app`.
        std::mem::forget(self);

        // The app's declared initial relay set (#1493): exactly what
        // `.with_relays(...)` declared, or empty if `.without_initial_relays()`
        // was chosen. NMP carries no relay fallback.
        let relay_defaults: Vec<(String, String)> = user_relays;

        // Decide the initial relay set:
        //   * Persistent store → load the JSON sidecar from the storage dir; on
        //     first run (no sidecar) persist the declared defaults, then use
        //     them. Subsequent runs reload the user's edited list.
        //   * In-memory store → no disk I/O; use the declared defaults directly.
        //
        // SAFETY: `app` is non-null (builder invariant) and not yet started, so
        // a shared borrow to read the storage path is sound.
        let initial_relays: Vec<(String, String)> = match unsafe { &*app }.storage_path_for_start()
        {
            Some(path) if !path.trim().is_empty() => {
                let dir = std::path::Path::new(&path);
                match relay_config::load(dir) {
                    Some(loaded) => loaded,
                    None => {
                        relay_config::save(dir, &relay_defaults);
                        relay_defaults
                    }
                }
            }
            // No storage path (in-memory) — use defaults, no sidecar.
            _ => relay_defaults,
        };

        // Stage the initial relays before start so `start_runtime` carries them
        // in `ActorCommand::Lifecycle(LifecycleCommand::Start { initial_relays })`.
        // SAFETY: `app` non-null; not yet started.
        unsafe { &*app }.set_initial_relays_for_start(initial_relays);

        // ADR-0070 DEBT 2: by the time we reach `start()` the host has ALREADY
        // made an explicit projection-consumption decision — the typestate
        // guarantees it (`ProjectionsDeclared` is only reachable via
        // `.declare_consumed_projections` or `.consume_all_builtin_projections`).
        // No runtime check is needed on the builder path. The complementary
        // `tracing::warn!` in `start_runtime` is the backstop for raw C ABI
        // wrappers, which are outside Rust's type system.

        // SAFETY: `app` is non-null (builder invariant).
        unsafe { &*app }.start_runtime(config.visible_limit as usize, config.emit_hz);
        app
    }
}

// ── AppHost + ActionRegistrar delegations (all states) ──────────────────────
//
// Every wiring method is available in every builder state. They do not advance
// the required chain; the only constraint is that they run before `start()`,
// which the typestate already guarantees.

impl<S> ActionRegistrar for NmpAppBuilder<S> {
    fn register_action<M: nmp_core::substrate::ActionModule + 'static>(
        &mut self,
        module: M,
    ) -> Result<(), nmp_core::substrate::RegistrationError> {
        // SAFETY: `self.app` non-null (builder invariant). Exclusive borrow via
        // `&mut self` means no aliasing.
        let app: &mut NmpApp = unsafe { &mut *self.app };
        app.register_action(module)
    }

    /// Route the yielding-default path to `NmpApp::register_default_action` (the
    /// kernel's true entry-or-insert semantics), not the trait default, which
    /// delegates to the app path and would record every canonical NMP default
    /// as an app registration.
    fn register_default_action<M: nmp_core::substrate::ActionModule + 'static>(
        &mut self,
        module: M,
    ) -> bool {
        // SAFETY: as `register_action` above.
        let app: &mut NmpApp = unsafe { &mut *self.app };
        app.register_default_action(module)
    }
}

// ── Drop guard ───────────────────────────────────────────────────────────────

impl<S> Drop for NmpAppBuilder<S> {
    /// Free the inner `NmpApp` if `start()` was never called.
    ///
    /// This prevents a memory leak when a builder is constructed but then
    /// dropped without starting, for example after an error during wiring.
    fn drop(&mut self) {
        // `start()` uses `mem::forget(self)` to bypass this destructor, so this
        // branch is only reached when the builder is dropped without starting.
        if !self.app.is_null() {
            // SAFETY: `self.app` is non-null and owned exclusively by the
            // builder (invariant). `start()` used `mem::forget` so this is the
            // sole drop point.
            unsafe {
                drop(Box::from_raw(self.app));
            }
        }
    }
}

/// Write the storage path into the `NmpApp`'s `storage_path` slot.
fn set_storage_path(app: *mut NmpApp, path: &str) {
    let status = unsafe { &*app }.set_storage_path(Some(path.to_string()));
    debug_assert_eq!(
        status,
        NmpConfigStatus::Ok,
        "builder storage_path must run before NmpApp::start_runtime"
    );
}
