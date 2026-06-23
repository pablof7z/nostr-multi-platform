//! `ActorCommand::Protocol` dispatch arm.
//!
//! Extracted from `dispatch/mod.rs` to keep it under the 500-LOC ceiling.
//! No behaviour change — all logic is verbatim from the original file.

use crate::actor::tick::emit_now;
use crate::relay::OutboundMessage;

use super::substrate_adapters::{
    ActionStageTrackerAdapter, ErrorSurfaceAdapter, HostOpHandlerAccessAdapter,
    KernelClockAdapter, LocalSignerAccessAdapter, RecipientRelayLookupAdapter,
    WalletKernelAccessAdapter, ZapProfileLookupAdapter,
};
use super::ActorContext;

/// Dispatch `ActorCommand::Protocol(cmd)`.
///
/// Step 1.b — the open-seam dispatch arm. Debt C replaced the
/// prior 12-positional-closure bundle with typed capability
/// adapters (`KernelClock`/`LocalSignerAccess`/`DmInboxLookup`/
/// `ErrorSurface`/`ActionStageTracker`/`RecipientRelayLookup`).
/// Each adapter borrows a `RefCell`-wrapped reference to the
/// kernel or identity runtime; the kernel and identity types
/// stay crate-private (D0 — NIP crates name neither). Borrows
/// are released the moment `cmd.run` returns — the worker thread
/// the LNURL command spawns owns its own `Sender<ActorCommand>`
/// clone and never re-enters the context.
///
/// V-38: the dispatch arm additionally attaches an `&mut Kernel`
/// and an outbound-frame sink so NIP-crate runtimes (today
/// `nmp-nip47`) can mutate the kernel synchronously and surface
/// relay frames the actor drains into `send_all_outbound`
/// without re-entering through the `send` channel.
pub(super) fn protocol(
    cmd: Box<dyn crate::substrate::ProtocolCommand>,
    ctx: &mut ActorContext<'_>,
) -> Option<Vec<OutboundMessage>> {
    let tx = ctx.command_tx_self.clone();
    let send = move |c: crate::actor::ActorCommand| {
        // D6 — disconnected sender (post-Shutdown) is a benign
        // send-failure on the worker side; swallow as a no-op.
        let _ = tx.send(c);
    };
    // Snapshot the DM-inbox lookup Arc for the duration of this
    // dispatch arm. The `Arc<dyn DmInboxRelayLookup>` is the
    // production kind:10050 cache (`nmp_nip17::DmRelayCache`).
    let dm_lookup = ctx.kernel.dm_inbox_relays_arc();
    // The kernel + identity adapters share disjoint borrows of the
    // actor context via `RefCell`. `ProtocolCommand::run` is
    // single-threaded sync, so the inner `borrow`/`borrow_mut`
    // calls serialize naturally.
    //
    // ADR-0052 §D5: ALL kernel-touching capability adapters — including
    // the new `WalletKernelAccessAdapter` (mutating) and
    // `ZapProfileLookupAdapter` (reading) — go through this one
    // `kernel_cell` via per-call `try_borrow[_mut]`. The prior V-38
    // `with_kernel` exclusive borrow (a long-lived `&mut Kernel` held
    // for the whole `cmd.run`) is deleted: a wallet command's eight
    // mutations now interleave with the sibling reads through the same
    // `RefCell`, so no separate exclusive-borrow window is needed.
    let identity_cell = std::cell::RefCell::new(&*ctx.identity);
    let kernel_cell = std::cell::RefCell::new(&mut *ctx.kernel);

    let clock = KernelClockAdapter {
        kernel: &kernel_cell,
    };
    let signers = LocalSignerAccessAdapter {
        identity: &identity_cell,
    };
    let errors = ErrorSurfaceAdapter {
        kernel: &kernel_cell,
    };
    let stages = ActionStageTrackerAdapter {
        kernel: &kernel_cell,
    };
    let recipients = RecipientRelayLookupAdapter {
        kernel: &kernel_cell,
    };
    // ADR-0052 §D4 — per-app host-op handler accessor, so the
    // `HostOpCommand` (which replaced the deleted `DispatchHostOp` arm)
    // can clone the start-time configured handler at `run` time.
    // Reaches no kernel/identity state, so it needs no `RefCell`
    // borrow and is safe to read inside the whole-body catch_unwind.
    let host_op_handler = HostOpHandlerAccessAdapter {
        handler: ctx.config.host_op_handler.clone(),
    };
    // ADR-0052 §D5 — narrow wallet kernel-mutation + zap-profile-read
    // adapters replace the deleted `kernel_mut()` / `lnurl_for_pubkey`
    // surfaces. Both borrow the SAME `kernel_cell` the read adapters
    // use, via per-call `try_borrow_mut` / `try_borrow`, so the prior
    // long-lived `with_kernel` exclusive borrow is gone and the wallet
    // command's eight mutations interleave naturally with the other
    // capability reads during `cmd.run`.
    let wallet_kernel = WalletKernelAccessAdapter {
        kernel: &kernel_cell,
    };
    let zap_profiles = ZapProfileLookupAdapter {
        kernel: &kernel_cell,
    };

    // A second sender clone for the worker-thread surface. Cloning
    // a `mpsc::Sender` is cheap (atomic ref-count bump); the
    // dispatch arm always populates this slot in production.
    let worker_tx = ctx.command_tx_self.clone();
    let mut outbound: Vec<crate::relay::OutboundMessage> = Vec::new();
    // ADR-0052 §D4 guarantee #1 — WHOLE-BODY panic isolation. Before
    // this rung the `Protocol` arm called `cmd.run` bare; a panic in a
    // command's own non-capability logic unwound the actor thread
    // (only per-accessor D15 shortcuts were caught). The
    // `DispatchHostOp` arm we are deleting wrapped its handler in
    // `catch_unwind`; merging the two seams MUST preserve that, so the
    // entire `cmd.run` is wrapped here. A panic becomes a logged
    // `ProtocolCommand panicked` (the same observable surface as an
    // `Err` return) and the actor survives.
    //
    // Borrow scoping (#1364 / ADR-0052 §D5): NO long-lived
    // `kernel_cell.borrow_mut()` is held across `cmd.run`. Every kernel
    // touch a command makes — including the very first one, the
    // `HostOpCommand`'s `record_action_stage_requested` write — goes
    // through a per-call `try_borrow_mut` on the sibling adapters (see
    // `ActionStageTrackerAdapter::record_requested`). Because no borrow
    // outlives the call, that `try_borrow_mut` always succeeds, so a
    // panic-guarded `HostOpCommand` records its `Requested` stage like
    // every other action path (the #1356 regression — a held
    // `with_kernel` exclusive borrow that made the `try_borrow_mut`
    // return `Err` and silently drop the stage — was eliminated when
    // that exclusive borrow was deleted). On a panic the unwinding
    // closure has no outstanding `RefCell` borrow to drop, so the
    // post-arm `emit_now` re-borrow is always safe.
    // `AssertUnwindSafe` is required because the closure captures `&mut`
    // state (`outbound`, the adapters' shared `RefCell`s) across the
    // unwind boundary; that is sound here because a panic abandons the
    // command and the actor reads no partially-mutated `outbound`
    // (`run_err` is `Err`-shaped on panic and the outbound drain below
    // only carries whatever frames were pushed before the panic, which
    // is benign — same as an early `Err` return).
    let run_err = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut pctx = crate::substrate::ProtocolCommandContext::new(
            crate::substrate::ProtocolCommandContextParts {
                send: &send,
                command_sender: worker_tx,
                clock: &clock,
                signers: &signers,
                dms: &*dm_lookup,
                errors: &errors,
                stages: &stages,
                recipients: &recipients,
                host_op_handler: &host_op_handler,
                // ADR-0052 §D5 — the narrow wallet kernel-mutation +
                // zap-profile-read capabilities. A wallet/zap command
                // reaches its needs through these; every other command
                // ignores them (it holds the noop singleton's surface).
                wallet_kernel: &wallet_kernel,
                zap_profiles: &zap_profiles,
            },
        )
        .with_outbound(&mut outbound);
        cmd.run(&mut pctx)
    }))
    .unwrap_or_else(|_| {
        // A panic in the command body is converted to the same
        // observable surface as an `Err` return (logged below). For a
        // host op this is belt-and-suspenders: `HostOpCommand` already
        // catches a panicking handler internally and records a
        // `RecordActionFailure`; this whole-body catch covers a panic
        // in any OTHER part of any command's `run`.
        Err(crate::substrate::ProtocolCommandError::new(
            "ProtocolCommand panicked",
        ))
    });
    if let Err(e) = run_err {
        tracing::warn!(error = %e, "ProtocolCommand returned error");
    }
    // Drop the adapter borrows before the emit so `emit_now` can
    // re-borrow `ctx.kernel` mutably. The `kernel_cell` /
    // `identity_cell` `RefCell` borrows are released when the
    // adapters drop at end-of-block — explicitly drop the
    // adapters here so the `emit_now` below sees a fully
    // released `ctx.kernel`. The `RefCell` owners themselves are
    // moved at function end (no explicit `drop` needed once the
    // adapters that borrowed them are dropped).
    //
    // ADR-0052 §D5: `wallet_kernel` / `zap_profiles` also borrow
    // `kernel_cell`, so they too must drop before the `emit_now`
    // re-borrow.
    drop(zap_profiles);
    drop(wallet_kernel);
    drop(recipients);
    drop(stages);
    drop(errors);
    drop(signers);
    drop(clock);
    // V-41 + V-39+V-40 + V-38 — a `ProtocolCommand` body may have
    // mutated the kernel (the `Requested` stage write, a toast, a
    // recorded failure) or queued follow-up `ActorCommand`s
    // (`ShowToast` / `RecordActionFailure` / `PublishSignedEvent`).
    // Emit promptly so the next snapshot tick carries the visible
    // effect, mirroring the legacy `FetchLnurlInvoice` and
    // `SendGiftWrappedDm` arms' `emit_now` precedents.
    emit_now(ctx.kernel, *ctx.running, ctx.update_tx, ctx.last_emit);
    Some(outbound)
}
