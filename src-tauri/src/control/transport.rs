use super::Error;
use crate::storage::private_fs;
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
        Ok(Self {
            listener,
            endpoint,
            path,
        })
    }

    /// The calling thread is the single native executor. A separate acceptor
    /// hands each connection to a bounded pool that completes the handshake
    /// promptly, even while a long operation runs; only authenticated
    /// requests reach the executor's bounded queue.
    pub(crate) fn run(
        self,
        quitting: impl Fn() -> bool,
        mut execute: impl FnMut(Value) -> Result<Value, Error>,
    ) {
        let (jobs, queue) = mpsc::sync_channel::<Job>(QUEUE);
        let stop = Arc::new(AtomicBool::new(false));
        let failures = Arc::new(Mutex::new(Vec::new()));
        // Stops accepting and withdraws the descriptor however this thread
        // ends, so clients relaunch instead of reaching a dead executor.
        let _teardown = Teardown {
            stop: stop.clone(),
            path: self.path.clone(),
        };
        let acceptor = {
            let (stop, failures) = (stop.clone(), failures.clone());
            let endpoint = Arc::new(self.endpoint);
            let path = self.path.clone();
            let listener = self.listener;
            std::thread::Builder::new()
                .name("local-control-accept".into())
                .spawn(move || accept(listener, endpoint, path, jobs, stop, failures))
        };
        if acceptor.is_err() {
            return;
        }
        while !quitting() {
            match queue.recv_timeout(Duration::from_millis(25)) {
                Ok(job) => {
                    let started = Instant::now();
                    // Native commits are atomic replacements under the Store
                    // mutex and receipts are reserved before execution, so a
                    // panicking operation fails alone; the executor continues.
                    let result =
                        std::panic::catch_unwind(AssertUnwindSafe(|| execute(job.request)))
                            .unwrap_or_else(|_| {
                                note(
                                    &failures,
                                    &self.path,
                                    json!({"stage":"execute","kind":"Panic",
                                    "elapsed_ms":started.elapsed().as_millis()}),
                                );
                                Err(Error::new("internal_error", Value::Null))
                            });
                    let _ = job.reply.send(result);
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
    }
}

struct Teardown {
    stop: Arc<AtomicBool>,
    path: PathBuf,
}
impl Drop for Teardown {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        let _ = fs::remove_file(&self.path);
    }
}

/// Maximum concurrent connections (handshaking, queued or awaiting a reply).
const CONNECTIONS: usize = 16;
/// Authenticated requests waiting behind the running native operation.
pub(crate) const QUEUE: usize = 4;

struct Job {
    request: Value,
    reply: mpsc::Sender<Result<Value, Error>>,
}

fn accept(
    listener: TcpListener,
    endpoint: Arc<Endpoint>,
    path: PathBuf,
    jobs: mpsc::SyncSender<Job>,
    stop: Arc<AtomicBool>,
    failures: Arc<Mutex<Vec<Value>>>,
) {
    let active = Arc::new(AtomicUsize::new(0));
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
                let (endpoint, jobs, failures, path) = (
                    endpoint.clone(),
                    jobs.clone(),
                    failures.clone(),
                    path.clone(),
                );
                let _ = std::thread::Builder::new()
                    .name("local-control-connection".into())
                    .spawn(move || {
                        let _slot = slot;
                        let started = Instant::now();
                        let result = serve_diagnosed(stream, &endpoint, &mut |request| {
                            submit(&jobs, request)
                        });
                        if let Err(failure) = result {
                            record(&failures, &path, failure, started);
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
fn submit(jobs: &mpsc::SyncSender<Job>, request: Value) -> Result<Value, Error> {
    let (reply, result) = mpsc::channel();
    match jobs.try_send(Job { request, reply }) {
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
