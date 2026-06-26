//! `ActorCommand::Relay(RelayCommand::ReconnectRelays)` transport half (#1689).
//!
//! A kernel-driven "reconnect all": host apps (network-change, app-foreground,
//! a Settings "Reconnect" button) drive it through the actor bus so the kernel
//! stays the sole driver of transport rather than reaching into the relay pool
//! directly. Split out of `relay_mgmt` to keep that file under its file-size
//! cap.

use crate::kernel::Kernel;
use nmp_network::pool::Pool;

use super::relay_runtime::RelayRuntime;

/// Re-attempt connection on every relay worker the actor tracks.
///
/// For each URL in `relay_controls`, re-call [`Pool::ensure_open_with_role`]:
///
/// * **Connected / connecting** sockets: `ensure_open` returns the live handle
///   unchanged — idempotent no-op (generation does not move).
/// * **Closed / errored** sockets (a permanent 401/403 close, or any slot the
///   pool tore down): `ensure_open` reopens the slot in place with a bumped
///   generation. The fresh handle is adopted and the kernel's per-URL diagnostic
///   row is re-stamped to `connecting`. The slot id is stable across a reopen,
///   so `slot_to_url` needs no update.
///
/// Fail-closed: the dead-handle sentinel (`u32::MAX` slot) the pool returns for
/// a non-canonicalizable URL or a post-shutdown pool is never adopted over a
/// live handle — the URL is never dialed under a malformed key (mirrors the
/// `ensure_open` #967 guard). The dispatch arm additionally gates this on the
/// actor being `running`.
///
/// Returns the number of sockets whose slot was reopened (errored → re-dialing).
pub(super) fn reconnect_relays(rt: &mut RelayRuntime, pool: &Pool, kernel: &mut Kernel) -> usize {
    let mut reopened = 0_usize;
    for control in rt.relay_controls.values_mut() {
        let role = control.role;
        let url = control.relay_url.clone();
        let handle = pool.ensure_open_with_role(&url, role);
        // Fail-closed: never overwrite a live handle with the dead-handle sentinel.
        if handle.slot() == u32::MAX {
            continue;
        }
        if handle.generation() != control.handle.generation() {
            // Slot reopened: a downed socket is now re-dialing.
            control.handle = handle;
            control.idle_since = None;
            kernel.relay_connecting_url(role, &url);
            reopened += 1;
        }
    }
    reopened
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actor::relay_mgmt::ensure_relay_worker;
    use crate::relay::CanonicalRelayUrl;
    use nmp_network::pool::{Pool, PoolConfig, PoolEvent};
    use nmp_network::role::RelayRole;

    /// #1689 — `reconnect_relays` re-dials a downed relay worker (the
    /// load-bearing proof that `ActorCommand::Relay(RelayCommand::ReconnectRelays)` triggers a
    /// reconnect). Spawn a worker, `Pool::close` its slot (simulating a
    /// disconnect / permanent error), then assert `reconnect_relays` reopens it:
    /// the pool generation bumps, the slot id is stable, exactly one reopen is
    /// reported, and a second call is an idempotent no-op. No live server is
    /// needed — the reconnect is a synchronous decision over pool slot state.
    #[test]
    fn reconnect_relays_redials_downed_worker() {
        let (events_tx, _events_rx) = std::sync::mpsc::channel::<PoolEvent>();
        let pool = Pool::new(PoolConfig::default(), events_tx);
        let mut kernel = Kernel::new(80);
        let mut rt = RelayRuntime::new();
        let url = "wss://127.0.0.1:1".to_string();

        let spawned =
            ensure_relay_worker(&mut rt, &pool, &mut kernel, RelayRole::Content, url.clone());
        assert!(spawned, "first ensure_relay_worker call must spawn");

        let key = CanonicalRelayUrl::parse_or_raw(&url);
        let before = rt.relay_controls.get(&key).expect("control exists").handle;
        assert!(pool.close(before), "closing the live handle must succeed");

        let reopened = reconnect_relays(&mut rt, &pool, &mut kernel);
        assert_eq!(reopened, 1, "#1689: must re-dial the one downed socket");

        let after = rt
            .relay_controls
            .get(&key)
            .expect("control still exists")
            .handle;
        assert_eq!(
            after.slot(),
            before.slot(),
            "#1689: reopen reuses the slot id (one socket per URL)"
        );
        assert!(
            after.generation() > before.generation(),
            "#1689: reopen bumps the generation (before={}, after={})",
            before.generation(),
            after.generation(),
        );

        // A second reconnect over the now-live (Connecting) slot is a no-op.
        let again = reconnect_relays(&mut rt, &pool, &mut kernel);
        assert_eq!(again, 0, "#1689: reconnect over a live socket is a no-op");
    }

    use crate::actor::commands::{self, IdentityRuntime};
    use crate::actor::signer_port_test_harness::dispatch_one_with_relays;
    use crate::actor::ActorCommand;
    use crate::actor::RelayCommand;

    fn fresh_identity() -> IdentityRuntime {
        IdentityRuntime::new(
            commands::new_bunker_handshake_slot(),
            commands::new_signer_state_slot(),
        )
    }

    /// Seed one relay worker for `url`, close its pool slot (down state), and
    /// return the harness state (pool + kernel + runtime) plus the pre-reconnect
    /// handle generation.
    fn seed_downed_relay(url: &str) -> (Pool, Kernel, RelayRuntime, u64) {
        let (events_tx, _events_rx) = std::sync::mpsc::channel::<PoolEvent>();
        let pool = Pool::new(PoolConfig::default(), events_tx);
        let mut kernel = Kernel::new(80);
        let mut rt = RelayRuntime::new();
        ensure_relay_worker(
            &mut rt,
            &pool,
            &mut kernel,
            RelayRole::Content,
            url.to_string(),
        );
        let key = CanonicalRelayUrl::parse_or_raw(url);
        let before = rt.relay_controls.get(&key).expect("control").handle;
        assert!(pool.close(before), "close the live handle");
        (pool, kernel, rt, before.generation())
    }

    /// #1689 — the `ActorCommand::Relay(RelayCommand::ReconnectRelays)` DISPATCH ARM re-dials a downed
    /// relay. This proves the command is routed through the actor command bus
    /// (`dispatch_command`), not merely that the helper works: if the arm were
    /// removed or broken, the generation would not bump and this fails.
    #[test]
    fn reconnect_relays_command_dispatches_and_redials() {
        let url = "wss://127.0.0.1:1";
        let (pool, mut kernel, mut rt, gen_before) = seed_downed_relay(url);
        let mut identity = fresh_identity();

        dispatch_one_with_relays(
            ActorCommand::Relay(RelayCommand::ReconnectRelays),
            &mut identity,
            &mut kernel,
            &pool,
            &mut rt,
            true, // running
        );

        let key = CanonicalRelayUrl::parse_or_raw(url);
        let gen_after = rt
            .relay_controls
            .get(&key)
            .expect("control")
            .handle
            .generation();
        assert!(
            gen_after > gen_before,
            "#1689: dispatching ReconnectRelays must re-dial the downed socket \
             (generation {gen_before} → {gen_after})"
        );
    }

    /// #1689 fail-closed — `ReconnectRelays` dispatched while the actor is NOT
    /// running must be a no-op (no re-dial of unconsented relays before `Start`).
    #[test]
    fn reconnect_relays_command_is_noop_before_start() {
        let url = "wss://127.0.0.1:1";
        let (pool, mut kernel, mut rt, gen_before) = seed_downed_relay(url);
        let mut identity = fresh_identity();

        dispatch_one_with_relays(
            ActorCommand::Relay(RelayCommand::ReconnectRelays),
            &mut identity,
            &mut kernel,
            &pool,
            &mut rt,
            false, // NOT running — fail-closed
        );

        let key = CanonicalRelayUrl::parse_or_raw(url);
        let gen_after = rt
            .relay_controls
            .get(&key)
            .expect("control")
            .handle
            .generation();
        assert_eq!(
            gen_after, gen_before,
            "#1689 fail-closed: ReconnectRelays before Start must not re-dial"
        );
    }
}
