//! A connection to one scope. The socket lives on its own thread, so the game
//! and render threads never block on the network: they queue frames with
//! [`Connection::send`] and collect [`Event`]s with [`Connection::poll`].
//! Drops reconnect with exponential backoff, fetching a fresh token each time.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TryRecvError, TrySendError};
use std::time::{Duration, Instant};

use crate::event::{Event, decode};
use crate::protocol::{SUBPROTOCOL, Writer};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Status {
    Connecting,
    Connected,
    /// Lost; trying again after the given delay.
    Reconnecting {
        attempt: u32,
        in_ms: u64,
        reason: String,
    },
    /// Stopped for good (closed, or the token was refused).
    Closed {
        reason: String,
    },
}

/// Where to connect. `base` is e.g. `wss://circuit.kitgameengine.win/api/v1/realtime/connect`.
#[derive(Clone, Debug)]
pub struct Target {
    pub base: String,
    pub environment: String,
    pub scope: String,
    /// Sectors: let the service pick an instance with room.
    pub auto_instance: bool,
    /// Keep a party together in instanced sectors.
    pub party: Option<String>,
}

impl Target {
    pub fn url(&self) -> String {
        let mut u = format!("{}?environment={}&scope={}", self.base, self.environment, self.scope);
        if self.auto_instance {
            u.push_str("&instance=auto");
        }
        if let Some(p) = &self.party {
            u.push_str(&format!("&party={p}"));
        }
        u
    }
}

/// Supplies a fresh connection token (from the game's backend or the
/// anonymous endpoint). Called on every (re)connect, off the game thread.
pub type TokenSource = Arc<dyn Fn() -> anyhow::Result<String> + Send + Sync>;

/// Bounded queues: a stalled network never grows memory without limit.
const OUTBOX: usize = 256;
const INBOX: usize = 4096;
const MAX_BACKOFF_MS: u64 = 15_000;

pub struct Connection {
    out: SyncSender<Vec<u8>>,
    inbox: Receiver<Event>,
    stop: Arc<AtomicBool>,
    dropped: u64,
}

impl Connection {
    #[cfg(not(target_arch = "wasm32"))]
    pub fn open(target: Target, tokens: TokenSource) -> Self {
        let (out, out_rx) = std::sync::mpsc::sync_channel::<Vec<u8>>(OUTBOX);
        let (in_tx, inbox) = std::sync::mpsc::sync_channel::<Event>(INBOX);
        let stop = Arc::new(AtomicBool::new(false));
        let stop2 = stop.clone();
        std::thread::Builder::new()
            .name(format!("kit-realtime {}", target.scope))
            .spawn(move || run(target, tokens, out_rx, in_tx, stop2))
            .expect("spawn realtime thread");
        Self { out, inbox, stop, dropped: 0 }
    }

    /// Queue one frame (one or more messages). Returns false when the outbox is
    /// full (the network is behind): drop transforms, retry state later.
    pub fn send(&mut self, frame: &mut Writer) -> bool {
        if frame.is_empty() {
            return true;
        }
        match self.out.try_send(frame.take()) {
            Ok(()) => true,
            Err(TrySendError::Full(_)) | Err(TrySendError::Disconnected(_)) => {
                self.dropped += 1;
                false
            }
        }
    }

    /// Frames dropped because the outbox was full.
    pub fn dropped(&self) -> u64 {
        self.dropped
    }

    /// Everything that arrived since the last poll.
    pub fn poll(&mut self) -> Vec<Event> {
        let mut v = Vec::new();
        loop {
            match self.inbox.try_recv() {
                Ok(e) => v.push(e),
                Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => return v,
            }
        }
    }

