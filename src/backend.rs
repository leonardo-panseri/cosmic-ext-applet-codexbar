// SPDX-License-Identifier: MPL-2.0

//! Client for the desktop app's local socket, `$XDG_RUNTIME_DIR/codexbar-linux/desktop.sock`.
//!
//! One request per connection: we send one line of compact JSON, the desktop app replies with
//! one line and closes. Timeouts and the reply cap match upstream's own client
//! (`Integrations/Linux/main.cpp`, `request()`).

use std::ffi::OsString;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::Deserialize;
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;
use tokio::time::{Instant, timeout_at};

use crate::snapshot::{self, Incompatible, Snapshot};

/// Why a request didn't produce a usable reply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackendError {
    /// There's no socket: the desktop app isn't installed or isn't running, or
    /// `XDG_RUNTIME_DIR` isn't set.
    NotRunning,
    /// The socket exists but refuses connections, which is what a crashed desktop app leaves
    /// behind, or the connection closed before a whole reply line arrived.
    NotResponding,
    /// Connecting took over 500 ms, or the whole exchange over 3 s.
    Timeout,
    /// The reply grew past 4 MiB without a line end.
    TooLarge,
    /// The reply isn't a snapshot or acknowledgement this applet understands.
    Incompatible(Incompatible),
    /// The desktop app replied `ok: false`, with its reason, which may be empty.
    Rejected(String),
}

impl std::fmt::Display for BackendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotRunning => f.write_str("CodexBar for Linux isn't running"),
            Self::NotResponding => f.write_str("CodexBar for Linux isn't responding"),
            Self::Timeout => f.write_str("CodexBar for Linux timed out"),
            Self::TooLarge => f.write_str("CodexBar for Linux sent a reply that is too large"),
            Self::Incompatible(reason) => {
                write!(f, "incompatible reply from CodexBar for Linux: {reason}")
            }
            Self::Rejected(reason) if reason.is_empty() => {
                f.write_str("CodexBar for Linux refused the request")
            }
            Self::Rejected(reason) => f.write_str(reason),
        }
    }
}

impl std::error::Error for BackendError {}

/// A desktop app window the applet can ask it to open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Window {
    Usage,
    /// Unused: the popup's "Usage & Spend…" opens `Usage`, as upstream's adapters do.
    #[allow(dead_code)]
    Spending,
    Settings,
}

