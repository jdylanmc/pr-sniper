use oauth2::{
    basic::{BasicClient, BasicErrorResponseType, BasicTokenType},
    AuthType, ClientId, DeviceAuthorizationResponse, DeviceAuthorizationUrl,
    DeviceCodeErrorResponse, DeviceCodeErrorResponseType, EmptyExtraDeviceAuthorizationFields,
    RefreshToken, RequestTokenError, Scope, SyncHttpClient, TokenResponse, TokenUrl,
};
use reqwest::{blocking::Client, redirect::Policy};
use serde::Deserialize;
use std::{
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant, SystemTime},
};
use zeroize::Zeroizing;

pub const GITHUB_OAUTH_CLIENT_ID: &str = "Ov23lidoL3QovWyfxnA4";
pub const GITHUB_DEVICE_AUTHORIZATION_URL: &str = "https://github.com/login/device/code";
const GITHUB_TOKEN_URL: &str = "https://github.com/login/oauth/access_token";
const GITHUB_REFRESH_LIFETIME: Duration = Duration::from_secs(15_897_600);

pub struct PreparedDeviceAuthorization {
    response: DeviceAuthorizationResponse<EmptyExtraDeviceAuthorizationFields>,
    verification_uri: String,
}

impl PreparedDeviceAuthorization {
    pub fn user_code(&self) -> &str {
        self.response.user_code().secret()
    }

    pub fn verification_uri(&self) -> &str {
        &self.verification_uri
    }

    pub fn expires_in(&self) -> Duration {
        self.response.expires_in()
    }
}

impl std::fmt::Debug for PreparedDeviceAuthorization {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PreparedDeviceAuthorization")
            .field("verification_uri", &self.verification_uri)
            .field("expires_in", &self.expires_in())
            .finish_non_exhaustive()
    }
}

pub fn request_device_authorization_with<C, F>(
    http_client: &C,
    open_browser: F,
) -> Result<PreparedDeviceAuthorization, OAuthError>
where
    C: SyncHttpClient,
    F: FnOnce(&url::Url) -> Result<(), OAuthError>,
{
    request_device_authorization_for_with(http_client, open_browser, OAuthPurpose::Repository)
}

#[derive(Clone, Copy)]
pub enum OAuthPurpose {
    Repository,
    Copilot,
}

pub fn request_device_authorization_for_with<C, F>(
    http_client: &C,
    open_browser: F,
    purpose: OAuthPurpose,
) -> Result<PreparedDeviceAuthorization, OAuthError>
where
    C: SyncHttpClient,
    F: FnOnce(&url::Url) -> Result<(), OAuthError>,
{
    let diagnostic_http =
        |request| diagnostic_oauth_request(http_client, request, "device_authorization");
    let response: DeviceAuthorizationResponse<EmptyExtraDeviceAuthorizationFields> =
        device_oauth_client()?
            .exchange_device_code()
            .add_scope(Scope::new(
                match purpose {
                    OAuthPurpose::Repository => "repo",
                    OAuthPurpose::Copilot => "read:user",
                }
                .into(),
            ))
            .add_scope(Scope::new("offline_access".into()))
            .request(&diagnostic_http)
            .map_err(map_token_error)?;
    if response.user_code().secret().is_empty() || response.expires_in().is_zero() {
        return Err(OAuthError::InvalidResponse);
    }
    let verification_uri =
        validated_github_verification_uri(response.verification_uri().url().as_str())?;
    let browser_uri = response
        .verification_uri_complete()
        .and_then(|uri| validated_github_verification_uri(uri.secret()).ok())
        .unwrap_or_else(|| verification_uri.clone());
    open_browser(&browser_uri)?;
    eprintln!("[github-auth] stage=device_authorization outcome=browser_opened");
    Ok(PreparedDeviceAuthorization {
        response,
        verification_uri: verification_uri.to_string(),
    })
}

