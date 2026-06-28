//! Relay-inventory operations for `BrowserRuntimeHandle`.
//!
//! Runtime relay edits are transport control, while `publish_relay_preferences`
//! is an app write that this module lowers into the same typed `dispatch_bytes`
//! doorway used by the web shell. The shell triggers the verb; Rust owns the
//! current relay projection and NIP-65 payload construction.

use nmp_core::dispatch_envelope::{encode_dispatch_envelope, DISPATCH_ENVELOPE_SCHEMA_VERSION};
use nmp_core::substrate::ActionPayload;
use nmp_defaults::action_payloads::PublishRelayListInput;
use serde_json::json;

use super::handle::BrowserRuntimeHandle;
use super::kernel_ops::DispatchBytesResult;

#[derive(Debug)]
pub(crate) enum RelayConfigResult {
    Applied {
        action_type: String,
        correlation_id: String,
    },
    Rejected {
        capability: String,
        correlation_id: String,
        reason: String,
    },
}

#[derive(Debug)]
pub(crate) enum RelayConfigAction {
    Add,
    Remove,
}

impl BrowserRuntimeHandle {
    pub(crate) fn apply_relay_config(
        &mut self,
        action: RelayConfigAction,
        url: String,
        role: Option<String>,
        correlation_id: &str,
    ) -> RelayConfigResult {
        let capability = "nmp.relay_config".to_string();
        let Some(canonical_url) = nmp_core::canonical_relay_url(&url) else {
            return RelayConfigResult::Rejected {
                capability,
                correlation_id: correlation_id.to_string(),
                reason: "invalid relay URL — expected ws:// or wss://".to_string(),
            };
        };

        let mut rows = self.configured_relay_rows();
        match action {
            RelayConfigAction::Add => {
                let raw_role = role.unwrap_or_else(|| "both".to_string());
                let Some(role) = normalize_relay_role(&raw_role) else {
                    return RelayConfigResult::Rejected {
                        capability,
                        correlation_id: correlation_id.to_string(),
                        reason: "invalid relay role — expected read, write, both, indexer, or a composite role".to_string(),
                    };
                };
                if let Some(existing) = rows.iter_mut().find(|(u, _)| *u == canonical_url) {
                    existing.1 = role.clone();
                } else {
                    rows.push((canonical_url.clone(), role.clone()));
                }
                self.runtime
                    .relay_pool
                    .spawn_configured_relay(&canonical_url, &role)
                    .into_iter()
                    .for_each(|event| self.runtime.pending_startup_events.push(event));
            }
            RelayConfigAction::Remove => {
                rows.retain(|(u, _)| u != &canonical_url);
                self.runtime.relay_pool.close_relay(&canonical_url);
            }
        }

        self.runtime.reducer.set_configured_relays(rows);
        self.notify_configured_relays_changed();
        let outbound = self
            .runtime
            .relay_pool
            .tick_and_arm(&mut self.runtime.reducer, nmp_core::time::Instant::now());
        self.fan_out_outbound(outbound);

        RelayConfigResult::Applied {
            action_type: "nmp.relay_config".to_string(),
            correlation_id: correlation_id.to_string(),
        }
    }

    pub(crate) fn publish_relay_preferences(
        &mut self,
        correlation_id: &str,
    ) -> DispatchBytesResult {
        let entries: Vec<_> = self
            .configured_relay_rows()
            .into_iter()
            .map(|(url, role)| json!({ "url": url, "role": role }))
            .collect();
        let input: PublishRelayListInput =
            match serde_json::from_value(json!({ "relays": entries })) {
                Ok(input) => input,
                Err(err) => {
                    return DispatchBytesResult::Rejected {
                        capability: PublishRelayListInput::SCHEMA_ID.to_string(),
                        correlation_id: correlation_id.to_string(),
                        reason: format!(
                            "configured relay projection did not match NIP-65 payload: {err}"
                        ),
                    };
                }
            };
        let payload = input.encode();
        let envelope = encode_dispatch_envelope(
            correlation_id,
            PublishRelayListInput::SCHEMA_ID,
            DISPATCH_ENVELOPE_SCHEMA_VERSION,
            &payload,
        );
        self.apply_dispatch_bytes(&envelope)
    }

    /// Merge identity-provided relay URLs into the kernel's configured relay list
    /// and apply the result (#2139 HIGH 4).
    pub(crate) fn apply_identity_relays(&mut self, new_rows: Vec<(String, String)>) {
        if new_rows.is_empty() {
            return;
        }
        let mut merged = self.configured_relay_rows();
        let mut changed = false;
        for (url, role) in new_rows {
            if let Some(existing) = merged.iter_mut().find(|(eu, _)| eu == &url) {
                let merged_role = merge_relay_roles(&existing.1, &role);
                if merged_role != existing.1 {
                    existing.1 = merged_role;
                    changed = true;
                }
            } else {
                merged.push((url, role));
                changed = true;
            }
        }

        if changed {
            self.runtime.reducer.set_configured_relays(merged);
            self.notify_configured_relays_changed();
        }
    }

    fn notify_configured_relays_changed(&self) {
        for observer in &self.runtime.configured_relays_change_observers {
            observer();
        }
    }

    fn configured_relay_rows(&self) -> Vec<(String, String)> {
        self.configured_relays
            .lock()
            .map(|g| {
                g.as_slice()
                    .iter()
                    .map(|r| (r.url().to_string(), r.role().to_string()))
                    .collect()
            })
            .unwrap_or_default()
    }
}

fn merge_relay_roles(existing: &str, incoming: &str) -> String {
    let (er, ew, ei) = parse_role_flags(existing);
    let (ir, iw, ii) = parse_role_flags(incoming);
    let r = er || ir;
    let w = ew || iw;
    let i = ei || ii;
    let mut parts: Vec<&str> = Vec::new();
    match (r, w) {
        (true, true) => parts.push("both"),
        (true, false) => parts.push("read"),
        (false, true) => parts.push("write"),
        (false, false) => {}
    }
    if i {
        parts.push("indexer");
    }
    if parts.is_empty() {
        existing.to_string()
    } else {
        parts.join(",")
    }
}

fn parse_role_flags(role: &str) -> (bool, bool, bool) {
    let mut read = false;
    let mut write = false;
    let mut indexer = false;
    for part in role.split(',') {
        match part.trim() {
            "read" => read = true,
            "write" => write = true,
            "both" => {
                read = true;
                write = true;
            }
            "indexer" => indexer = true,
            _ => {}
        }
    }
    (read, write, indexer)
}

fn normalize_relay_role(role: &str) -> Option<String> {
    let read = nmp_core::actor::has_role(role, "read");
    let write = nmp_core::actor::has_role(role, "write");
    let indexer = nmp_core::actor::has_role(role, "indexer");
    if !read && !write && !indexer {
        return None;
    }
    let mut parts = Vec::new();
    match (read, write) {
        (true, true) => parts.push("both"),
        (true, false) => parts.push("read"),
        (false, true) => parts.push("write"),
        (false, false) => {}
    }
    if indexer {
        parts.push("indexer");
    }
    Some(parts.join(","))
}
