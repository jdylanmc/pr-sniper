use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::Html,
    routing::get,
    Router,
};
use oauth2::{
    basic::{BasicClient, BasicTokenType},
    AuthType, AuthUrl, AuthorizationCode, ClientId, CsrfToken, PkceCodeChallenge, PkceCodeVerifier,
    RedirectUrl, RefreshToken, RequestTokenError, SyncHttpClient, TokenResponse, TokenUrl,
};
use reqwest::{blocking::Client, redirect::Policy};
use serde::Deserialize;
use std::time::{Duration, SystemTime};
use std::{
    net::{SocketAddr, TcpListener},
    sync::{Arc, Mutex},
    time::Instant,
};
use zeroize::Zeroizing;

pub const GITHUB_APP_CLIENT_ID: &str = "Iv23li1HXvoQVkSzV2l5";
pub const GITHUB_CALLBACK_URL: &str = "http://127.0.0.1:53682/oauth/github/callback";
const GITHUB_AUTHORIZE_URL: &str = "https://github.com/login/oauth/authorize";
const GITHUB_TOKEN_URL: &str = "https://github.com/login/oauth/access_token";
const GITHUB_REFRESH_LIFETIME: Duration = Duration::from_secs(15_897_600);
pub const GITHUB_CALLBACK_ADDRESS: &str = "127.0.0.1:53682";
pub const GITHUB_AUTH_TIMEOUT: Duration = Duration::from_secs(300);

pub struct AuthorizationAttempt {
    authorization_url: url::Url,
    state: CsrfToken,
    verifier: PkceCodeVerifier,
}

impl AuthorizationAttempt {
    pub fn new(select_account: bool) -> Result<Self, OAuthError> {
        Self::new_with_redirect(select_account, GITHUB_CALLBACK_URL)
    }

    fn new_with_redirect(select_account: bool, redirect_url: &str) -> Result<Self, OAuthError> {
        let client = BasicClient::new(ClientId::new(GITHUB_APP_CLIENT_ID.into()))
            .set_auth_type(AuthType::RequestBody)
            .set_auth_uri(
                AuthUrl::new(GITHUB_AUTHORIZE_URL.into())
                    .map_err(|_| OAuthError::InvalidResponse)?,
            )
            .set_token_uri(
                TokenUrl::new(GITHUB_TOKEN_URL.into()).map_err(|_| OAuthError::InvalidResponse)?,
            )
            .set_redirect_uri(
                RedirectUrl::new(redirect_url.into()).map_err(|_| OAuthError::InvalidResponse)?,
            );
        let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
        let request = client
            .authorize_url(CsrfToken::new_random)
            .set_pkce_challenge(challenge);
        let (authorization_url, state) = if select_account {
            request.add_extra_param("prompt", "select_account").url()
        } else {
            request.url()
        };
        Ok(Self {
            authorization_url,
            state,
            verifier,
        })
    }

    pub fn authorization_url(&self) -> &url::Url {
        &self.authorization_url
    }

    pub fn state(&self) -> &CsrfToken {
        &self.state
    }

    pub fn into_verifier(self) -> PkceCodeVerifier {
        self.verifier
    }
}

pub struct PreparedAuthorization {
    pub attempt: AuthorizationAttempt,
    pub listener: TcpListener,
    pub deadline: Instant,
}

pub struct AuthorizationCompletion {
    pub code: AuthorizationCode,
    pub verifier: PkceCodeVerifier,
}