impl Window {
    fn command(self) -> &'static str {
        match self {
            Self::Usage => "usage",
            Self::Spending => "spending",
            Self::Settings => "settings",
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Limits {
    connect: Duration,
    deadline: Duration,
    max_reply: usize,
}

const LIMITS: Limits = Limits {
    connect: Duration::from_millis(500),
    deadline: Duration::from_secs(3),
    max_reply: 4 * 1024 * 1024,
};

/// The desktop app, as reached through its socket. Cheap to clone into tasks.
#[derive(Debug, Clone)]
pub struct Backend {
    socket: Option<PathBuf>,
    limits: Limits,
}

impl Backend {
    /// The socket under `$XDG_RUNTIME_DIR`. Without it every request fails with `NotRunning`:
    /// upstream then falls back to a folder in `/tmp`, which the sandbox can't reach.
    pub fn from_env() -> Self {
        Self::at(socket_path(std::env::var_os("XDG_RUNTIME_DIR")))
    }

    pub fn at(socket: Option<PathBuf>) -> Self {
        Self {
            socket,
            limits: LIMITS,
        }
    }

    #[allow(dead_code)] // Only the tests use it so far.
    pub fn socket(&self) -> Option<&Path> {
        self.socket.as_deref()
    }

    /// With `spending`, the desktop app also starts a spending scan if its cache is older than
    /// five minutes. The scan's result arrives in a later snapshot.
    pub async fn snapshot(&self, spending: bool) -> Result<Snapshot, BackendError> {
        let request = if spending {
            json!({"command": "snapshot", "spending": true})
        } else {
            json!({"command": "snapshot"})
        };
        let reply = self.exchange(&request).await?;
        snapshot::parse(&reply).map_err(BackendError::Incompatible)
    }

    /// Asks the desktop app to query every provider and rescan spending now.
    pub async fn refresh(&self) -> Result<(), BackendError> {
        self.command(&json!({"command": "refresh"})).await.map(drop)
    }

    pub async fn open(&self, window: Window) -> Result<(), BackendError> {
        self.command(&json!({"command": window.command()}))
            .await
            .map(drop)
    }

    /// Whether the desktop app starts at login.
    pub async fn autostart_status(&self) -> Result<bool, BackendError> {
        self.autostart("status").await
    }

    /// Turns on start at login and returns the resulting state.
    pub async fn enable_autostart(&self) -> Result<bool, BackendError> {
        self.autostart("enable").await
    }

    /// Hides the desktop app's own tray icon and saves that in its settings.
    pub async fn hide_tray(&self) -> Result<(), BackendError> {
        self.command(&json!({"command": "configure", "settings": {"showTray": false}}))
            .await
            .map(drop)
    }

    async fn autostart(&self, action: &str) -> Result<bool, BackendError> {
        let ack = self
            .command(&json!({"command": "autostart", "action": action}))
            .await?;
        ack.enabled.ok_or_else(|| {
            BackendError::Incompatible(Incompatible::InvalidJson(
                "autostart reply has no `enabled`".into(),
            ))
        })
    }

    /// Sends a non-snapshot command and checks its `ok`.
    async fn command(&self, request: &Value) -> Result<Ack, BackendError> {
        let reply = self.exchange(request).await?;
        let ack: Ack = serde_json::from_slice(&reply)
            .map_err(|e| BackendError::Incompatible(Incompatible::InvalidJson(e.to_string())))?;
        if ack.ok {
            Ok(ack)
        } else {
            Err(BackendError::Rejected(ack.error))
        }
    }

    /// Sends one request line and returns the reply line without its `\n`.
    async fn exchange(&self, request: &Value) -> Result<Vec<u8>, BackendError> {
        let socket = self.socket.as_deref().ok_or(BackendError::NotRunning)?;
        let start = Instant::now();
        let mut stream = timeout_at(start + self.limits.connect, UnixStream::connect(socket))
            .await
            .map_err(|_| BackendError::Timeout)?
            .map_err(|e| match e.kind() {
                ErrorKind::NotFound | ErrorKind::NotADirectory => BackendError::NotRunning,
                _ => BackendError::NotResponding,
            })?;
        timeout_at(
            start + self.limits.deadline,
            self.converse(&mut stream, request),
        )
        .await
        .map_err(|_| BackendError::Timeout)?
    }

    async fn converse(
        &self,
        stream: &mut UnixStream,
        request: &Value,
    ) -> Result<Vec<u8>, BackendError> {
        let mut line = request.to_string().into_bytes();
        line.push(b'\n');
        stream
            .write_all(&line)
            .await
            .map_err(|_| BackendError::NotResponding)?;

        let mut reply = Vec::with_capacity(16 * 1024);
        loop {
            let scanned = reply.len();
            reply.reserve(16 * 1024);
            let read = stream
                .read_buf(&mut reply)
                .await
                .map_err(|_| BackendError::NotResponding)?;
            if read == 0 {
                // Upstream's client also treats a reply without a line end as no reply.
                return Err(BackendError::NotResponding);
            }
            if let Some(end) = reply[scanned..].iter().position(|&b| b == b'\n') {
                reply.truncate(scanned + end);
                return Ok(reply);
            }
            if reply.len() >= self.limits.max_reply {
                return Err(BackendError::TooLarge);
            }
        }
    }
}

/// `{runtime}/codexbar-linux/desktop.sock`, if `runtime` is set and absolute.
fn socket_path(runtime: Option<OsString>) -> Option<PathBuf> {
    let runtime = PathBuf::from(runtime?);
    runtime
        .is_absolute()
        .then(|| runtime.join("codexbar-linux").join("desktop.sock"))
}

/// Reply to every command except `snapshot`.
#[derive(Debug, Deserialize)]
struct Ack {
    /// Upstream's client also treats a missing `ok` as success.
    #[serde(default = "yes")]
    ok: bool,
    #[serde(default)]
    enabled: Option<bool>,
    #[serde(default)]
    error: String,
}

fn yes() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncBufReadExt, BufReader};
    use tokio::net::UnixListener;
    use tokio::task::JoinHandle;

