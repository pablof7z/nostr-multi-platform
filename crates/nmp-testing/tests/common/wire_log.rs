//! Stderr-capture helper for `NMP_CLAIM_LOG` wire-log assertions.
//!
//! # Overview
//!
//! [`StderrCapture`] redirects file descriptor 2 (stderr) to an OS pipe for
//! the duration of its lifetime.  Any code that writes to stderr via the
//! usual `eprintln!` / `write!(std::io::stderr(), ...)` path — including the
//! actor thread spawned by `nmp_core::testing::spawn_actor()` — will write
//! into the pipe's read end, which [`StderrCapture::collect`] drains.
//!
//! # Usage
//!
//! ```ignore
//! // MUST be first: set NMP_CLAIM_LOG before spawn_actor() is called so
//! // claim_log_enabled()'s OnceLock initialises with `true`.
//! unsafe { std::env::set_var("NMP_CLAIM_LOG", "1") };
//!
//! let cap = StderrCapture::start();
//! // ... run actor + claim ...
//! let lines = cap.collect();
//! let req_emits: Vec<_> = wire_log_events(&lines, "ReqEmit").collect();
//! ```
//!
//! # Caveats
//!
//! - Thread-safety: only one `StderrCapture` may be active per test binary at
//!   a time.  Each `[[test]]` file is its own binary, so this is safe as long
//!   as tests run sequentially within that binary (the default).
//! - The `Drop` impl restores fd 2 unconditionally, so a panic during the
//!   captured window still restores stderr.
//! - `collect()` consumes the capture and closes the write end, which signals
//!   EOF to the reader thread, allowing it to drain cleanly.

use std::io::Read;
use std::thread;

/// A guard that redirects fd 2 (stderr) to an OS pipe.
///
/// Drop (or call [`collect`]) to restore stderr and retrieve lines.
pub(crate) struct StderrCapture {
    /// File descriptor of the original stderr (saved before redirection).
    saved_fd: libc::c_int,
    /// Read end of the capture pipe.
    read_fd: libc::c_int,
    /// Write end of the capture pipe (held open until `collect` closes it).
    write_fd: libc::c_int,
}

impl StderrCapture {
    /// Redirect fd 2 to a new pipe.  Returns the capture guard.
    ///
    /// # Safety
    ///
    /// Uses `libc::dup`/`libc::dup2` — raw fd operations.  Safe here because
    /// the test binary is single-threaded prior to `spawn_actor`, and the fd
    /// operations are done atomically before any actor thread is started.
    #[allow(unsafe_code)]
    pub(crate) fn start() -> Self {
        let mut fds = [0i32; 2];
        let rc = unsafe { libc::pipe(fds.as_mut_ptr()) };
        assert_eq!(rc, 0, "StderrCapture: pipe() failed");
        let read_fd = fds[0];
        let write_fd = fds[1];

        // Save the current stderr so we can restore it in Drop.
        let saved_fd = unsafe { libc::dup(2) };
        assert!(saved_fd >= 0, "StderrCapture: dup(2) failed");

        // Redirect stderr to the write end of the pipe.
        let rc = unsafe { libc::dup2(write_fd, 2) };
        assert_eq!(rc, 2, "StderrCapture: dup2(write_fd, 2) failed");

        Self {
            saved_fd,
            read_fd,
            write_fd,
        }
    }

    /// Stop capturing, restore stderr, and return all captured lines.
    ///
    /// Closes the write end so the reader sees EOF.  Spawns a thread to drain
    /// the read end (avoids blocking when the buffer is large).
    #[allow(unsafe_code)]
    pub(crate) fn collect(self) -> Vec<String> {
        // Restore stderr before touching the pipe so any subsequent
        // eprintln! calls go to the real terminal.
        unsafe {
            libc::dup2(self.saved_fd, 2);
            libc::close(self.saved_fd);
            // Close the write end — this signals EOF to the read thread.
            libc::close(self.write_fd);
        }

        // Drain the read end on a background thread.
        let read_fd = self.read_fd;
        let reader = thread::spawn(move || {
            let mut f = unsafe {
                use std::os::unix::io::FromRawFd;
                std::fs::File::from_raw_fd(read_fd)
            };
            let mut buf = String::new();
            let _ = f.read_to_string(&mut buf);
            buf
        });

        // Forget `self` so Drop doesn't double-close.
        std::mem::forget(self);

        let raw = reader.join().unwrap_or_default();
        raw.lines().map(str::to_owned).collect()
    }
}

impl Drop for StderrCapture {
    /// Safety guard: if `collect` was NOT called (e.g. test panicked), restore
    /// stderr and close the file descriptors so the process doesn't leak them.
    #[allow(unsafe_code)]
    fn drop(&mut self) {
        if self.saved_fd >= 0 {
            unsafe {
                libc::dup2(self.saved_fd, 2);
                libc::close(self.saved_fd);
                libc::close(self.write_fd);
                libc::close(self.read_fd);
            }
            // Mark as consumed so a double-drop (via forget path) is benign.
            self.saved_fd = -1;
        }
    }
}

// ─── Wire-log parsing helpers ────────────────────────────────────────────────

/// Filter captured lines to those matching `"nmp.wire {"type":"<event_type>"…}"`.
///
/// Returns owned copies of the JSON payload (without the `"nmp.wire "` prefix).
pub(crate) fn wire_log_events<'a>(
    lines: &'a [String],
    event_type: &'a str,
) -> impl Iterator<Item = &'a str> {
    lines.iter().filter_map(move |line| {
        let payload = line.strip_prefix("nmp.wire ")?;
        if payload.contains(&format!("\"type\":\"{event_type}\"")) {
            Some(payload)
        } else {
            None
        }
    })
}

/// Return all `relay_url` values from `ReqEmit` lines filtered by `phase`.
pub(crate) fn req_emit_relays_for_phase<'a>(lines: &'a [String], phase: &'a str) -> Vec<String> {
    wire_log_events(lines, "ReqEmit")
        .filter(|payload| payload.contains(&format!("\"phase\":\"{phase}\"")))
        .filter_map(|payload| {
            // Extract `"relay_url":"<value>"` from the JSON payload.
            let after = payload.split("\"relay_url\":\"").nth(1)?;
            let url = after.split('"').next()?;
            Some(url.to_owned())
        })
        .collect()
}

/// Return `true` if any `EventRx` line has `"author":"<author>"`.
pub(crate) fn event_rx_for_author(lines: &[String], author: &str) -> bool {
    wire_log_events(lines, "EventRx")
        .any(|payload| payload.contains(&format!("\"author\":\"{author}\"")))
}

/// Return all `(author, relay_url, delta, new_weight)` tuples from `ScoreUpdate` lines.
pub(crate) fn score_updates(lines: &[String]) -> Vec<(String, String, String, f64)> {
    wire_log_events(lines, "ScoreUpdate")
        .filter_map(|payload| {
            let v: serde_json::Value = serde_json::from_str(payload).ok()?;
            let author = v.get("author")?.as_str()?.to_owned();
            let relay_url = v.get("relay_url")?.as_str()?.to_owned();
            let delta = v.get("delta")?.as_str()?.to_owned();
            let new_weight = v.get("new_weight")?.as_f64().unwrap_or(0.0);
            Some((author, relay_url, delta, new_weight))
        })
        .collect()
}
