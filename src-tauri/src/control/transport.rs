use super::Error;
use crate::storage::{file_identity, private_fs};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    panic::AssertUnwindSafe,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc, Arc, Mutex,
    },
    time::{Duration, Instant},
};

pub const PROTOCOL: u32 = 2;
pub const MAX_FRAME: usize = 1024 * 1024;
const MAGIC: &[u8; 8] = b"SNIPER1\n";
// Distinct labels keep each proof from ever being reusable in another role
// even though all are keyed by the same capability.
const CHALLENGE_PROOF: &[u8] = b"pr-sniper/control/2/challenge";
const REQUEST_PROOF: &[u8] = b"pr-sniper/control/2/request";
const RESPONSE_PROOF: &[u8] = b"pr-sniper/control/2/response";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Endpoint {
    pub protocol: u32,
    pub port: u16,
    pub token: String,
    pub app_version: String,
    /// Lets a client discard a descriptor left behind by a crashed app.
    pub pid: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Hello {
    protocol: u32,
    nonce: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    protocol: u32,
    proof: String,
    /// Base64 of the exact request bytes the proof covers.
    request: String,
}

pub(crate) struct Server {
    listener: TcpListener,
    endpoint: Endpoint,
    path: PathBuf,
    /// The descriptor this server wrote; only that file is ever withdrawn.
    file: file_identity::Identity,
    queue_wait: Duration,
}

/// Fail closed rather than repair a publicly readable control directory.
pub(crate) fn private_directory(path: &Path) -> Result<(), String> {
    private_fs::directory(path).map_err(|_| "Control directory unavailable.")?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let metadata = fs::symlink_metadata(path).map_err(|_| "Control directory unavailable.")?;
        // std has no getuid; the user's home owner also excludes a foreign
        // precreated endpoint without introducing a libc dependency.
        let home = std::env::var_os("HOME").ok_or("Home directory unavailable.")?;
        let owner = fs::metadata(home)
            .map_err(|_| "Home directory unavailable.")?
            .uid();
        if metadata.uid() != owner || metadata.permissions().mode() & 0o077 != 0 {
            return Err("Control directory is not private to this user.".into());
        }
    }
    Ok(())
}

impl Server {
    pub(crate) fn bind(directory: &Path) -> Result<Self, String> {
        private_directory(directory)?;
        let listener = TcpListener::bind(("127.0.0.1", 0))
            .map_err(|_| "Local control listener unavailable.")?;
        listener
            .set_nonblocking(true)
            .map_err(|_| "Local control unavailable.")?;
        let endpoint = Endpoint {
            protocol: PROTOCOL,
            port: listener
                .local_addr()
                .map_err(|_| "Local control unavailable.")?
                .port(),
            token: format!(
                "{}{}",
                uuid::Uuid::new_v4().simple(),
                uuid::Uuid::new_v4().simple()
            ),
            app_version: env!("CARGO_PKG_VERSION").into(),
            pid: std::process::id(),
        };
        let path = directory.join("endpoint.json");
        private_fs::replace(
            &path,
            &serde_json::to_vec(&endpoint).unwrap(),
            "control endpoint",
        )?;
        let file = file_identity::identity(&path)
            .ok()
            .flatten()
            .ok_or("Local control unavailable.")?;
        Ok(Self {
            listener,
            endpoint,
            path,
            file,
            queue_wait: QUEUE_WAIT,
        })
    }

    #[cfg(test)]
    pub(crate) fn with_queue_wait(mut self, wait: Duration) -> Self {
        self.queue_wait = wait;
        self
    }

