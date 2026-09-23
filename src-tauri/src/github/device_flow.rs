use serde::Serialize;
use std::time::{Duration, SystemTime};
use zeroize::Zeroizing;

pub const GITHUB_APP_CLIENT_ID: &str = "Iv23li1HXvoQVkSzV2l5";

#[derive(Clone, PartialEq, Eq)]
pub struct TokenPair {
    access_token: Zeroizing<String>,
    refresh_token: Zeroizing<String>,
    access_expires_at: SystemTime,
    refresh_expires_at: SystemTime,
}

impl TokenPair {
    pub fn new(
        access_token: impl Into<String>,
        refresh_token: impl Into<String>,
        access_lifetime: Duration,
        refresh_lifetime: Duration,
    ) -> Self {
        Self::new_at(
            access_token,
            refresh_token,
            SystemTime::now(),
            access_lifetime,
            refresh_lifetime,
        )
    }

    pub fn new_at(
        access_token: impl Into<String>,
        refresh_token: impl Into<String>,
        issued_at: SystemTime,
        access_lifetime: Duration,
        refresh_lifetime: Duration,
    ) -> Self {
        Self {
            access_token: Zeroizing::new(access_token.into()),
            refresh_token: Zeroizing::new(refresh_token.into()),
            access_expires_at: issued_at.checked_add(access_lifetime).unwrap_or(issued_at),
            refresh_expires_at: issued_at.checked_add(refresh_lifetime).unwrap_or(issued_at),
        }
    }

    pub fn from_expirations(
        access_token: impl Into<String>,
        refresh_token: impl Into<String>,
        access_expires_at: SystemTime,
        refresh_expires_at: SystemTime,
    ) -> Self {
        Self {
            access_token: Zeroizing::new(access_token.into()),
            refresh_token: Zeroizing::new(refresh_token.into()),
            access_expires_at,
            refresh_expires_at,
        }
    }

    pub fn access_token(&self) -> &str {
        self.access_token.as_str()
    }

    pub fn refresh_token(&self) -> &str {
        self.refresh_token.as_str()
    }

    pub fn access_expires_at(&self) -> SystemTime {
        self.access_expires_at
    }

    pub fn refresh_expires_at(&self) -> SystemTime {
        self.refresh_expires_at
    }

    pub fn access_is_expired(&self, now: SystemTime) -> bool {
        now >= self.access_expires_at
    }

    pub fn refresh_is_expired(&self, now: SystemTime) -> bool {
        now >= self.refresh_expires_at
    }
}

impl std::fmt::Debug for TokenPair {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("TokenPair([REDACTED])")
    }
}

pub struct DeviceAuthorization {
    device_code: Zeroizing<String>,
    prompt: DevicePrompt,
}

impl DeviceAuthorization {
    pub fn new(
        device_code: impl Into<String>,
        user_code: impl Into<String>,
        verification_uri: impl Into<String>,
        expires_in: Duration,
        interval: Duration,
    ) -> Self {
        Self {
            device_code: Zeroizing::new(device_code.into()),
            prompt: DevicePrompt {
                user_code: user_code.into(),
                verification_uri: verification_uri.into(),
                expires_in,
                interval,
            },
        }
    }

