//! T130 — pre-connect frames buffer on the worker until Open.
//!
//! T130 invariant: when the kernel sends a frame to a worker whose socket
//! is not yet open, the worker's internal `pending: VecDeque<String>`
//! holds the frame until `flush_relay_writes` runs post-Connected. This is
//! the "buffered waiting for that URL" mechanism — implemented at the
//! worker layer, not via a kernel-side per-URL gate.
use std::io::ErrorKind;
use std::net::TcpListener;
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use super::super::{spawn_relay_worker_with_keepalive, RelayCommand, RelayEvent};
use super::support::drain_until;
use crate::role::RelayRole;

#[test]
fn t130_frames_sent_before_connect_arrive_after_open() {
    // We force the race by binding a local server but accepting connections
    // SLOWLY: the listener thread sleeps before calling `accept()`, while
    // the worker is already dialing. We send a sentinel frame to the worker's
    // control channel during that window — it MUST land on the wire once the
    // server finishes its handshake.
    use std::io::{Read, Write};
    use tungstenite::handshake::derive_accept_key;

    let listener = TcpListener::bind("127.0.0.1:0").expect("bind");
    let port = listener.local_addr().unwrap().port();
    let url = format!("ws://127.0.0.1:{port}");

    // Server: accept after a 300ms delay so the worker has time to dial and
    // queue a frame before the handshake completes. Once connected, log every
    // text frame into a channel for the test to inspect.
    let (server_observed_tx, server_observed_rx) = mpsc::channel::<String>();
    let _server_thread = thread::spawn(move || {
        listener.set_nonblocking(false).ok();
        // Stall before accepting: the worker is racing to dial; the frame the
        // test sends to its control channel lands in the worker's pending
        // queue while there's no socket yet.
        thread::sleep(Duration::from_millis(300));
        let (mut stream, _) = match listener.accept() {
            Ok(s) => s,
            Err(_) => return,
        };
        stream
            .set_read_timeout(Some(Duration::from_millis(50)))
            .ok();
        // Hand-rolled HTTP→WS upgrade (so the test does not depend on the
        // exact tungstenite server-side accept loop).
        let mut req = Vec::with_capacity(2048);
        let mut tmp = [0u8; 1024];
        let deadline = Instant::now() + Duration::from_millis(500);
        while Instant::now() < deadline && !req.windows(4).any(|w| w == b"\r\n\r\n") {
            match stream.read(&mut tmp) {
                Ok(0) => return,
                Ok(n) => req.extend_from_slice(&tmp[..n]),
                Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {}
                Err(_) => return,
            }
        }
        let req_str = String::from_utf8_lossy(&req).to_string();
        let key = req_str
            .lines()
            .find(|l| l.to_ascii_lowercase().starts_with("sec-websocket-key:"))
            .and_then(|l| l.split(':').nth(1))
            .map(str::trim)
            .unwrap_or("")
            .to_string();
        let accept_val = derive_accept_key(key.as_bytes());
        let response = format!(
            "HTTP/1.1 101 Switching Protocols\r\n\
             Upgrade: websocket\r\n\
             Connection: Upgrade\r\n\
             Sec-WebSocket-Accept: {accept_val}\r\n\r\n",
        );
        if stream.write_all(response.as_bytes()).is_err() {
            return;
        }
        // Now parse WS frames manually. We only care about Text frames.
        // tungstenite::WebSocket::from_raw_socket would also work, but
        // hand-rolling keeps this test free of unstable internals.
        // Decode one or more frames; loop until we see the sentinel or time out.
        let mut buf = Vec::with_capacity(2048);
        let frame_deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < frame_deadline {
            let mut chunk = [0u8; 256];
            match stream.read(&mut chunk) {
                Ok(0) => return,
                Ok(n) => buf.extend_from_slice(&chunk[..n]),
                Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                    continue;
                }
                Err(_) => return,
            }
            // Try to decode one frame. Per RFC 6455, client→server frames are
            // masked and start with FIN+opcode byte. We just want to extract
            // the payload of any Text frame (opcode 0x1).
            if let Some(text) = decode_first_text_frame(&buf) {
                let _ = server_observed_tx.send(text);
                return;
            }
        }
    });

    // Worker dials immediately (the listener is bound, so `connect` returns
    // either after the 300ms server-side stall or as soon as the kernel
    // accepts the TCP — depends on platform; on macOS the listen backlog
    // accepts TCP first, then the WS handshake stalls).
    let (relay_tx, relay_rx) = mpsc::channel::<RelayEvent>();
    let control_tx = spawn_relay_worker_with_keepalive(
        RelayRole::Content,
        url.clone(),
        1,
        relay_tx,
        Duration::from_secs(60), // long idle so keepalive doesn't interfere
        Duration::from_secs(60),
        None,
    );

    // CRITICAL: send the frame BEFORE Connected arrives. The worker's outer
    // loop reads from `control_rx` before it has a socket; on first dial it
    // calls `open_relay_socket` first, but any RelayCommand::Send queued
    // during the dial sits in the worker thread's mpsc channel and gets
    // pumped on the next iteration of run_connected_relay (post-Connected).
    let sentinel = "[\"REQ\",\"t130-sentinel\",{\"kinds\":[1]}]".to_string();
    control_tx
        .send(RelayCommand::Send(sentinel.clone()))
        .expect("worker control channel must accept the pre-connect frame");

    // Wait for the server to observe the sentinel. Budget includes the 300ms
    // server stall + handshake + frame round-trip.
    let observed = server_observed_rx
        .recv_timeout(Duration::from_secs(3))
        .expect("server did not observe the pre-connect frame within 3s");
    assert_eq!(
        observed, sentinel,
        "the pre-connect frame must land on the wire post-Open, with the same bytes"
    );

    // Sanity: the worker really did emit Connected during the test (proves
    // the race actually happened — frame went out AFTER socket open).
    let connected = drain_until(
        &relay_rx,
        |ev| matches!(ev, RelayEvent::Connected { .. }),
        Duration::from_millis(500),
    );
    assert!(connected.is_some(), "worker must report Connected");
}