    /// The calling thread is the single native executor. A separate acceptor
    /// hands each connection to a bounded pool that completes the handshake
    /// promptly, even while a long operation runs; only authenticated
    /// requests reach the executor's bounded queue.
    ///
    /// A panicking operation may have poisoned shared state or followed a
    /// partial commit, so it is fatal. `fatal` runs immediately to stop all
    /// other application work and returns the exit action. Then the request
    /// is `outcome_unknown`, queued requests are rejected, in-flight replies
    /// drain briefly, this server's descriptor is withdrawn and the exit
    /// action runs so the next client relaunches a clean process.
    pub(crate) fn run<Exit: FnOnce()>(
        self,
        quitting: impl Fn() -> bool,
        mut execute: impl FnMut(Value) -> Result<Value, Error>,
        fatal: impl FnOnce() -> Exit,
    ) {
        let stop = Arc::new(AtomicBool::new(false));
        let active = Arc::new(AtomicUsize::new(0));
        let failures = Arc::new(Mutex::new(Vec::new()));
        // Declared before the queue so an unwinding executor first releases
        // queued waiters, then stops accepting and withdraws the descriptor.
        let mut teardown = Teardown {
            stop: stop.clone(),
            active: active.clone(),
            acceptor: None,
            owned: Owned {
                path: self.path.clone(),
                token: self.endpoint.token.clone(),
                pid: self.endpoint.pid,
                file: self.file,
            },
        };
        let (jobs, queue) = mpsc::sync_channel::<Job>(QUEUE);
        let acceptor = {
            let stop = stop.clone();
            let connection = Connection {
                endpoint: Arc::new(self.endpoint),
                path: self.path.clone(),
                jobs,
                wait: self.queue_wait,
                failures: failures.clone(),
            };
            let listener = self.listener;
            std::thread::Builder::new()
                .name("local-control-accept".into())
                .spawn(move || accept(listener, connection, stop, active))
        };
        let Ok(acceptor) = acceptor else {
            return;
        };
        teardown.acceptor = Some(acceptor);
        let mut fatal = Some(fatal);
        let mut exit = None;
        while !quitting() {
            match queue.recv_timeout(Duration::from_millis(25)) {
                Ok(job) if Instant::now() > job.deadline => {
                    // The client can no longer wait for it: never execute.
                    let _ = job.reply.send(Err(expired()));
                }
                Ok(job) => {
                    let started = Instant::now();
                    match std::panic::catch_unwind(AssertUnwindSafe(|| execute(job.request))) {
                        Ok(result) => {
                            let _ = job.reply.send(result);
                        }
                        Err(_) => {
                            // Fail-stop before anything else can write.
                            exit = fatal.take().map(|begin| begin());
                            note(
                                &failures,
                                &self.path,
                                json!({"stage":"execute","kind":"Panic",
                                    "elapsed_ms":started.elapsed().as_millis()}),
                            );
                            // The operation may have partly committed: neither
                            // success nor failure.
                            let _ = job
                                .reply
                                .send(Err(Error::new("outcome_unknown", Value::Null)));
                            break;
                        }
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
        // Queued requests never ran; their clients get a typed rejection.
        drop(queue);
        drop(teardown);
        if let Some(exit) = exit {
            exit();
        }
    }
}

/// Identity of the descriptor a server wrote.
struct Owned {
    path: PathBuf,
    token: String,
    pid: u32,
    file: file_identity::Identity,
}

impl Owned {
    fn matches(&self, path: &Path) -> bool {
        file_identity::identity(path)
            .ok()
            .flatten()
            .is_some_and(|file| file.same_file(&self.file))
            && fs::read(path)
                .ok()
                .and_then(|bytes| serde_json::from_slice::<Endpoint>(&bytes).ok())
                .is_some_and(|endpoint| {
                    endpoint.pid == self.pid && authenticated(&endpoint.token, &self.token)
                })
    }

    /// Removes the descriptor only while it is still this server's. A
    /// successor's replacement is never deleted: the file is first moved
    /// aside and re-verified. One moved aside by a race is restored without
    /// overwriting (a hard link fails if a newer descriptor exists); it is
    /// deleted only once restored or superseded, otherwise kept and reported.
    fn withdraw(&self) {
        self.withdraw_with(|| {}, |aside, path| fs::hard_link(aside, path));
    }

    fn withdraw_with(
        &self,
        raced: impl FnOnce(),
        restore: impl FnOnce(&Path, &Path) -> std::io::Result<()>,
    ) {
        if !self.matches(&self.path) {
            return;
        }
        raced();
        let aside = self.path.with_file_name(format!(
            "endpoint.{}.retired",
            uuid::Uuid::new_v4().simple()
        ));
        if fs::rename(&self.path, &aside).is_err() {
            return;
        }
        if !self.matches(&aside) && restore(&aside, &self.path).is_err() && !self.superseded(&aside)
        {
            eprintln!("PR Sniper: kept a successor control endpoint that could not be restored.");
            return;
        }
        let _ = fs::remove_file(&aside);
    }

    /// A distinct, regular, valid descriptor of a live process now holds the
    /// public name. Anything else (missing, link, malformed, stale) is not.
    fn superseded(&self, aside: &Path) -> bool {
        let Ok(metadata) = fs::symlink_metadata(&self.path) else {
            return false;
        };
        if !metadata.file_type().is_file() || metadata.len() > 4096 {
            return false;
        }
        let (Ok(Some(current)), Ok(Some(moved))) = (
            file_identity::identity(&self.path),
            file_identity::identity(aside),
        ) else {
            return false;
        };
        !current.same_file(&moved)
            && fs::read(&self.path)
                .ok()
                .and_then(|bytes| serde_json::from_slice::<Endpoint>(&bytes).ok())
                .is_some_and(|endpoint| {
                    endpoint.protocol == PROTOCOL
                        && endpoint.port != 0
                        && is_nonce(&endpoint.token)
                        && live(endpoint.pid)
                })
    }
}

/// Whether `pid` is a running process this user can observe.
fn live(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    #[cfg(unix)]
    {
        extern "C" {
            fn kill(pid: i32, signal: i32) -> i32;
        }
        // Positive PIDs only: zero/negative values address process groups.
        let Ok(pid) = i32::try_from(pid) else {
            return false;
        };
        // SAFETY: signal 0 performs only the existence/permission check.
        unsafe { kill(pid, 0) == 0 }
    }
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::{
            Foundation::CloseHandle,
            System::Threading::{
                GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
            },
        };
        const STILL_ACTIVE: u32 = 259;
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if process.is_null() {
            return false;
        }
        let mut code = 0u32;
        let running = GetExitCodeProcess(process, &mut code) != 0 && code == STILL_ACTIVE;
        let _ = CloseHandle(process);
        running
    }
}

/// Stops accepting, waits briefly for in-flight replies, then withdraws this
/// server's descriptor, however the executor ends.
struct Teardown {
    stop: Arc<AtomicBool>,
    active: Arc<AtomicUsize>,
    acceptor: Option<std::thread::JoinHandle<()>>,
    owned: Owned,
}
impl Drop for Teardown {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(acceptor) = self.acceptor.take() {
            let _ = acceptor.join();
        }
        let deadline = Instant::now() + Duration::from_secs(3);
        while self.active.load(Ordering::SeqCst) > 0 && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        self.owned.withdraw();
    }
}

/// Maximum concurrent connections (handshaking, queued or awaiting a reply).
const CONNECTIONS: usize = 16;
/// Authenticated requests waiting behind the running native operation.
pub(crate) const QUEUE: usize = 4;
/// Clients wait `CLIENT_WAIT` after sending a request; native operations are
/// budgeted `OPERATION_BUDGET`. A request must start within the difference
/// (less a margin) or it is rejected unexecuted, so nothing runs after its
/// client has given up and reported `outcome_unknown`.
const CLIENT_WAIT: Duration = Duration::from_secs(120);
const OPERATION_BUDGET: Duration = Duration::from_secs(90);
const QUEUE_WAIT: Duration = CLIENT_WAIT
    .saturating_sub(OPERATION_BUDGET)
    .saturating_sub(Duration::from_secs(5));

struct Job {
    request: Value,
    reply: mpsc::Sender<Result<Value, Error>>,
    /// Latest start time; later the job is dropped unexecuted.
    deadline: Instant,
}

fn expired() -> Error {
    Error::new("control_busy", json!({"queue": QUEUE, "reason": "expired"}))
}

/// What each connection thread needs from the acceptor.
#[derive(Clone)]
struct Connection {
    endpoint: Arc<Endpoint>,
    path: PathBuf,
    jobs: mpsc::SyncSender<Job>,
    wait: Duration,
    failures: Arc<Mutex<Vec<Value>>>,
}

fn accept(
    listener: TcpListener,
    connection: Connection,
    stop: Arc<AtomicBool>,
    active: Arc<AtomicUsize>,
) {
    while !stop.load(Ordering::SeqCst) {
        // At capacity, further clients wait in the OS backlog.
        if active.load(Ordering::SeqCst) >= CONNECTIONS {
            std::thread::sleep(Duration::from_millis(5));
            continue;
        }
        match listener.accept() {
            Ok((stream, address)) if address.ip().is_loopback() => {
                // Released when the connection ends or its thread never starts.
                let slot = Slot::take(&active);
                let connection = connection.clone();
                let _ = std::thread::Builder::new()
                    .name("local-control-connection".into())
                    .spawn(move || {
                        let _slot = slot;
                        let started = Instant::now();
                        let result =
                            serve_diagnosed(stream, &connection.endpoint, &mut |request| {
                                submit(&connection.jobs, request, connection.wait)
                            });
                        if let Err(failure) = result {
                            record(&connection.failures, &connection.path, failure, started);
                        }
                    });
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(_) => break,
        }
    }
}

struct Slot(Arc<AtomicUsize>);
impl Slot {
    fn take(active: &Arc<AtomicUsize>) -> Self {
        active.fetch_add(1, Ordering::SeqCst);
        Self(active.clone())
    }
}
impl Drop for Slot {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
    }
}

/// Backpressure: a full queue is a typed, unexecuted rejection.
fn submit(jobs: &mpsc::SyncSender<Job>, request: Value, wait: Duration) -> Result<Value, Error> {
    let (reply, result) = mpsc::channel();
    let deadline = Instant::now() + wait;
    match jobs.try_send(Job {
        request,
        reply,
        deadline,
    }) {
        Ok(()) => result
            .recv()
            .unwrap_or_else(|_| Err(Error::new("app_quitting", Value::Null))),
        Err(mpsc::TrySendError::Full(_)) => {
            Err(Error::new("control_busy", json!({"queue": QUEUE})))
        }
        Err(mpsc::TrySendError::Disconnected(_)) => Err(Error::new("app_quitting", Value::Null)),
    }
}

fn record(failures: &Mutex<Vec<Value>>, path: &Path, failure: TransportFailure, started: Instant) {
    note(
        failures,
        path,
        json!({
            "stage":failure.stage,"kind":format!("{:?}",failure.error.kind()),
            "elapsed_ms":started.elapsed().as_millis()
        }),
    );
}

/// Content-free and bounded; never include frame, request or token bytes.
fn note(failures: &Mutex<Vec<Value>>, path: &Path, entry: Value) {
    let mut failures = failures
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    failures.push(entry);
    if failures.len() > 16 {
        failures.remove(0);
    }
    let _ = private_fs::replace(
        &path.with_file_name("transport-errors.json"),
        &serde_json::to_vec(&*failures).unwrap(),
        "control transport diagnostics",
    );
}

fn read_deadline(
    stream: &mut TcpStream,
    bytes: &mut [u8],
    deadline: Instant,
) -> std::io::Result<()> {
    let mut offset = 0;
    while offset < bytes.len() {
        let remaining = deadline
            .checked_duration_since(Instant::now())
            .filter(|d| !d.is_zero())
            .ok_or(std::io::ErrorKind::TimedOut)?;
        stream.set_read_timeout(Some(remaining))?;
        let read = stream.read(&mut bytes[offset..])?;
        if read == 0 {
            return Err(std::io::ErrorKind::UnexpectedEof.into());
        }
        offset += read;
    }
    Ok(())
}

pub fn serve_connection(
    stream: TcpStream,
    endpoint: &Endpoint,
    execute: &mut impl FnMut(Value) -> Result<Value, Error>,
) -> std::io::Result<()> {
    serve_diagnosed(stream, endpoint, execute).map_err(|failure| failure.error)
}

struct TransportFailure {
    stage: &'static str,
    error: std::io::Error,
}

fn stage<T>(name: &'static str, result: std::io::Result<T>) -> Result<T, TransportFailure> {
    result.map_err(|error| TransportFailure { stage: name, error })
}

fn serve_diagnosed(
    mut stream: TcpStream,
    endpoint: &Endpoint,
    execute: &mut impl FnMut(Value) -> Result<Value, Error>,
) -> Result<(), TransportFailure> {
    // BSD/macOS (and some other socket stacks) inherit the listener's
    // nonblocking mode on accept. This connection uses bounded blocking I/O;
    // otherwise a split frame is mistaken for a zero-millisecond timeout.
    stage("connection_mode", stream.set_nonblocking(false))?;
    let deadline = Instant::now() + Duration::from_secs(2);
    stage(
        "response_timeout",
        stream.set_write_timeout(Some(Duration::from_secs(2))),
    )?;
    // Mutual authentication: the app proves capability possession for the
    // client's fresh nonce before the client reveals anything sensitive.
    let hello = read_frame(&mut stream, deadline, "hello")?;
    let client = match serde_json::from_slice::<Hello>(&hello) {
        Ok(hello) if hello.protocol == PROTOCOL && is_nonce(&hello.nonce) => hello.nonce,
        _ => {
            let error = if serde_json::from_slice::<Value>(&hello)
                .ok()
                .and_then(|value| value.get("protocol")?.as_u64())
                .is_some_and(|protocol| protocol != u64::from(PROTOCOL))
            {
                incompatible()
            } else {
                Error::new("unauthorized", Value::Null)
            };
            let bytes = encode(&json!({"protocol": PROTOCOL, "ok": false, "error": error}));
            return write_frame(&mut stream, &bytes, "response");
        }
    };
    let server = new_nonce();
    let nonces = [client.as_bytes(), server.as_bytes()];
    let challenge = mac(&endpoint.token, CHALLENGE_PROOF, &nonces);
    write_frame(
        &mut stream,
        &encode(&json!({"protocol": PROTOCOL, "nonce": server, "proof": challenge})),
        "challenge",
    )?;
    let bytes = read_frame(&mut stream, deadline, "request")?;
    // The proof covers the exact transmitted request bytes; a relay that
    // forwards a valid proof with any other request is rejected undispatched.
    let envelope = serde_json::from_slice::<Envelope>(&bytes).ok();
    let raw = envelope
        .as_ref()
        .and_then(|envelope| BASE64.decode(&envelope.request).ok())
        .unwrap_or_default();
    let digest = Sha256::digest(&raw);
    let result = match envelope {
        Some(envelope)
            if authenticated(
                &envelope.proof,
                &mac(
                    &endpoint.token,
                    REQUEST_PROOF,
                    &[nonces[0], nonces[1], &digest],
                ),
            ) =>
        {
            if envelope.protocol != PROTOCOL {
                Err(incompatible())
            } else {
                match serde_json::from_slice::<Value>(&raw) {
                    Ok(request) => execute(request),
                    Err(_) => Err(Error::new("invalid_request", Value::Null)),
                }
            }
        }
        _ => Err(Error::new("unauthorized", Value::Null)),
    };
    let mut response = encode(&match result {
        Ok(value) => json!({"protocol": PROTOCOL, "ok": true, "result": value}),
        Err(error) => json!({"protocol": PROTOCOL, "ok": false, "error": error}),
    });
    // Base64 plus the envelope must still fit one frame.
    if response.len() > (MAX_FRAME - 256) / 4 * 3 {
        response = encode(&json!({
            "protocol": PROTOCOL, "ok": false,
            "error": {"code": "response_too_large", "detail": null}
        }));
    }
    let proof = mac(
        &endpoint.token,
        RESPONSE_PROOF,
        &[nonces[0], nonces[1], &digest, &Sha256::digest(&response)],
    );
    write_frame(
        &mut stream,
        &encode(&json!({
            "protocol": PROTOCOL, "proof": proof, "response": BASE64.encode(&response)
        })),
        "response",
    )
}

fn encode(value: &Value) -> Vec<u8> {
    serde_json::to_vec(value).expect("JSON values always serialize")
}

fn incompatible() -> Error {
    Error::new("incompatible_protocol", json!({"supported": PROTOCOL}))
}

fn read_frame(
    stream: &mut TcpStream,
    deadline: Instant,
    name: &'static str,
) -> Result<Vec<u8>, TransportFailure> {
    let mut header = [0u8; 12];
    stage(name, read_deadline(stream, &mut header, deadline))?;
    if &header[..8] != MAGIC {
        return stage("request_magic", Err(std::io::ErrorKind::InvalidData.into()));
    }
    let size = u32::from_be_bytes(header[8..].try_into().unwrap()) as usize;
    if size > MAX_FRAME {
        return stage("request_size", Err(std::io::ErrorKind::InvalidData.into()));
    }
    let mut bytes = vec![0; size];
    stage(name, read_deadline(stream, &mut bytes, deadline))?;
    Ok(bytes)
}

fn write_frame(
    stream: &mut TcpStream,
    bytes: &[u8],
    name: &'static str,
) -> Result<(), TransportFailure> {
    stage(name, stream.write_all(&(bytes.len() as u32).to_be_bytes()))?;
    stage(name, stream.write_all(bytes))
}

fn new_nonce() -> String {
    format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    )
}

fn is_nonce(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// HMAC-SHA256 over `len ‖ label` then each `len ‖ field` (u32 big-endian
/// lengths), so no two distinct label/field sequences share an input.
pub(crate) fn mac(token: &str, label: &[u8], fields: &[&[u8]]) -> String {
    let mut framed = Vec::new();
    for part in std::iter::once(label).chain(fields.iter().copied()) {
        framed.extend_from_slice(&(part.len() as u32).to_be_bytes());
        framed.extend_from_slice(part);
    }
    hmac(token.as_bytes(), &[&framed])
}

pub(crate) fn hmac(secret: &[u8], parts: &[&[u8]]) -> String {
    let mut key = [0u8; 64];
    if secret.len() > key.len() {
        key[..32].copy_from_slice(&Sha256::digest(secret));
    } else {
        key[..secret.len()].copy_from_slice(secret);
    }
    let mut inner = Sha256::new();
    inner.update(key.map(|b| b ^ 0x36));
    for part in parts {
        inner.update(part);
    }
    let mut outer = Sha256::new();
    outer.update(key.map(|b| b ^ 0x5c));
    outer.update(inner.finalize());
    format!("{:x}", outer.finalize())
}

/// Test-only reference client: performs the mutual handshake against
/// `serve_connection`, optionally tampering with the exact request bytes after
/// computing the proof (an active relay), and returns the verified reply.
#[cfg(test)]
pub(crate) fn client_exchange_with(
    stream: &mut TcpStream,
    token: &str,
    request: Value,
    tamper: impl FnOnce(&mut Vec<u8>),
) -> Result<Value, Value> {
    let nonce = new_nonce();
    client_send(stream, &json!({"protocol": PROTOCOL, "nonce": nonce}));
    let challenge = client_read(stream);
    let server = challenge["nonce"].as_str().unwrap().to_owned();
    let nonces = [nonce.as_bytes(), server.as_bytes()];
    assert!(authenticated(
        challenge["proof"].as_str().unwrap(),
        &mac(token, CHALLENGE_PROOF, &nonces)
    ));
    let mut raw = encode(&request);
    let digest = Sha256::digest(&raw);
    let proof = mac(token, REQUEST_PROOF, &[nonces[0], nonces[1], &digest]);
    tamper(&mut raw);
    client_send(
        stream,
        &json!({"protocol": PROTOCOL, "proof": proof, "request": BASE64.encode(&raw)}),
    );
    let frame = client_frame(stream);
    let response = BASE64.decode(frame["response"].as_str().unwrap()).unwrap();
    let expected = mac(
        token,
        RESPONSE_PROOF,
        &[nonces[0], nonces[1], &digest, &Sha256::digest(&response)],
    );
    let reply = serde_json::from_slice(&response).unwrap();
    if authenticated(frame["proof"].as_str().unwrap(), &expected) {
        Ok(reply)
    } else {
        Err(reply)
    }
}

#[cfg(test)]
pub(crate) fn client_exchange(stream: &mut TcpStream, token: &str, request: Value) -> Value {
    client_exchange_with(stream, token, request, |_| {}).expect("Verified native response")
}

#[cfg(test)]
pub(crate) fn client_send(stream: &mut TcpStream, value: &Value) {
    let bytes = encode(value);
    stream.write_all(MAGIC).unwrap();
    stream
        .write_all(&(bytes.len() as u32).to_be_bytes())
        .unwrap();
    stream.write_all(&bytes).unwrap();
}

#[cfg(test)]
pub(crate) fn client_frame(stream: &mut TcpStream) -> Value {
    let mut size = [0; 4];
    stream.read_exact(&mut size).unwrap();
    let size = u32::from_be_bytes(size) as usize;
    assert!(size <= MAX_FRAME);
    let mut bytes = vec![0; size];
    stream.read_exact(&mut bytes).unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

/// Reads one reply frame; a signed response is unwrapped without verification.
#[cfg(test)]
pub(crate) fn client_read(stream: &mut TcpStream) -> Value {
    let frame = client_frame(stream);
    match frame["response"].as_str() {
        Some(response) => serde_json::from_slice(&BASE64.decode(response).unwrap()).unwrap(),
        None => frame,
    }
}

#[cfg(test)]
pub(crate) const PROOF_LABELS: [&[u8]; 3] = [CHALLENGE_PROOF, REQUEST_PROOF, RESPONSE_PROOF];

fn authenticated(actual: &str, expected: &str) -> bool {
    if actual.len() != expected.len() {
        return false;
    }
    actual
        .bytes()
        .zip(expected.bytes())
        .fold(0, |difference, (a, b)| difference | (a ^ b))
        == 0
}

#[cfg(test)]
mod withdraw_tests {
    use super::*;

    fn publish(directory: &Path, token: char) -> Owned {
        let path = directory.join("endpoint.json");
        let endpoint = Endpoint {
            protocol: PROTOCOL,
            port: 1,
            token: token.to_string().repeat(64),
            app_version: "fixture".into(),
            pid: std::process::id(),
        };
        private_fs::replace(&path, &serde_json::to_vec(&endpoint).unwrap(), "fixture").unwrap();
        Owned {
            file: file_identity::identity(&path).unwrap().unwrap(),
            path,
            token: endpoint.token,
            pid: endpoint.pid,
        }
    }

    fn retired(directory: &Path) -> Vec<PathBuf> {
        fs::read_dir(directory)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.to_string_lossy().ends_with(".retired"))
            .collect()
    }

    #[test]
    fn raced_successor_is_restored_kept_or_superseded_never_deleted() {
        // Restored: the successor's descriptor returns to its name.
        let root = tempfile::tempdir().unwrap();
        let own = publish(root.path(), 'a');
        let mut successor = None;
        own.withdraw_with(
            || successor = Some(publish(root.path(), 'b')),
            |aside, path| fs::hard_link(aside, path),
        );
        assert!(successor
            .unwrap()
            .matches(&root.path().join("endpoint.json")));
        assert!(retired(root.path()).is_empty());

        // Restore fails and nothing newer exists: keep the moved file.
        let root = tempfile::tempdir().unwrap();
        let own = publish(root.path(), 'a');
        let mut successor = None;
        own.withdraw_with(
            || successor = Some(publish(root.path(), 'b')),
            |_, _| Err(std::io::ErrorKind::PermissionDenied.into()),
        );
        let kept = retired(root.path());
        assert_eq!(kept.len(), 1, "Successor descriptor kept aside");
        assert!(successor.unwrap().matches(&kept[0]));

        // Restore fails because an even newer descriptor exists: that one
        // stays and the superseded one is removed.
        let root = tempfile::tempdir().unwrap();
        let own = publish(root.path(), 'a');
        let mut newest = None;
        own.withdraw_with(
            || {
                publish(root.path(), 'b');
            },
            |_, _| {
                newest = Some(publish(root.path(), 'c'));
                Err(std::io::ErrorKind::AlreadyExists.into())
            },
        );
        assert!(newest.unwrap().matches(&root.path().join("endpoint.json")));
        assert!(retired(root.path()).is_empty());

        // Restore fails and the public name holds something that is not a
        // valid live descriptor: keep the moved successor.
        type Occupy = Box<dyn Fn(&Path)>;
        let occupants: Vec<(&str, Occupy)> = vec![
            (
                "malformed",
                Box::new(|path: &Path| fs::write(path, b"{not an endpoint").unwrap()),
            ),
            (
                "foreign protocol",
                Box::new(|path: &Path| {
                    fs::write(
                        path,
                        br#"{"protocol":1,"port":1,"token":"x","app_version":"0","pid":1}"#,
                    )
                    .unwrap()
                }),
            ),
            #[cfg(unix)]
            (
                "symlink to a valid descriptor",
                Box::new(|path: &Path| {
                    let target = path.with_file_name("elsewhere.json");
                    publish(path.parent().unwrap(), 'd');
                    fs::rename(path, &target).unwrap();
                    std::os::unix::fs::symlink(&target, path).unwrap();
                }),
            ),
        ];
        for (name, occupy) in occupants {
            let root = tempfile::tempdir().unwrap();
            let own = publish(root.path(), 'a');
            let mut successor = None;
            own.withdraw_with(
                || successor = Some(publish(root.path(), 'b')),
                |_, path| {
                    occupy(path);
                    Err(std::io::ErrorKind::AlreadyExists.into())
                },
            );
            let kept = retired(root.path());
            assert_eq!(kept.len(), 1, "{name}: successor kept aside");
            assert!(successor.unwrap().matches(&kept[0]), "{name}");
        }

        // Unraced: only this server's own descriptor is removed.
        let root = tempfile::tempdir().unwrap();
        publish(root.path(), 'a').withdraw();
        assert!(!root.path().join("endpoint.json").exists());
        assert!(retired(root.path()).is_empty());
    }
}
