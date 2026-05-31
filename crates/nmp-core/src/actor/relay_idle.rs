//! Temporary relay idle eviction.
//!
//! Persistent sockets are owned by user/app/session configuration and are only
//! closed by explicit lifecycle commands. This module only evicts on-demand
//! sockets after the kernel reports no active demand for their URL.

use crate::kernel::Kernel;
use crate::relay::CanonicalRelayUrl;
use nmp_network::pool::Pool;
use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use super::relay_mgmt::shutdown_relay_worker;
use super::{RelayConnectionKind, RelayControl};

pub(super) const TEMPORARY_RELAY_IDLE_GRACE: Duration = Duration::from_secs(60);

pub(super) fn sweep_temporary_idle_relays(
    relay_controls: &mut HashMap<CanonicalRelayUrl, RelayControl>,
    slot_to_url: &mut HashMap<u32, CanonicalRelayUrl>,
    connected_urls: &mut HashSet<CanonicalRelayUrl>,
    pool: &Pool,
    kernel: &mut Kernel,
    now: Instant,
    grace: Duration,
) {
    let mut to_close = Vec::new();
    for (url, control) in relay_controls.iter_mut() {
        if control.connection_kind == RelayConnectionKind::Persistent {
            control.idle_since = None;
            continue;
        }
        if kernel.relay_socket_is_persistent(url, control.role) {
            control.connection_kind = RelayConnectionKind::Persistent;
            control.idle_since = None;
            continue;
        }
        if kernel.relay_has_active_demand(url) {
            control.idle_since = None;
            continue;
        }

        match control.idle_since {
            None => control.idle_since = Some(now),
            Some(idle_since) if now.duration_since(idle_since) >= grace => {
                to_close.push(url.clone());
            }
            Some(_) => {}
        }
    }

    for url in to_close {
        if let Some(control) = relay_controls.get(&url) {
            let role = control.role;
            kernel.relay_closed(role, url.as_str());
            kernel.mark_publish_relay_unavailable(url.as_str());
            connected_urls.remove(&url);
        }
        shutdown_relay_worker(relay_controls, slot_to_url, pool, url.as_str());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actor::relay_mgmt::{close_relays, ensure_relay_worker_with_kind};
    use crate::relay::{RelayRole, DEFAULT_VISIBLE_LIMIT};
    use nmp_network::pool::{PoolConfig, PoolEvent};
    use serde_json::json;
    use std::collections::HashSet;
    use std::sync::mpsc;

    fn fresh_pool() -> (Pool, mpsc::Receiver<PoolEvent>) {
        let (events_tx, events_rx) = mpsc::channel::<PoolEvent>();
        (Pool::new(PoolConfig::default(), events_tx), events_rx)
    }

    fn snapshot(kernel: &mut Kernel) -> serde_json::Value {
        serde_json::from_str(&kernel.make_update_json_for_test(true)).expect("snapshot JSON")
    }

    fn diagnostic_connection(snapshot: &serde_json::Value, relay_url: &str) -> Option<String> {
        snapshot["projections"]["relay_diagnostics"]["relays"]
            .as_array()?
            .iter()
            .find(|row| row["relay_url"].as_str() == Some(relay_url))
            .and_then(|row| row["connection_label"].as_str())
            .map(str::to_string)
    }

    fn insert_control(
        relay_controls: &mut HashMap<CanonicalRelayUrl, RelayControl>,
        slot_to_url: &mut HashMap<u32, CanonicalRelayUrl>,
        pool: &Pool,
        kernel: &mut Kernel,
        url: &str,
        kind: RelayConnectionKind,
    ) {
        let mut next_generation = 1;
        assert!(ensure_relay_worker_with_kind(
            relay_controls,
            slot_to_url,
            pool,
            kernel,
            &mut next_generation,
            RelayRole::Content,
            url.to_string(),
            kind,
        ));
    }

    #[test]
    fn temporary_relay_closes_after_idle_grace() {
        let (pool, _rx) = fresh_pool();
        let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
        let mut relay_controls = HashMap::new();
        let mut slot_to_url = HashMap::new();
        let mut connected_urls = HashSet::new();
        let url = "ws://127.0.0.1:9";
        let key = CanonicalRelayUrl::parse_or_raw(url);
        insert_control(
            &mut relay_controls,
            &mut slot_to_url,
            &pool,
            &mut kernel,
            url,
            RelayConnectionKind::Temporary,
        );

        let now = Instant::now();
        sweep_temporary_idle_relays(
            &mut relay_controls,
            &mut slot_to_url,
            &mut connected_urls,
            &pool,
            &mut kernel,
            now,
            Duration::from_secs(10),
        );
        assert!(
            relay_controls.contains_key(&key),
            "first idle sweep only starts grace"
        );

        sweep_temporary_idle_relays(
            &mut relay_controls,
            &mut slot_to_url,
            &mut connected_urls,
            &pool,
            &mut kernel,
            now + Duration::from_secs(11),
            Duration::from_secs(10),
        );
        assert!(
            relay_controls.is_empty(),
            "temporary idle relay should close after grace"
        );
    }

    #[test]
    fn idle_close_updates_diagnostics_and_reconnect_bookkeeping() {
        let (pool, _rx) = fresh_pool();
        let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
        let mut relay_controls = HashMap::new();
        let mut slot_to_url = HashMap::new();
        let mut connected_urls = HashSet::new();
        let url = "ws://127.0.0.1:9";
        let key = CanonicalRelayUrl::parse_or_raw(url);
        insert_control(
            &mut relay_controls,
            &mut slot_to_url,
            &pool,
            &mut kernel,
            url,
            RelayConnectionKind::Temporary,
        );
        kernel.relay_connected_url(RelayRole::Content, key.as_str());
        connected_urls.insert(key.clone());
        assert_eq!(
            diagnostic_connection(&snapshot(&mut kernel), key.as_str()).as_deref(),
            Some("Connected")
        );

        let now = Instant::now();
        sweep_temporary_idle_relays(
            &mut relay_controls,
            &mut slot_to_url,
            &mut connected_urls,
            &pool,
            &mut kernel,
            now,
            Duration::from_secs(1),
        );
        sweep_temporary_idle_relays(
            &mut relay_controls,
            &mut slot_to_url,
            &mut connected_urls,
            &pool,
            &mut kernel,
            now + Duration::from_secs(2),
            Duration::from_secs(1),
        );

        assert_eq!(
            diagnostic_connection(&snapshot(&mut kernel), key.as_str()).as_deref(),
            Some("Closed"),
            "idle eviction must not leave diagnostics connected"
        );
        assert!(
            !connected_urls.contains(&key),
            "idle eviction must clear reconnect discriminator"
        );
        assert!(
            connected_urls.insert(key),
            "the next open of this URL must be treated as fresh, not reconnect"
        );
    }

    #[test]
    fn persistent_relay_is_not_idle_closed() {
        let (pool, _rx) = fresh_pool();
        let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
        let mut relay_controls = HashMap::new();
        let mut slot_to_url = HashMap::new();
        let mut connected_urls = HashSet::new();
        let url = "ws://127.0.0.1:9";
        let key = CanonicalRelayUrl::parse_or_raw(url);
        insert_control(
            &mut relay_controls,
            &mut slot_to_url,
            &pool,
            &mut kernel,
            url,
            RelayConnectionKind::Persistent,
        );

        let now = Instant::now();
        sweep_temporary_idle_relays(
            &mut relay_controls,
            &mut slot_to_url,
            &mut connected_urls,
            &pool,
            &mut kernel,
            now,
            Duration::from_secs(1),
        );
        sweep_temporary_idle_relays(
            &mut relay_controls,
            &mut slot_to_url,
            &mut connected_urls,
            &pool,
            &mut kernel,
            now + Duration::from_secs(2),
            Duration::from_secs(1),
        );
        assert!(
            relay_controls.contains_key(&key),
            "persistent relay must stay open"
        );

        close_relays(
            &mut relay_controls,
            &mut slot_to_url,
            &pool,
            &mut HashSet::new(),
            &mut kernel,
        );
    }

    #[test]
    fn active_wire_sub_prevents_temporary_idle_close() {
        let (pool, _rx) = fresh_pool();
        let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
        let mut relay_controls = HashMap::new();
        let mut slot_to_url = HashMap::new();
        let mut connected_urls = HashSet::new();
        let url = "ws://127.0.0.1:9";
        let key = CanonicalRelayUrl::parse_or_raw(url);
        insert_control(
            &mut relay_controls,
            &mut slot_to_url,
            &pool,
            &mut kernel,
            url,
            RelayConnectionKind::Temporary,
        );
        let _req = kernel.req_for_relay(
            RelayRole::Content,
            url.to_string(),
            "sub-temp",
            "temporary test",
            json!({ "kinds": [1] }),
        );

        let now = Instant::now();
        sweep_temporary_idle_relays(
            &mut relay_controls,
            &mut slot_to_url,
            &mut connected_urls,
            &pool,
            &mut kernel,
            now,
            Duration::from_secs(1),
        );
        sweep_temporary_idle_relays(
            &mut relay_controls,
            &mut slot_to_url,
            &mut connected_urls,
            &pool,
            &mut kernel,
            now + Duration::from_secs(2),
            Duration::from_secs(1),
        );
        assert!(
            relay_controls.contains_key(&key),
            "active demand must keep temp relay open"
        );

        close_relays(
            &mut relay_controls,
            &mut slot_to_url,
            &pool,
            &mut HashSet::new(),
            &mut kernel,
        );
    }

    #[test]
    fn configured_relay_promotes_temporary_socket_to_persistent() {
        let (pool, _rx) = fresh_pool();
        let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
        let mut relay_controls = HashMap::new();
        let mut slot_to_url = HashMap::new();
        let mut connected_urls = HashSet::new();
        let url = "ws://127.0.0.1:9";
        let key = CanonicalRelayUrl::parse_or_raw(url);
        insert_control(
            &mut relay_controls,
            &mut slot_to_url,
            &pool,
            &mut kernel,
            url,
            RelayConnectionKind::Temporary,
        );
        kernel.set_configured_relays(vec![crate::kernel::AppRelay::new(
            key.to_string(),
            "both".to_string(),
        )]);

        let now = Instant::now();
        sweep_temporary_idle_relays(
            &mut relay_controls,
            &mut slot_to_url,
            &mut connected_urls,
            &pool,
            &mut kernel,
            now,
            Duration::from_secs(1),
        );
        sweep_temporary_idle_relays(
            &mut relay_controls,
            &mut slot_to_url,
            &mut connected_urls,
            &pool,
            &mut kernel,
            now + Duration::from_secs(2),
            Duration::from_secs(1),
        );

        let control = relay_controls
            .get(&key)
            .expect("configured relay should stay open");
        assert_eq!(control.connection_kind, RelayConnectionKind::Persistent);

        close_relays(
            &mut relay_controls,
            &mut slot_to_url,
            &pool,
            &mut HashSet::new(),
            &mut kernel,
        );
    }
}