    const TEST_LIMITS: Limits = Limits {
        connect: Duration::from_millis(500),
        deadline: Duration::from_millis(200),
        max_reply: 64 * 1024,
    };

    struct FakeServer {
        _dir: tempfile::TempDir,
        backend: Backend,
    }

    fn fake_dir() -> FakeServer {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("desktop.sock");
        FakeServer {
            backend: Backend {
                socket: Some(socket),
                limits: TEST_LIMITS,
            },
            _dir: dir,
        }
    }

    impl FakeServer {
        fn path(&self) -> &Path {
            self.backend.socket().unwrap()
        }

        /// Accepts one connection, reads the request line, sends `reply` and closes.
        /// The handle yields the request line.
        fn reply_once(&self, reply: impl Into<Vec<u8>>) -> JoinHandle<String> {
            let listener = UnixListener::bind(self.path()).unwrap();
            let reply = reply.into();
            tokio::spawn(async move {
                let (stream, _) = listener.accept().await.unwrap();
                let mut stream = BufReader::new(stream);
                let mut request = String::new();
                stream.read_line(&mut request).await.unwrap();
                // The client may hang up first, for example after `TooLarge`.
                let _ = stream.get_mut().write_all(&reply).await;
                request
            })
        }
    }

    async fn request_for(reply: &str, call: impl AsyncFnOnce(&Backend)) -> Value {
        let server = fake_dir();
        let handle = server.reply_once(format!("{reply}\n"));
        call(&server.backend).await;
        serde_json::from_str(&handle.await.unwrap()).unwrap()
    }

    /// Against the real desktop app: `cargo test -- --ignored live`. Read-only.
    #[tokio::test]
    #[ignore = "needs a running codexbar-linux"]
    async fn live_snapshot_and_autostart_status() {
        let backend = Backend::from_env();
        let snapshot = backend.snapshot(false).await.unwrap();
        println!("{:?}", snapshot.bar());
        println!("autostart: {}", backend.autostart_status().await.unwrap());
    }

    #[test]
    fn socket_path_needs_an_absolute_runtime_dir() {
        assert_eq!(
            socket_path(Some("/run/user/1000".into())),
            Some(PathBuf::from("/run/user/1000/codexbar-linux/desktop.sock"))
        );
        assert_eq!(socket_path(None), None);
        assert_eq!(socket_path(Some("".into())), None);
        assert_eq!(socket_path(Some("relative".into())), None);
    }

    #[tokio::test]
    async fn no_runtime_dir_is_not_running() {
        assert_eq!(
            Backend::at(None).snapshot(false).await,
            Err(BackendError::NotRunning)
        );
    }

    #[tokio::test]
    async fn no_socket_is_not_running() {
        let server = fake_dir();
        assert_eq!(
            server.backend.snapshot(false).await,
            Err(BackendError::NotRunning)
        );

        let missing_folder = Backend::at(Some(server.path().join("nested/desktop.sock")));
        assert_eq!(
            missing_folder.snapshot(false).await,
            Err(BackendError::NotRunning)
        );
    }

    #[tokio::test]
    async fn stale_socket_is_not_responding() {
        let server = fake_dir();
        drop(std::os::unix::net::UnixListener::bind(server.path()).unwrap());
        assert!(server.path().exists());
        assert_eq!(
            server.backend.snapshot(false).await,
            Err(BackendError::NotResponding)
        );
    }