    pub fn prompt(&self) -> &DevicePrompt {
        &self.prompt
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DevicePrompt {
    pub user_code: String,
    pub verification_uri: String,
    #[serde(serialize_with = "serialize_seconds")]
    pub expires_in: Duration,
    #[serde(serialize_with = "serialize_seconds")]
    pub interval: Duration,
}

fn serialize_seconds<S>(duration: &Duration, serializer: S) -> Result<S::Ok, S::Error>
where
    S: serde::Serializer,
{
    serializer.serialize_u64(duration.as_secs())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceFlowError {
    InvalidResponse,
    Network,
    Provider,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProviderPoll {
    AuthorizationPending,
    SlowDown,
    AccessDenied,
    ExpiredToken,
    Authorized(TokenPair),
}

pub trait DeviceFlowTransport {
    fn begin(&self, client_id: &str) -> Result<DeviceAuthorization, DeviceFlowError>;
    fn poll(&self, client_id: &str, device_code: &str) -> Result<ProviderPoll, DeviceFlowError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceFlowPoll {
    WaitUntil(Duration),
    Pending { retry_at: Duration },
    Authorized(TokenPair),
    Denied,
    Expired,
    Cancelled,
    Failed(DeviceFlowError),
}

enum Terminal {
    Authorized(TokenPair),
    Denied,
    Expired,
    Cancelled,
    Failed(DeviceFlowError),
}

pub struct DeviceFlow<T> {
    transport: T,
    device_code: Zeroizing<String>,
    prompt: DevicePrompt,
    expires_at: Duration,
    interval: Duration,
    next_poll_at: Duration,
    terminal: Option<Terminal>,
}

impl<T: DeviceFlowTransport> DeviceFlow<T> {
    pub fn begin(transport: T, now: Duration) -> Result<Self, DeviceFlowError> {
        let authorization = transport.begin(GITHUB_APP_CLIENT_ID)?;
        if authorization.device_code.is_empty()
            || authorization.prompt.user_code.is_empty()
            || authorization.prompt.verification_uri != "https://github.com/login/device"
            || authorization.prompt.expires_in.is_zero()
            || authorization.prompt.interval.is_zero()
        {
            return Err(DeviceFlowError::InvalidResponse);
        }
        let prompt = authorization.prompt;
        let interval = prompt.interval;
        Ok(Self {
            transport,
            device_code: authorization.device_code,
            expires_at: now.saturating_add(prompt.expires_in),
            next_poll_at: now.saturating_add(interval),
            prompt,
            interval,
            terminal: None,
        })
    }

    pub fn prompt(&self) -> &DevicePrompt {
        &self.prompt
    }

    pub fn cancel(&mut self) {
        if self.terminal.is_none() {
            self.terminal = Some(Terminal::Cancelled);
        }
    }

    pub fn poll(&mut self, now: Duration) -> DeviceFlowPoll {
        if let Some(terminal) = &self.terminal {
            return terminal.result();
        }
        if now >= self.expires_at {
            self.terminal = Some(Terminal::Expired);
            return DeviceFlowPoll::Expired;
        }
        if now < self.next_poll_at {
            return DeviceFlowPoll::WaitUntil(self.next_poll_at);
        }
        match self
            .transport
            .poll(GITHUB_APP_CLIENT_ID, self.device_code.as_str())
        {
            Ok(ProviderPoll::AuthorizationPending) => {
                self.next_poll_at = now.saturating_add(self.interval);
                DeviceFlowPoll::Pending {
                    retry_at: self.next_poll_at,
                }
            }
            Ok(ProviderPoll::SlowDown) => {
                self.interval = self.interval.saturating_add(Duration::from_secs(5));
                self.next_poll_at = now.saturating_add(self.interval);
                DeviceFlowPoll::Pending {
                    retry_at: self.next_poll_at,
                }
            }
            Ok(ProviderPoll::Authorized(pair)) => {
                self.terminal = Some(Terminal::Authorized(pair.clone()));
                DeviceFlowPoll::Authorized(pair)
            }
            Ok(ProviderPoll::AccessDenied) => {
                self.terminal = Some(Terminal::Denied);
                DeviceFlowPoll::Denied
            }
            Ok(ProviderPoll::ExpiredToken) => {
                self.terminal = Some(Terminal::Expired);
                DeviceFlowPoll::Expired
            }
            Err(error) => {
                self.terminal = Some(Terminal::Failed(error));
                DeviceFlowPoll::Failed(error)
            }
        }
    }
}

impl Terminal {
    fn result(&self) -> DeviceFlowPoll {
        match self {
            Self::Authorized(pair) => DeviceFlowPoll::Authorized(pair.clone()),
            Self::Denied => DeviceFlowPoll::Denied,
            Self::Expired => DeviceFlowPoll::Expired,
            Self::Cancelled => DeviceFlowPoll::Cancelled,
            Self::Failed(error) => DeviceFlowPoll::Failed(*error),
        }
    }
}