pub async fn receive_callback(
    prepared: PreparedAuthorization,
    mut cancel: tokio::sync::oneshot::Receiver<()>,
) -> Result<AuthorizationCompletion, OAuthError> {
    let PreparedAuthorization {
        attempt,
        listener,
        deadline,
    } = prepared;
    let AuthorizationAttempt {
        state, verifier, ..
    } = attempt;
    let listener = tokio::net::TcpListener::from_std(listener).map_err(|_| OAuthError::Bind)?;
    let (result_tx, mut result_rx) = tokio::sync::oneshot::channel();
    let callback_state = CallbackState {
        guard: Mutex::new(CallbackGuard::new(state)),
        result: Mutex::new(Some(result_tx)),
    };
    let app = Router::new()
        .route("/oauth/github/callback", get(callback))
        .with_state(Arc::new(callback_state));
    let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let _ = axum::serve(listener, app)
            .with_graceful_shutdown(async {
                let _ = shutdown_rx.await;
            })
            .await;
    });

    let result = tokio::select! {
        result = &mut result_rx => result.unwrap_or(Err(OAuthError::InvalidResponse)),
        _ = &mut cancel => Err(OAuthError::Cancelled),
        _ = tokio::time::sleep_until(tokio::time::Instant::from_std(deadline)) => {
            Err(OAuthError::Timeout)
        }
    };
    let _ = shutdown_tx.send(());
    let _ = server.await;
    result.map(|code| AuthorizationCompletion { code, verifier })
}

struct CallbackState {
    guard: Mutex<CallbackGuard>,
    result: Mutex<Option<tokio::sync::oneshot::Sender<Result<AuthorizationCode, OAuthError>>>>,
}

async fn callback(
    State(state): State<Arc<CallbackState>>,
    Query(parameters): Query<CallbackParameters>,
) -> (StatusCode, Html<&'static str>) {
    let result = state
        .guard
        .lock()
        .map_err(|_| OAuthError::InvalidResponse)
        .and_then(|mut guard| guard.accept(parameters));
    if let Ok(mut result_sender) = state.result.lock() {
        if let Some(result_sender) = result_sender.take() {
            let _ = result_sender.send(result.clone());
        }
    }
    match result {
        Ok(_) => (
            StatusCode::OK,
            Html(
                "<!doctype html><title>Connected</title><p>Connected - return to PR Sniper. You may close this window.</p>",
            ),
        ),
        Err(_) => (
            StatusCode::BAD_REQUEST,
            Html(
                "<!doctype html><title>Connection failed</title><p>Could not connect. Return to PR Sniper and try again.</p>",
            ),
        ),
    }
}

pub fn prepare_authorization_with<F>(
    address: SocketAddr,
    redirect_url: &str,
    select_account: bool,
    open_browser: F,
) -> Result<PreparedAuthorization, OAuthError>
where
    F: FnOnce(&url::Url) -> Result<(), ()>,
{
    if !address.ip().is_loopback() {
        return Err(OAuthError::Bind);
    }
    let listener = TcpListener::bind(address).map_err(|_| OAuthError::Bind)?;
    listener
        .set_nonblocking(true)
        .map_err(|_| OAuthError::Bind)?;
    let attempt = AuthorizationAttempt::new_with_redirect(select_account, redirect_url)?;
    open_browser(attempt.authorization_url()).map_err(|_| OAuthError::BrowserOpen)?;
    Ok(PreparedAuthorization {
        attempt,
        listener,
        deadline: Instant::now() + GITHUB_AUTH_TIMEOUT,
    })
}

