//! Browser NIP-46 bunker signer bridge.
//!
//! The shared `nmp-nip46-runtime` reducer owns the protocol state and relay
//! transport. This module adapts its effects to the browser runtime: outbound
//! frames stay on the browser relay pool, completed signers are installed in
//! the browser capability registry, and decrypted RPC responses are delivered
//! back to the same `Nip46Signer` instance that parked the request.

use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;

use nmp_core::substrate::{RelayConnectedHook, RelayTextInterceptor};
use nmp_core::time::{SystemTime, UNIX_EPOCH};
use nmp_core::{CommandSender, Kernel, OutboundMessage};
use nmp_network::role::RelayRole;
use nmp_nip46::Effect;
use nmp_nip46_runtime::{
    complete_signer_from_ready, init_bunker, make_sub_id, mark_persistent_sub_registered,
    new_nip46_runtime_handle, take_persistent_registration, Nip46RuntimeHandle,
};
use nmp_signers::{parse_bunker_uri, Nip46Signer, PublicKey};
use nostr::Keys;

use super::PendingSignerCompletions;

pub(crate) enum BrowserNip46Event {
    Progress {
        stage: String,
        code: Option<String>,
        detail: Option<String>,
    },
    Failed {
        reason: String,
    },
    InstallSigner {
        signer: Arc<Nip46Signer>,
    },
    SignerResponse {
        response_json: String,
    },
}

/// Browser-owned NIP-46 state.
pub(crate) struct BrowserNip46Runtime {
    handle: Nip46RuntimeHandle,
    events_tx: Sender<BrowserNip46Event>,
    events_rx: Receiver<BrowserNip46Event>,
    remote_signers: HashMap<String, Arc<Nip46Signer>>,
    pub(crate) pending_signs: PendingSignerCompletions,
}

impl BrowserNip46Runtime {
    pub(crate) fn install(
        interceptors: &mut Vec<Arc<dyn RelayTextInterceptor>>,
        hooks: &mut Vec<Arc<dyn RelayConnectedHook>>,
        sender: CommandSender,
    ) -> Self {
        let handle = new_nip46_runtime_handle();
        let (events_tx, events_rx) = mpsc::channel();
        interceptors.push(Arc::new(BrowserNip46Interceptor {
            handle: Arc::clone(&handle),
            sender: sender.clone(),
            events_tx: events_tx.clone(),
        }));
        hooks.push(Arc::new(BrowserNip46ConnectedHook {
            handle: Arc::clone(&handle),
            events_tx: events_tx.clone(),
        }));
        Self {
            handle,
            events_tx,
            events_rx,
            remote_signers: HashMap::new(),
            pending_signs: PendingSignerCompletions::new(),
        }
    }

    pub(crate) fn start_bunker(
        &self,
        bunker_uri: &str,
        now_secs: u64,
    ) -> Result<Vec<OutboundMessage>, String> {
        let parsed = parse_bunker_uri(bunker_uri)
            .map_err(|err| format!("invalid_nip46_bunker_uri: {err}"))?;
        let remote_pubkey = PublicKey::from_hex(&parsed.remote_pubkey_hex)
            .map_err(|err| format!("invalid_nip46_bunker_pubkey: {err}"))?;
        let local_keys = Keys::generate();
        let sub_id = make_sub_id(local_keys.public_key());
        let effects = init_bunker(
            &self.handle,
            sub_id,
            local_keys,
            remote_pubkey,
            parsed.relays,
            parsed.secret.as_ref().map(|secret| secret.as_str()),
            parsed.permissions.as_deref(),
            now_secs,
        )?;
        Ok(translate_start_effects(effects, &self.events_tx))
    }

    pub(crate) fn remember_signer(&mut self, signer: Arc<Nip46Signer>) {
        self.remote_signers
            .insert(signer.remote_user_pubkey().to_hex(), signer);
    }

    pub(crate) fn deliver_response_to_signers(&self, response_json: &str) {
        for signer in self.remote_signers.values() {
            signer.ingest_rpc_response(response_json);
        }
    }

    pub(crate) fn drain_events(&mut self) -> Vec<BrowserNip46Event> {
        let mut events = Vec::new();
        while let Ok(event) = self.events_rx.try_recv() {
            events.push(event);
        }
        events
    }

    pub(crate) fn drain_ready_signs(&mut self) -> Vec<super::SignerCompletion> {
        self.pending_signs.drain_ready()
    }

    #[cfg(test)]
    pub(crate) fn push_event_for_test(&self, event: BrowserNip46Event) {
        let _ = self.events_tx.send(event);
    }
}

struct BrowserNip46Interceptor {
    handle: Nip46RuntimeHandle,
    sender: CommandSender,
    events_tx: Sender<BrowserNip46Event>,
}

impl RelayTextInterceptor for BrowserNip46Interceptor {
    fn on_relay_text(
        &self,
        kernel: &mut Kernel,
        relay_url: &str,
        text: &str,
    ) -> Vec<OutboundMessage> {
        let now = kernel.now_secs();
        let (effects, decoded) = {
            let Ok(mut guard) = self.handle.lock() else {
                return Vec::new();
            };
            let Some(runtime) = guard.as_mut() else {
                return Vec::new();
            };
            runtime.on_relay_text(relay_url, text, now)
        };
        if let Some(response_json) = decoded {
            let _ = self
                .events_tx
                .send(BrowserNip46Event::SignerResponse { response_json });
        }
        self.translate_effects(effects, kernel)
    }

