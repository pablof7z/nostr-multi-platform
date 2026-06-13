//! `ActionRegistrar` trait impl for [`NmpApp`]. Extracted from `lib.rs` to
//! keep that file under its baseline LOC ceiling. Loaded via
//! `#[path = "registrar.rs"] mod registrar_impl;` in `lib.rs`.

use crate::NmpApp;

impl nmp_core::substrate::ActionRegistrar for NmpApp {
    fn register_action<M: nmp_core::substrate::ActionModule + 'static>(&mut self, module: M) {
        NmpApp::register_action::<M>(self, module);
    }

    /// ADR-0049 Part 1 — override the trait default so the canonical NMP
    /// defaults (`nmp_nip02` / `nmp_nip17` / `nmp_nip57` / `nmp_router`, which
    /// register through `&mut impl AppHost`) get true entry-or-insert yielding
    /// semantics. Without this override the trait's default impl would delegate
    /// to `register_action` (the app path), recording every default as an app
    /// registration and making a repeated `register_defaults` collide.
    fn register_default_action<M: nmp_core::substrate::ActionModule + 'static>(
        &mut self,
        module: M,
    ) -> bool {
        NmpApp::register_default_action::<M>(self, module)
    }
}