    pub fn close(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl Drop for Connection {
    fn drop(&mut self) {
        self.close();
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn run(
    target: Target,
    tokens: TokenSource,
    out_rx: Receiver<Vec<u8>>,
    inbox: SyncSender<Event>,
    stop: Arc<AtomicBool>,
) {
    use tungstenite::client::IntoClientRequest;
    use tungstenite::{Message, stream::MaybeTlsStream};

    let status = |s: Status| {
        let _ = inbox.try_send(Event::Status(s));
    };
    let mut attempt = 0u32;
    while !stop.load(Ordering::Relaxed) {
        status(Status::Connecting);
        let fail = |reason: String, attempt: &mut u32| -> bool {
            *attempt += 1;
            let ms = (250u64 << (*attempt).min(6)).min(MAX_BACKOFF_MS);
            status(Status::Reconnecting { attempt: *attempt, in_ms: ms, reason });
            let until = Instant::now() + Duration::from_millis(ms);
            while Instant::now() < until {
                if stop.load(Ordering::Relaxed) {
                    return false;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            true
        };
        let token = match tokens() {
            Ok(t) => t,
            Err(e) => {
                if fail(format!("couldn't get a token: {e:#}"), &mut attempt) {
                    continue;
                }
                break;
            }
        };
        let mut req = match target.url().into_client_request() {
            Ok(r) => r,
            Err(e) => {
                status(Status::Closed { reason: format!("bad address: {e}") });
                return;
            }
        };
        req.headers_mut().insert("sec-websocket-protocol", format!("{SUBPROTOCOL}, {token}").parse().expect("header"));
        let (mut ws, _resp) = match tungstenite::connect(req) {
            Ok(c) => c,
            Err(tungstenite::Error::Http(resp)) if matches!(resp.status().as_u16(), 401 | 403 | 404) => {
                let body = resp.body().as_ref().map(|b| String::from_utf8_lossy(b).into_owned()).unwrap_or_default();
                // A refused token may just be expired: one fresh try, then give up.
                if attempt < 1 && fail(format!("refused ({}): {body}", resp.status()), &mut attempt) {
                    continue;
                }
                status(Status::Closed { reason: format!("refused ({}): {body}", resp.status()) });
                return;
            }
            Err(e) => {
                if fail(e.to_string(), &mut attempt) {
                    continue;
                }
                break;
            }
        };
        // Short reads so the thread also services the outbox.
        let timeout = Some(Duration::from_millis(5));
        match ws.get_mut() {
            MaybeTlsStream::Plain(s) => {
                let _ = s.set_read_timeout(timeout);
                let _ = s.set_nodelay(true);
            }
            MaybeTlsStream::Rustls(s) => {
                let _ = s.get_mut().set_read_timeout(timeout);
                let _ = s.get_mut().set_nodelay(true);
            }
            _ => {}
        }
        attempt = 0;
        status(Status::Connected);
        let reason = loop {
            if stop.load(Ordering::Relaxed) {
                let _ = ws.close(None);
                let _ = ws.flush();
                status(Status::Closed { reason: "closed".into() });
                return;
            }
            let mut wrote = false;
            while let Ok(frame) = out_rx.try_recv() {
                if ws.write(Message::Binary(frame.into())).is_err() {
                    break;
                }
                wrote = true;
            }
            if wrote && let Err(e) = ws.flush() {
                break e.to_string();
            }
            match ws.read() {
                Ok(Message::Binary(b)) => {
                    let mut events = Vec::new();
                    if let Err(e) = decode(&b, &mut events) {
                        log::warn!("realtime: {e}");
                    }
                    for e in events {
                        // Inbox full means the game stopped polling; drop rather than grow.
                        let _ = inbox.try_send(e);
                    }
                }
                Ok(Message::Close(frame)) => {
                    break frame.map(|f| f.reason.to_string()).unwrap_or_else(|| "closed by scope".into());
                }
                Ok(_) => {}
                Err(tungstenite::Error::Io(e))
                    if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut) => {}
                Err(e) => break e.to_string(),
            }
        };
        if !fail(reason, &mut attempt) {
            break;
        }
    }
    status(Status::Closed { reason: "closed".into() });
}