pub fn poll_device_authorization_with<C, S>(
    authorization: PreparedDeviceAuthorization,
    issued_at: SystemTime,
    http_client: &C,
    sleep: S,
    cancelled: &AtomicBool,
) -> Result<TokenPair, OAuthError>
where
    C: SyncHttpClient,
    S: Fn(Duration),
{
    let network_failure = AtomicBool::new(false);
    let adapted_http = |request| {
        if cancelled.load(Ordering::SeqCst) {
            eprintln!("[github-auth] stage=device_poll reason=cancelled");
            return Ok(oauth2::http::Response::builder()
                .status(oauth2::http::StatusCode::BAD_REQUEST)
                .header("content-type", "application/json")
                .body(br#"{"error":"access_denied"}"#.to_vec())
                .expect("static cancellation response is valid"));
        }
        let response = diagnostic_oauth_request(http_client, request, "device_poll");
        network_failure.store(response.is_err(), Ordering::SeqCst);
        response
    };
    let timeout = authorization.response.expires_in();
    let response = device_oauth_client()?
        .exchange_device_access_token(&authorization.response)
        .set_max_backoff_interval(timeout)
        .request(&adapted_http, sleep, Some(timeout))
        .map_err(|error| {
            map_device_poll_error::<C>(
                error,
                cancelled.load(Ordering::SeqCst),
                network_failure.load(Ordering::SeqCst),
            )
        })?;
    token_pair(response, issued_at)
}

fn map_device_poll_error<C: SyncHttpClient>(
    error: RequestTokenError<C::Error, DeviceCodeErrorResponse>,
    cancelled: bool,
    network_failure: bool,
) -> OAuthError {
    if cancelled {
        return OAuthError::Cancelled;
    }
    match error {
        RequestTokenError::Request(_) => OAuthError::Network,
        RequestTokenError::ServerResponse(error) => match error.error() {
            DeviceCodeErrorResponseType::AccessDenied => OAuthError::Denied,
            DeviceCodeErrorResponseType::ExpiredToken if network_failure => OAuthError::Network,
            DeviceCodeErrorResponseType::ExpiredToken => OAuthError::Expired,
            _ => OAuthError::Provider,
        },
        RequestTokenError::Parse(_, _) | RequestTokenError::Other(_) => OAuthError::InvalidResponse,
    }
}

fn validated_github_verification_uri(value: &str) -> Result<url::Url, OAuthError> {
    let url = url::Url::parse(value).map_err(|_| OAuthError::InvalidResponse)?;
    if url.scheme() != "https" || url.host_str() != Some("github.com") {
        return Err(OAuthError::InvalidResponse);
    }
    Ok(url)
}

pub struct GithubOAuthHttp {
    client: Client,
}

impl GithubOAuthHttp {
    pub fn new() -> Result<Self, OAuthError> {
        let client = Client::builder()
            .user_agent("PR-Sniper/0.1")
            .redirect(Policy::none())
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|_| OAuthError::Network)?;
        Ok(Self { client })
    }

    pub fn request_device_authorization(
        &self,
        open_browser: impl FnOnce(&url::Url) -> Result<(), OAuthError>,
    ) -> Result<PreparedDeviceAuthorization, OAuthError> {
        request_device_authorization_with(&self.client, open_browser)
    }

    pub fn request_copilot_authorization(
        &self,
        open_browser: impl FnOnce(&url::Url) -> Result<(), OAuthError>,
    ) -> Result<PreparedDeviceAuthorization, OAuthError> {
        request_device_authorization_for_with(&self.client, open_browser, OAuthPurpose::Copilot)
    }

    pub fn poll_device_authorization(
        &self,
        authorization: PreparedDeviceAuthorization,
        cancelled: &AtomicBool,
    ) -> Result<TokenPair, OAuthError> {
        poll_device_authorization_with(
            authorization,
            SystemTime::now(),
            &self.client,
            |duration| Self::interruptible_sleep(duration, cancelled),
            cancelled,
        )
    }

    pub fn refresh(&self, refresh_token: &str) -> Result<TokenPair, OAuthError> {
        refresh_token_with(
            RefreshToken::new(refresh_token.into()),
            SystemTime::now(),
            &self.client,
        )
    }

    fn interruptible_sleep(duration: Duration, cancelled: &AtomicBool) {
        let deadline = Instant::now() + duration;
        while !cancelled.load(Ordering::SeqCst) {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }
            std::thread::sleep(remaining.min(Duration::from_millis(50)));
        }
    }
}

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

pub fn refresh_token_with<C: SyncHttpClient>(
    refresh_token: RefreshToken,
    issued_at: SystemTime,
    http_client: &C,
) -> Result<TokenPair, OAuthError> {
    let diagnostic_http = |request| diagnostic_oauth_request(http_client, request, "refresh");
    let response = device_oauth_client()?
        .exchange_refresh_token(&refresh_token)
        .request(&diagnostic_http)
        .map_err(map_token_error)?;
    token_pair(response, issued_at)
}