pub fn prepare_authorization(select_account: bool) -> Result<PreparedAuthorization, OAuthError> {
    let address = GITHUB_CALLBACK_ADDRESS
        .parse()
        .map_err(|_| OAuthError::Bind)?;
    prepare_authorization_with(
        address,
        GITHUB_CALLBACK_URL,
        select_account,
        |authorization_url| webbrowser::open(authorization_url.as_str()).map_err(|_| ()),
    )
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

    pub fn exchange(&self, completion: AuthorizationCompletion) -> Result<TokenPair, OAuthError> {
        exchange_code_with(
            completion.code,
            completion.verifier,
            SystemTime::now(),
            &self.client,
        )
    }

    pub fn refresh(&self, refresh_token: &str) -> Result<TokenPair, OAuthError> {
        refresh_token_with(
            RefreshToken::new(refresh_token.into()),
            SystemTime::now(),
            &self.client,
        )
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

pub fn exchange_code_with<C: SyncHttpClient>(
    code: AuthorizationCode,
    verifier: PkceCodeVerifier,
    issued_at: SystemTime,
    http_client: &C,
) -> Result<TokenPair, OAuthError> {
    let response = oauth_client()?
        .exchange_code(code)
        .set_pkce_verifier(verifier)
        .request(http_client)
        .map_err(map_token_error::<C>)?;
    token_pair(response, issued_at)
}

pub fn refresh_token_with<C: SyncHttpClient>(
    refresh_token: RefreshToken,
    issued_at: SystemTime,
    http_client: &C,
) -> Result<TokenPair, OAuthError> {
    let response = oauth_client()?
        .exchange_refresh_token(&refresh_token)
        .request(http_client)
        .map_err(map_token_error::<C>)?;
    token_pair(response, issued_at)
}

fn oauth_client() -> Result<
    BasicClient<
        oauth2::EndpointSet,
        oauth2::EndpointNotSet,
        oauth2::EndpointNotSet,
        oauth2::EndpointNotSet,
        oauth2::EndpointSet,
    >,
    OAuthError,
> {
    Ok(BasicClient::new(ClientId::new(GITHUB_APP_CLIENT_ID.into()))
        .set_auth_type(AuthType::RequestBody)
        .set_auth_uri(
            AuthUrl::new(GITHUB_AUTHORIZE_URL.into()).map_err(|_| OAuthError::InvalidResponse)?,
        )
        .set_token_uri(
            TokenUrl::new(GITHUB_TOKEN_URL.into()).map_err(|_| OAuthError::InvalidResponse)?,
        )
        .set_redirect_uri(
            RedirectUrl::new(GITHUB_CALLBACK_URL.into())
                .map_err(|_| OAuthError::InvalidResponse)?,
        ))
}

fn map_token_error<C: SyncHttpClient>(
    error: RequestTokenError<C::Error, oauth2::basic::BasicErrorResponse>,
) -> OAuthError {
    match error {
        RequestTokenError::Request(_) => OAuthError::Network,
        RequestTokenError::ServerResponse(_) => OAuthError::Provider,
        RequestTokenError::Parse(_, _) | RequestTokenError::Other(_) => OAuthError::InvalidResponse,
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
        .ok_or(OAuthError::InvalidResponse)?;
    let refresh_token = response
        .refresh_token()
        .map(|token| token.secret())
        .filter(|token| !token.is_empty())
        .ok_or(OAuthError::InvalidResponse)?;
    if response.access_token().secret().is_empty() {
        return Err(OAuthError::InvalidResponse);
    }
    Ok(TokenPair::new_at(
        response.access_token().secret(),
        refresh_token,
        issued_at,
        access_lifetime,
        GITHUB_REFRESH_LIFETIME,
    ))
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
pub struct CallbackParameters {
    pub code: Option<String>,
    pub state: Option<String>,
    pub error: Option<String>,
}

pub struct CallbackGuard {
    expected_state: CsrfToken,
    used: bool,
}

impl CallbackGuard {
    pub fn new(expected_state: CsrfToken) -> Self {
        Self {
            expected_state,
            used: false,
        }
    }

    pub fn accept(
        &mut self,
        parameters: CallbackParameters,
    ) -> Result<AuthorizationCode, OAuthError> {
        if self.used {
            return Err(OAuthError::Replayed);
        }
        self.used = true;
        let state = parameters
            .state
            .filter(|state| !state.is_empty())
            .ok_or(OAuthError::StateMismatch)?;
        if CsrfToken::new(state) != self.expected_state {
            return Err(OAuthError::StateMismatch);
        }
        if parameters.error.is_some() {
            return Err(OAuthError::Provider);
        }
        let code = parameters
            .code
            .filter(|code| !code.is_empty())
            .ok_or(OAuthError::InvalidResponse)?;
        Ok(AuthorizationCode::new(code))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OAuthError {
    InvalidResponse,
    Network,
    Bind,
    BrowserOpen,
    Cancelled,
    Timeout,
    Provider,
    StateMismatch,
    Replayed,
}
