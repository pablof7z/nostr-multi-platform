use std::collections::VecDeque;
use std::io;
use std::net::TcpStream;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use mio::unix::SourceFd;
use mio::{Events, Interest, Poll, Token, Waker};
use tungstenite::stream::MaybeTlsStream;

use super::{BackoffClass, RelayCommand, RelaySocket};

const SOCKET: Token = Token(0);
const CONTROL: Token = Token(1);

pub(super) enum ControlDrain {
    Continue,
    Shutdown,
    Disconnected,
}

pub(super) struct ControlInbox {
    rx: Receiver<RelayCommand>,
    wake: Arc<Mutex<Option<Waker>>>,
}

pub(super) fn spawn_control_inbox(control_rx: Receiver<RelayCommand>) -> ControlInbox {
    let (tx, rx) = mpsc::channel();
    let wake = Arc::new(Mutex::new(None));
    let forward_wake = Arc::clone(&wake);
    thread::spawn(move || forward_commands(control_rx, tx, forward_wake));
    ControlInbox { rx, wake }
}

fn forward_commands(
    control_rx: Receiver<RelayCommand>,
    tx: Sender<RelayCommand>,
    wake: Arc<Mutex<Option<Waker>>>,
) {
    while let Ok(command) = control_rx.recv() {
        if tx.send(command).is_err() {
            return;
        }
        if let Ok(slot) = wake.lock() {
            if let Some(waker) = slot.as_ref() {
                let _ = waker.wake();
            }
        }
    }
}

impl ControlInbox {
    /// Drain pending commands into `pending` (for outbound text frames) and
    /// `backoff_hint` (for V-58 rate-limit hints). Returns the appropriate
    /// `ControlDrain` variant when a shutdown or disconnect is observed.
    ///
    /// `SetBackoffHint` updates the caller-supplied `backoff_hint` slot; the
    /// last hint wins if multiple arrive before the next disconnect. The caller
    /// consumes the hint in the reconnect branch and clears it there.
    pub(super) fn drain_pending(
        &self,
        pending: &mut VecDeque<String>,
        backoff_hint: &mut Option<BackoffClass>,
    ) -> ControlDrain {
        loop {
            match self.rx.try_recv() {
                Ok(RelayCommand::Send(text)) => pending.push_back(text),
                Ok(RelayCommand::Shutdown) => return ControlDrain::Shutdown,
                // V-58: store the hint; last writer wins.
                Ok(RelayCommand::SetBackoffHint(class)) => *backoff_hint = Some(class),
                Err(TryRecvError::Empty) => return ControlDrain::Continue,
                Err(TryRecvError::Disconnected) => return ControlDrain::Disconnected,
            }
        }
    }

    pub(super) fn recv_timeout(&self, timeout: Duration) -> Result<RelayCommand, RecvTimeoutError> {
        self.rx.recv_timeout(timeout)
    }

    fn install_waker(&self, waker: Waker) -> ControlWakeGuard {
        if let Ok(mut slot) = self.wake.lock() {
            *slot = Some(waker);
        }
        ControlWakeGuard {
            wake: Arc::clone(&self.wake),
        }
    }
}

pub(super) struct ControlWakeGuard {
    wake: Arc<Mutex<Option<Waker>>>,
}

impl Drop for ControlWakeGuard {
    fn drop(&mut self) {
        if let Ok(mut slot) = self.wake.lock() {
            *slot = None;
        }
    }
}

#[derive(Default)]
pub(super) struct Ready {
    pub(super) control: bool,
    pub(super) readable: bool,
    pub(super) writable: bool,
}

pub(super) struct RelayPoller {
    poll: Poll,
    events: Events,
    wants_write: bool,
}

impl RelayPoller {
    pub(super) fn new(
        socket: &mut RelaySocket,
        control: &ControlInbox,
    ) -> io::Result<(Self, ControlWakeGuard)> {
        socket_tcp(socket)?.set_nonblocking(true)?;
        let poll = Poll::new()?;
        register_socket(&poll, socket, false, false)?;
        let guard = control.install_waker(Waker::new(poll.registry(), CONTROL)?);
        Ok((
            Self {
                poll,
                events: Events::with_capacity(16),
                wants_write: false,
            },
            guard,
        ))
    }

    pub(super) fn set_wants_write(
        &mut self,
        socket: &mut RelaySocket,
        wants_write: bool,
    ) -> io::Result<()> {
        if self.wants_write == wants_write {
            return Ok(());
        }
        register_socket(&self.poll, socket, wants_write, true)?;
        self.wants_write = wants_write;
        Ok(())
    }

    pub(super) fn wait(&mut self, timeout: Duration) -> io::Result<Ready> {
        self.poll.poll(&mut self.events, Some(timeout))?;
        let mut ready = Ready::default();
        for event in &self.events {
            match event.token() {
                CONTROL => ready.control = true,
                SOCKET => {
                    ready.readable |= event.is_readable();
                    ready.writable |= event.is_writable();
                }
                _ => {}
            }
        }
        Ok(ready)
    }
}

fn register_socket(
    poll: &Poll,
    socket: &mut RelaySocket,
    wants_write: bool,
    registered: bool,
) -> io::Result<()> {
    use std::os::unix::io::AsRawFd;

    let fd = socket_tcp(socket)?.as_raw_fd();
    let interest = if wants_write {
        Interest::READABLE.add(Interest::WRITABLE)
    } else {
        Interest::READABLE
    };
    let mut source = SourceFd(&fd);
    if registered {
        poll.registry().reregister(&mut source, SOCKET, interest)
    } else {
        poll.registry().register(&mut source, SOCKET, interest)
    }
}

fn socket_tcp(socket: &mut RelaySocket) -> io::Result<&mut TcpStream> {
    match socket.get_mut() {
        MaybeTlsStream::Plain(stream) => Ok(stream),
        MaybeTlsStream::Rustls(stream) => Ok(stream.get_mut()),
        #[allow(unreachable_patterns)]
        _ => Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "unsupported relay socket stream variant",
        )),
    }
}
