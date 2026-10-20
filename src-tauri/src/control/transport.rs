use super::Error;
use crate::storage::private_fs;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    fs,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

pub const PROTOCOL: u32 = 1;
pub const MAX_FRAME: usize = 1024 * 1024;
const MAGIC: &[u8; 8] = b"SNIPER1\n";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Endpoint {
    pub protocol: u32,
    pub port: u16,
    pub token: String,
    pub app_version: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    protocol: u32,
    token: String,
    request: Value,
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

    pub(crate) fn run(
        self,
        quitting: impl Fn() -> bool,
        mut execute: impl FnMut(Value) -> Result<Value, Error>,
    ) {
        let mut failures = Vec::new();
        while !quitting() {
            match self.listener.accept() {
                Ok((stream, address)) if address.ip().is_loopback() => {
                    // A single bounded reader: unauthenticated clients never
                    // allocate workers or reach any application operation.
                    let started = Instant::now();
                    if let Err(failure) = serve_diagnosed(stream, &self.endpoint, &mut execute) {
                        failures.push(json!({
                            "stage":failure.stage,"kind":format!("{:?}",failure.error.kind()),
                            "elapsed_ms":started.elapsed().as_millis()
                        }));
                        if failures.len() > 16 {
                            failures.remove(0);
                        }
                        // Content-free and bounded; never include frame/token bytes.
                        let _ = private_fs::replace(
                            &self.path.with_file_name("transport-errors.json"),
                            &serde_json::to_vec(&failures).unwrap(),
                            "control transport diagnostics",
                        );
                    }
                }
                Ok(_) => {}
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(25));
                }
                Err(_) => break,
            }
        }
        let _ = fs::remove_file(&self.path);
    }
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
    let mut header = [0u8; 12];
    stage(
        "request_header",
        read_deadline(&mut stream, &mut header, deadline),
    )?;
    if &header[..8] != MAGIC {
        return stage("request_magic", Err(std::io::ErrorKind::InvalidData.into()));
    }
    let size = u32::from_be_bytes(header[8..].try_into().unwrap()) as usize;
    if size > MAX_FRAME {
        return stage("request_size", Err(std::io::ErrorKind::InvalidData.into()));
    }
    let mut bytes = vec![0; size];
    stage(
        "request_body",
        read_deadline(&mut stream, &mut bytes, deadline),
    )?;
    let result = match serde_json::from_slice::<Envelope>(&bytes) {
        Ok(envelope) if authenticated(&envelope.token, &endpoint.token) => {
            if envelope.protocol != PROTOCOL {
                Err(Error::new(
                    "incompatible_protocol",
                    json!({"supported": PROTOCOL}),
                ))
            } else {
                execute(envelope.request)
            }
        }
        _ => Err(Error::new("unauthorized", Value::Null)),
    };
    let response = match result {
        Ok(value) => json!({"protocol": PROTOCOL, "ok": true, "result": value}),
        Err(error) => json!({"protocol": PROTOCOL, "ok": false, "error": error}),
    };
    let mut bytes = stage(
        "response_encode",
        serde_json::to_vec(&response).map_err(std::io::Error::other),
    )?;
    if bytes.len() > MAX_FRAME {
        bytes = serde_json::to_vec(&json!({
            "protocol": PROTOCOL, "ok": false,
            "error": {"code": "response_too_large", "detail": null}
        }))
        .unwrap();
    }
    stage(
        "response_timeout",
        stream.set_write_timeout(Some(Duration::from_secs(2))),
    )?;
    stage(
        "response_header",
        stream.write_all(&(bytes.len() as u32).to_be_bytes()),
    )?;
    stage("response_body", stream.write_all(&bytes))
}

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