/// Minimal RFC-6455 frame decoder: returns the first complete Text frame's
/// payload, or `None` if the buffer doesn't yet contain one. Client→server
/// frames are masked; we apply the mask before returning.
fn decode_first_text_frame(buf: &[u8]) -> Option<String> {
    if buf.len() < 2 {
        return None;
    }
    let b0 = buf[0];
    let opcode = b0 & 0x0F;
    if opcode != 0x1 {
        // Not a text frame; we don't bother decoding (test only sends text).
        return None;
    }
    let b1 = buf[1];
    let masked = (b1 & 0x80) != 0;
    let mut len = (b1 & 0x7F) as usize;
    let mut idx = 2;
    if len == 126 {
        if buf.len() < idx + 2 {
            return None;
        }
        len = u16::from_be_bytes([buf[idx], buf[idx + 1]]) as usize;
        idx += 2;
    } else if len == 127 {
        // 64-bit length — not expected for the small sentinel; bail.
        return None;
    }
    let mask = if masked {
        if buf.len() < idx + 4 {
            return None;
        }
        let m = [buf[idx], buf[idx + 1], buf[idx + 2], buf[idx + 3]];
        idx += 4;
        Some(m)
    } else {
        None
    };
    if buf.len() < idx + len {
        return None;
    }
    let payload = &buf[idx..idx + len];
    let decoded: Vec<u8> = if let Some(m) = mask {
        payload
            .iter()
            .enumerate()
            .map(|(i, &b)| b ^ m[i % 4])
            .collect()
    } else {
        payload.to_vec()
    };
    String::from_utf8(decoded).ok()
}