pub async fn refresh_token_async(refresh: &str) -> Result<TokenPair, OAuthError> {
    let issued_at = SystemTime::now();
    let client = reqwest::Client::builder()
        .user_agent("PR-Sniper/0.1")
        .redirect(Policy::none())
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|_| OAuthError::Network)?;
    let diagnostic_http = |request| {
        let client = client.clone();
        async move {
            let mut response = oauth2::AsyncHttpClient::call(&client, request).await?;
            normalize_oauth_response(&mut response, "refresh");
            Ok::<_, oauth2::HttpClientError<reqwest::Error>>(response)
        }
    };
    let refresh = RefreshToken::new(refresh.into());
    let response = device_oauth_client()?
        .exchange_refresh_token(&refresh)
        .request_async(&diagnostic_http)
        .await
        .map_err(map_token_error)?;
    token_pair(response, issued_at)
}

// Diagnostics expose only response shape and allow-listed provider error categories.
#[derive(Deserialize)]
struct DiagnosticTokenFields {
    access_token: Option<serde::de::IgnoredAny>,
    refresh_token: Option<serde::de::IgnoredAny>,
    expires_in: Option<serde::de::IgnoredAny>,
    refresh_token_expires_in: Option<serde::de::IgnoredAny>,
    token_type: Option<serde::de::IgnoredAny>,
    error: Option<DiagnosticProviderError>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum DiagnosticProviderError {
    AuthorizationPending,
    SlowDown,
    ExpiredToken,
    BadVerificationCode,
    IncorrectClientCredentials,
    RedirectUriMismatch,
    DeviceFlowDisabled,
    UnauthorizedClient,
    UnsupportedGrantType,
    InvalidGrant,
    InvalidClient,
    InvalidRequest,
    AccessDenied,
    #[serde(other)]
    Other,
}

fn token_response_diagnostic(response: &oauth2::HttpResponse) -> String {
    let content_type = match response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.split(';').next())
    {
        Some("application/json") => "json",
        Some("application/x-www-form-urlencoded") => "form",
        Some(_) => "other",
        None => "missing",
    };
    match serde_json::from_slice::<DiagnosticTokenFields>(response.body()) {
        Ok(fields) => format!(
            "status={} content_type={content_type} json_shape=true access_token_present={} refresh_token_present={} expires_in_present={} refresh_expiry_present={} token_type_present={} provider_error={:?}",
            response.status().as_u16(),
            fields.access_token.is_some(),
            fields.refresh_token.is_some(),
            fields.expires_in.is_some(),
            fields.refresh_token_expires_in.is_some(),
            fields.token_type.is_some(),
            fields.error
        ),
        Err(_) => format!(
            "status={} content_type={content_type} json_shape=false",
            response.status().as_u16()
        ),
    }
}

fn diagnostic_oauth_request<C: SyncHttpClient>(
    client: &C,
    request: oauth2::HttpRequest,
    stage: &str,
) -> Result<oauth2::HttpResponse, C::Error> {
    let mut response = client.call(request);
    match &mut response {
        Ok(response) => normalize_oauth_response(response, stage),
        Err(_) => eprintln!("[github-auth] stage={stage}_http_response reason=network"),
    }
    response
}

fn normalize_oauth_response(response: &mut oauth2::HttpResponse, stage: &str) {
    eprintln!(
        "[github-auth] stage={stage}_http_response {}",
        token_response_diagnostic(response)
    );
    let provider_error = serde_json::from_slice::<DiagnosticTokenFields>(response.body())
        .ok()
        .and_then(|fields| fields.error);
    if response.status().is_success() && provider_error.is_some() {
        *response.status_mut() = oauth2::http::StatusCode::BAD_REQUEST;
    }
}

fn device_oauth_client() -> Result<
    BasicClient<
        oauth2::EndpointNotSet,
        oauth2::EndpointSet,
        oauth2::EndpointNotSet,
        oauth2::EndpointNotSet,
        oauth2::EndpointSet,
    >,
    OAuthError,
> {
    Ok(
        BasicClient::new(ClientId::new(GITHUB_OAUTH_CLIENT_ID.into()))
            .set_auth_type(AuthType::RequestBody)
            .set_device_authorization_url(
                DeviceAuthorizationUrl::new(GITHUB_DEVICE_AUTHORIZATION_URL.into())
                    .map_err(|_| OAuthError::InvalidResponse)?,
            )
            .set_token_uri(
                TokenUrl::new(GITHUB_TOKEN_URL.into()).map_err(|_| OAuthError::InvalidResponse)?,
            ),
    )
}

