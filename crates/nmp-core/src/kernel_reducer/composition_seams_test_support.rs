impl super::KernelReducer {
    /// Test-only: seed the active account directly (no Identity command).
    ///
    /// Lets headless/browser-runtime tests reach the `NeedsSign` publish path
    /// (which requires an active account) without the native actor thread's
    /// roster machinery. Mirrors `Kernel::set_active_account_for_test`.
    pub fn set_active_account_for_test(&mut self, pubkey: impl Into<String>) {
        self.kernel.set_active_account_for_test(pubkey);
    }

    #[allow(clippy::too_many_arguments)]
    pub fn project_raw_event_for_test(
        &mut self,
        id: &str,
        pubkey: &str,
        created_at: u64,
        kind: u32,
        tags: Vec<Vec<String>>,
        content: &str,
    ) {
        self.kernel
            .project_raw_event_for_test(id, pubkey, created_at, kind, tags, content);
    }
}