    fn on_idle_tick(&self, kernel: &mut Kernel) -> Vec<OutboundMessage> {
        let now = kernel.now_secs();
        let registration = take_persistent_registration(&self.handle);
        let effects = {
            let Ok(mut guard) = self.handle.lock() else {
                return Vec::new();
            };
            let Some(runtime) = guard.as_mut() else {
                return Vec::new();
            };
            runtime.tick(now)
        };
        if let Some((relay_urls, sub_id)) = registration {
            for relay_url in relay_urls {
                kernel.register_persistent_sub(relay_url, sub_id.clone());
            }
        }
        self.translate_effects(effects, kernel)
    }
}

impl BrowserNip46Interceptor {
    fn translate_effects(&self, effects: Vec<Effect>, kernel: &mut Kernel) -> Vec<OutboundMessage> {
        let mut outbound = Vec::new();
        for effect in effects {
            match effect {
                Effect::Subscribe { relay_url, frame } => {
                    if let Some(sub_id) = extract_sub_id(&frame) {
                        kernel.register_persistent_sub(relay_url.clone(), sub_id);
                        mark_persistent_sub_registered(&self.handle);
                    }
                    outbound.push(OutboundMessage::new(RelayRole::Signer, relay_url, frame));
                }
                Effect::SendFrame { relay_url, text } => {
                    outbound.push(OutboundMessage::new(RelayRole::Signer, relay_url, text));
                }
                Effect::Progress {
                    stage,
                    code,
                    detail,
                } => {
                    let _ = self.events_tx.send(BrowserNip46Event::Progress {
                        stage,
                        code,
                        detail,
                    });
                }
                Effect::SignerReady(ready) => {
                    match complete_signer_from_ready(&self.handle, ready, self.sender.clone()) {
                        Ok(signer) => {
                            let _ = self.events_tx.send(BrowserNip46Event::Progress {
                                stage: "ready".to_string(),
                                code: None,
                                detail: Some("NIP-46 signer ready".to_string()),
                            });
                            let _ = self.events_tx.send(BrowserNip46Event::InstallSigner {
                                signer: Arc::new(signer),
                            });
                        }
                        Err(reason) => {
                            let _ = self.events_tx.send(BrowserNip46Event::Failed { reason });
                        }
                    }
                }
                Effect::DeliverResponse { result, .. } => {
                    let _ = self.events_tx.send(BrowserNip46Event::SignerResponse {
                        response_json: result,
                    });
                }
                Effect::Error { error } => {
                    let _ = self.events_tx.send(BrowserNip46Event::Failed {
                        reason: error.to_string(),
                    });
                }
            }
        }
        outbound
    }
}

struct BrowserNip46ConnectedHook {
    handle: Nip46RuntimeHandle,
    events_tx: Sender<BrowserNip46Event>,
}

impl RelayConnectedHook for BrowserNip46ConnectedHook {
    fn on_relay_connected(
        &self,
        relay_url: &str,
        is_reconnect: bool,
        command_sender: CommandSender,
    ) {
        let effects = {
            let Ok(mut guard) = self.handle.lock() else {
                return;
            };
            let Some(runtime) = guard.as_mut() else {
                return;
            };
            runtime.on_relay_connected(relay_url, is_reconnect, now_secs())
        };
        for effect in effects {
            match effect {
                Effect::Subscribe { relay_url, frame } => {
                    command_sender.enqueue_outbound(RelayRole::Signer, relay_url, frame);
                }
                Effect::SendFrame { relay_url, text } => {
                    command_sender.enqueue_outbound(RelayRole::Signer, relay_url, text);
                }
                Effect::Progress {
                    stage,
                    code,
                    detail,
                } => {
                    let _ = self.events_tx.send(BrowserNip46Event::Progress {
                        stage,
                        code,
                        detail,
                    });
                }
                Effect::Error { error } => {
                    let _ = self.events_tx.send(BrowserNip46Event::Failed {
                        reason: error.to_string(),
                    });
                }
                Effect::SignerReady(_) | Effect::DeliverResponse { .. } => {}
            }
        }
    }
}

fn translate_start_effects(
    effects: Vec<Effect>,
    events_tx: &Sender<BrowserNip46Event>,
) -> Vec<OutboundMessage> {
    let mut outbound = Vec::new();
    for effect in effects {
        match effect {
            Effect::Subscribe { relay_url, frame } => {
                outbound.push(OutboundMessage::new(RelayRole::Signer, relay_url, frame));
            }
            Effect::SendFrame { relay_url, text } => {
                outbound.push(OutboundMessage::new(RelayRole::Signer, relay_url, text));
            }
            Effect::Progress {
                stage,
                code,
                detail,
            } => {
                let _ = events_tx.send(BrowserNip46Event::Progress {
                    stage,
                    code,
                    detail,
                });
            }
            Effect::Error { error } => {
                let _ = events_tx.send(BrowserNip46Event::Failed {
                    reason: error.to_string(),
                });
            }
            Effect::SignerReady(_) | Effect::DeliverResponse { .. } => {}
        }
    }
    outbound
}

fn extract_sub_id(frame: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(frame).ok()?;
    let arr = value.as_array()?;
    if arr.first()?.as_str()? != "REQ" {
        return None;
    }
    arr.get(1)?.as_str().map(str::to_string)
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}
