use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use nmp_core::actor::ActorCommand;
use nmp_core::{ActorMail, CommandSender};
use nmp_network::pool::{Pool, PoolEvent, RelayFrame, RelayHandle, WireFrame};
use nmp_nip46::Effect;
use nmp_nip46_runtime::Nip46RuntimeHandle;

#[allow(clippy::too_many_arguments)]
pub(super) fn pump_loop(
    pool: Arc<Pool>,
    pool_rx: std::sync::mpsc::Receiver<PoolEvent>,
    internal_rx: std::sync::mpsc::Receiver<ActorMail>,
    runtime: Nip46RuntimeHandle,
    url_to_handle: Arc<Mutex<HashMap<String, RelayHandle>>>,
    handle_to_url: Arc<Mutex<HashMap<RelayHandle, String>>>,
    external_tx: CommandSender,
    internal_tx: CommandSender,
) {
    loop {
        let tick = Duration::from_millis(10);

        // 1. Pool events (Opened, Frame).
        while let Ok(event) = pool_rx.recv_timeout(tick) {
            match event {
                PoolEvent::Opened { h, url, .. } => {
                    // Track handle ↔ url mapping so inbound frames can be
                    // dispatched to the runtime with the correct relay URL.
                    handle_to_url.lock().unwrap().insert(h, url.clone());
                    url_to_handle.lock().unwrap().insert(url, h);
                }
                PoolEvent::Frame {
                    h,
                    frame: RelayFrame::Text(text),
                    ..
                } => {
                    let relay_url = { handle_to_url.lock().unwrap().get(&h).cloned() };
                    let Some(relay_url) = relay_url else { continue };

                    let now = super::now_unix_secs();
                    let (effects, body) = {
                        let mut guard = runtime.lock().expect("runtime lock");
                        let Some(rt) = guard.as_mut() else { continue };
                        rt.on_relay_text(&relay_url, &text, now)
                    };

                    // Route handshake effects.
                    for effect in effects {
                        match effect {
                            Effect::SignerReady(ready) => {
                                super::handle_signer_ready(
                                    ready,
                                    &runtime,
                                    &internal_tx,
                                    &external_tx,
                                );
                            }
                            Effect::SendFrame {
                                relay_url: rurl,
                                text: frame_text,
                            } => {
                                let handles = url_to_handle.lock().unwrap();
                                if let Some(&fh) = handles.get(&rurl) {
                                    pool.send(fh, WireFrame::Text(frame_text));
                                }
                            }
                            Effect::Progress {
                                stage,
                                code,
                                detail,
                            } => {
                                external_tx.bunker_handshake_progress(stage, code, detail);
                            }
                            Effect::Error { error } => {
                                // Terminal handshake error -> surface as "failed" progress
                                // (matches interceptor::translate_effects Error arm).
                                external_tx.bunker_handshake_progress(
                                    "failed".to_string(),
                                    None,
                                    Some(error.to_string()),
                                );
                                external_tx.bunker_connection_state_changed(
                                    "failed".to_string(),
                                    Some(error.to_string()),
                                );
                            }
                            _ => {}
                        }
                    }

                    // Route steady-state delivery body.
                    if let Some(body) = body {
                        external_tx.deliver_signer_response(body);
                    }
                }
                _ => {}
            }
        }

        // 2. Internal actor commands (EnqueueOutbound from ActorLaneTransport).
        while let Ok(mail) = internal_rx.recv_timeout(tick) {
            if let ActorMail::Command(ActorCommand::EnqueueOutbound {
                relay_url, text, ..
            }) = mail
            {
                let h = url_to_handle.lock().unwrap().get(&relay_url).copied();
                if let Some(h) = h {
                    pool.send(h, WireFrame::Text(text));
                }
            }
        }
    }
}