    #[tokio::test]
    async fn silent_server_times_out() {
        let server = fake_dir();
        let listener = UnixListener::bind(server.path()).unwrap();
        let _held = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            tokio::time::sleep(Duration::from_secs(60)).await;
            drop(stream);
        });
        let started = Instant::now();
        assert_eq!(
            server.backend.snapshot(false).await,
            Err(BackendError::Timeout)
        );
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[tokio::test]
    async fn oversized_reply_is_too_large() {
        let server = fake_dir();
        let _handle = server.reply_once(vec![b' '; TEST_LIMITS.max_reply + 1]);
        assert_eq!(
            server.backend.snapshot(false).await,
            Err(BackendError::TooLarge)
        );
    }

    #[tokio::test]
    async fn reply_without_newline_is_not_responding() {
        let server = fake_dir();
        let _handle = server.reply_once(r#"{"schemaVersion":1}"#);
        assert_eq!(
            server.backend.snapshot(false).await,
            Err(BackendError::NotResponding)
        );
    }

    #[tokio::test]
    async fn closed_without_reply_is_not_responding() {
        let server = fake_dir();
        let _handle = server.reply_once("");
        assert_eq!(
            server.backend.snapshot(false).await,
            Err(BackendError::NotResponding)
        );
    }

    #[tokio::test]
    async fn snapshot_round_trip() {
        let fixture = include_str!("../tests/fixtures/snapshot-two-providers-spending.json");
        // The real server sends compact JSON; the fixture is pretty-printed.
        let compact = serde_json::from_str::<Value>(fixture).unwrap().to_string();
        let server = fake_dir();
        let handle = server.reply_once(format!("{compact}\nignored trailing bytes"));
        let snapshot = server.backend.snapshot(true).await.unwrap();
        assert_eq!(snapshot, snapshot::parse(fixture.as_bytes()).unwrap());
        assert_eq!(
            handle.await.unwrap(),
            "{\"command\":\"snapshot\",\"spending\":true}\n"
        );
    }

    #[tokio::test]
    async fn other_schema_is_incompatible() {
        let server = fake_dir();
        let _handle = server.reply_once("{\"schemaVersion\":2}\n");
        assert_eq!(
            server.backend.snapshot(false).await,
            Err(BackendError::Incompatible(Incompatible::SchemaVersion(
                Some(2)
            )))
        );
    }

    #[tokio::test]
    async fn requests_match_the_protocol() {
        let ok = r#"{"ok":true}"#;
        let on = r#"{"ok":true,"enabled":true}"#;
        assert_eq!(
            request_for(ok, async |b| assert_eq!(
                b.snapshot(false).await.map(drop),
                Err(BackendError::Incompatible(Incompatible::SchemaVersion(
                    None
                )))
            ))
            .await,
            json!({"command": "snapshot"})
        );
        assert_eq!(
            request_for(ok, async |b| b.refresh().await.unwrap()).await,
            json!({"command": "refresh"})
        );
        for (window, command) in [
            (Window::Usage, "usage"),
            (Window::Spending, "spending"),
            (Window::Settings, "settings"),
        ] {
            assert_eq!(
                request_for(ok, async |b| b.open(window).await.unwrap()).await,
                json!({"command": command})
            );
        }
        assert_eq!(
            request_for(on, async |b| assert!(b.autostart_status().await.unwrap())).await,
            json!({"command": "autostart", "action": "status"})
        );
        assert_eq!(
            request_for(on, async |b| assert!(b.enable_autostart().await.unwrap())).await,
            json!({"command": "autostart", "action": "enable"})
        );
        assert_eq!(
            request_for(ok, async |b| b.hide_tray().await.unwrap()).await,
            json!({"command": "configure", "settings": {"showTray": false}})
        );
    }

    #[tokio::test]
    async fn acknowledgements() {
        let server = fake_dir();
        let _handle = server.reply_once("{\"ok\":false,\"error\":\"linux.json is invalid\"}\n");
        assert_eq!(
            server.backend.hide_tray().await,
            Err(BackendError::Rejected("linux.json is invalid".into()))
        );

        // An older desktop app without the command.
        let server = fake_dir();
        let _handle = server.reply_once("{\"ok\":false,\"error\":\"Unknown command\"}\n");
        assert_eq!(
            server.backend.autostart_status().await,
            Err(BackendError::Rejected("Unknown command".into()))
        );

        let server = fake_dir();
        let _handle = server.reply_once("{\"ok\":true,\"enabled\":false}\n");
        assert_eq!(server.backend.autostart_status().await, Ok(false));

        let server = fake_dir();
        let _handle = server.reply_once("{\"ok\":true}\n");
        assert!(matches!(
            server.backend.autostart_status().await,
            Err(BackendError::Incompatible(_))
        ));

        let server = fake_dir();
        let _handle = server.reply_once("{}\n");
        assert_eq!(server.backend.refresh().await, Ok(()));

        let server = fake_dir();
        let _handle = server.reply_once("not json\n");
        assert!(matches!(
            server.backend.refresh().await,
            Err(BackendError::Incompatible(_))
        ));
    }
}