fn map_token_error<E: std::error::Error + 'static>(
    error: RequestTokenError<E, oauth2::basic::BasicErrorResponse>,
) -> OAuthError {
    match error {
        RequestTokenError::Request(_) => OAuthError::Network,
        RequestTokenError::ServerResponse(error)
            if matches!(
                error.error(),
                BasicErrorResponseType::Extension(code) if code == "device_flow_disabled"
            ) =>
        {
            OAuthError::DeviceFlowDisabled
        }
        RequestTokenError::ServerResponse(_) => OAuthError::Provider,
        RequestTokenError::Parse(_, _) => {
            eprintln!("[github-auth] stage=token_decode reason=parse_failure");
            OAuthError::InvalidResponse
        }
        RequestTokenError::Other(_) => {
            eprintln!("[github-auth] stage=token_decode reason=unexpected_response");
            OAuthError::InvalidResponse
        }
    }
}

fn token_pair(
    response: oauth2::StandardTokenResponse<oauth2::EmptyExtraTokenFields, BasicTokenType>,
    issued_at: SystemTime,
) -> Result<TokenPair, OAuthError> {
    if response.token_type() != &BasicTokenType::Bearer {
        return Err(OAuthError::InvalidResponse);
    }
    let access_lifetime = response
        .expires_in()
        .filter(|duration| !duration.is_zero())
        .ok_or_else(|| {
            eprintln!("[github-auth] stage=token_validation reason=missing_or_zero_access_expiry");
            OAuthError::InvalidResponse
        })?;
    let refresh_token = response
        .refresh_token()
        .map(|token| token.secret())
        .filter(|token| !token.is_empty())
        .ok_or_else(|| {
            eprintln!("[github-auth] stage=token_validation reason=missing_or_empty_refresh_token");
            OAuthError::InvalidResponse
        })?;
    if response.access_token().secret().is_empty() {
        eprintln!("[github-auth] stage=token_validation reason=empty_access_token");
        return Err(OAuthError::InvalidResponse);
    }
    eprintln!("[github-auth] stage=token_validation outcome=success");
    Ok(TokenPair::new_at(
        response.access_token().secret(),
        refresh_token,
        issued_at,
        access_lifetime,
        GITHUB_REFRESH_LIFETIME,
    ))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OAuthError {
    InvalidResponse,
    Network,
    BrowserOpen,
    Cancelled,
    Timeout,
    Denied,
    DeviceFlowDisabled,
    Expired,
    Provider,
}

#[cfg(test)]
mod diagnostic_tests {
    use super::*;

    #[test]
    fn response_diagnostics_report_shape_without_response_values() {
        let response = oauth2::http::Response::builder()
            .status(200)
            .header("content-type", "application/json; charset=utf-8")
            .body(
                br#"{"access_token":"access-secret","refresh_token":"refresh-secret","expires_in":28800,"refresh_token_expires_in":15897600,"token_type":"bearer","state":"state-secret","code":"code-secret","error_description":"private-detail"}"#
                    .to_vec(),
            )
            .unwrap();
        let diagnostic = token_response_diagnostic(&response);
        assert!(diagnostic.contains("status=200 content_type=json json_shape=true"));
        assert!(diagnostic.contains("access_token_present=true refresh_token_present=true"));
        assert!(diagnostic.contains("expires_in_present=true refresh_expiry_present=true"));
        for secret in [
            "access-secret",
            "refresh-secret",
            "state-secret",
            "code-secret",
            "private-detail",
        ] {
            assert!(!diagnostic.contains(secret));
        }
    }

    #[test]
    fn response_diagnostics_allowlist_provider_codes_and_discard_unknown_data() {
        for (body, expected) in [
            (
                r#"{"error":"incorrect_client_credentials","error_description":"secret-detail"}"#,
                "provider_error=Some(IncorrectClientCredentials)",
            ),
            (
                r#"{"error":"device_flow_disabled","error_description":"secret-detail"}"#,
                "provider_error=Some(DeviceFlowDisabled)",
            ),
            (
                r#"{"error":"secret-detail","error_description":"secret-detail"}"#,
                "provider_error=Some(Other)",
            ),
            ("secret-detail", "json_shape=false"),
        ] {
            let response = oauth2::http::Response::builder()
                .status(400)
                .header("content-type", "secret-content-type")
                .body(body.as_bytes().to_vec())
                .unwrap();
            let diagnostic = token_response_diagnostic(&response);
            assert!(diagnostic.contains(expected));
            assert!(!diagnostic.contains("secret-detail"));
            assert!(!diagnostic.contains("secret-content-type"));
        }
    }
}
