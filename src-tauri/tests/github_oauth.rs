use pr_sniper_lib::github::oauth::{
    exchange_code_with, prepare_authorization_with, receive_callback, AuthorizationAttempt,
    CallbackGuard, CallbackParameters, OAuthError, GITHUB_CALLBACK_URL,
};
use std::time::{Duration, UNIX_EPOCH};
use std::{collections::BTreeMap, net::TcpListener};

fn query(url: &url::Url) -> BTreeMap<String, String> {
    url.query_pairs().into_owned().collect()
}

#[test]
fn callback_accepts_one_matching_code_and_rejects_replay() {
    let state = oauth2::CsrfToken::new("expected-state".into());
    let mut callback = CallbackGuard::new(state);

    let code = callback
        .accept(CallbackParameters {
            code: Some("one-time-code".into()),
            state: Some("expected-state".into()),
            error: None,
        })
        .unwrap();
    assert_eq!(code.secret(), "one-time-code");
    assert_eq!(
        callback.accept(CallbackParameters {
            code: Some("replayed-code".into()),
            state: Some("expected-state".into()),
            error: None,
        }),
        Err(OAuthError::Replayed)
    );
}

#[test]
fn callback_mismatch_consumes_the_attempt_without_exposing_provider_details() {
    let mut callback = CallbackGuard::new(oauth2::CsrfToken::new("expected-state".into()));

    assert_eq!(
        callback.accept(CallbackParameters {
            code: Some("attacker-code".into()),
            state: Some("wrong-state".into()),
            error: None,
        }),
        Err(OAuthError::StateMismatch)
    );
    assert_eq!(
        callback.accept(CallbackParameters {
            code: Some("later-code".into()),
            state: Some("expected-state".into()),
            error: None,
        }),
        Err(OAuthError::Replayed)
    );
}

#[test]
fn callback_provider_rejection_is_typed_and_single_use() {
    let mut callback = CallbackGuard::new(oauth2::CsrfToken::new("expected-state".into()));

    assert_eq!(
        callback.accept(CallbackParameters {
            code: None,
            state: Some("expected-state".into()),
            error: Some("access_denied: raw provider detail".into()),
        }),
        Err(OAuthError::Provider)
    );
    assert_eq!(
        callback.accept(CallbackParameters::default()),
        Err(OAuthError::Replayed)
    );
}

#[test]
fn authorization_url_uses_pkce_state_and_the_registered_loopback_callback() {
    let first = AuthorizationAttempt::new(false).unwrap();
    let second = AuthorizationAttempt::new(false).unwrap();
    let first_query = query(first.authorization_url());
    let second_query = query(second.authorization_url());

    assert_eq!(
        first.authorization_url().as_str().split('?').next(),
        Some("https://github.com/login/oauth/authorize")
    );
    assert_eq!(
        first_query.get("client_id").map(String::as_str),
        Some("Ov23lidoL3QovWyfxnA4")
    );
    assert_eq!(first_query.get("scope").map(String::as_str), Some("repo"));
    assert_eq!(
        first_query.get("redirect_uri").map(String::as_str),
        Some(GITHUB_CALLBACK_URL)
    );
    assert_eq!(
        first_query.get("code_challenge_method").map(String::as_str),
        Some("S256")
    );
    assert_eq!(first_query.get("code_challenge").map(String::len), Some(43));
    assert!(first_query
        .get("state")
        .is_some_and(|state| !state.is_empty()));
    assert_ne!(first_query.get("state"), second_query.get("state"));
    assert_ne!(
        first_query.get("code_challenge"),
        second_query.get("code_challenge")
    );
    assert!(!first_query.contains_key("client_secret"));
}

#[test]
fn retry_can_request_github_account_selection_without_changing_the_callback() {
    let attempt = AuthorizationAttempt::new(true).unwrap();
    let query = query(attempt.authorization_url());

    assert_eq!(
        query.get("prompt").map(String::as_str),
        Some("select_account")
    );
    assert_eq!(
        query.get("redirect_uri").map(String::as_str),
        Some(GITHUB_CALLBACK_URL)
    );
}

#[test]
fn code_exchange_sends_pkce_without_a_client_secret_and_returns_rotating_tokens() {
    let http = |request: oauth2::HttpRequest| {
        assert_eq!(
            request.uri().to_string(),
            "https://github.com/login/oauth/access_token"
        );
        let fields: BTreeMap<String, String> =
            serde_urlencoded::from_bytes(request.body()).unwrap();
        assert_eq!(
            fields.get("client_id").map(String::as_str),
            Some("Ov23lidoL3QovWyfxnA4")
        );
        assert_eq!(
            fields.get("redirect_uri").map(String::as_str),
            Some(GITHUB_CALLBACK_URL)
        );
        assert_eq!(
            fields.get("code").map(String::as_str),
            Some("authorization-code")
        );
        assert_eq!(
            fields.get("code_verifier").map(String::as_str),
            Some("abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-._~")
        );
        assert!(!fields.contains_key("client_secret"));
        Ok::<_, std::io::Error>(
            oauth2::http::Response::builder()
                .status(200)
                .header("content-type", "application/json")
                .body(
                    br#"{
                      "access_token":"access-secret",
                      "expires_in":28800,
                      "refresh_token":"refresh-secret",
                      "refresh_token_expires_in":15897600,
                      "token_type":"bearer",
                      "scope":""
                    }"#
                    .to_vec(),
                )
                .unwrap(),
        )
    };

    let pair = exchange_code_with(
        oauth2::AuthorizationCode::new("authorization-code".into()),
        oauth2::PkceCodeVerifier::new(
            "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-._~".into(),
        ),
        UNIX_EPOCH + Duration::from_secs(100),
        &http,
    )
    .unwrap();

    assert_eq!(pair.access_token(), "access-secret");
    assert_eq!(pair.refresh_token(), "refresh-secret");
    assert_eq!(
        pair.access_expires_at(),
        UNIX_EPOCH + Duration::from_secs(28_900)
    );
    assert_eq!(
        pair.refresh_expires_at(),
        UNIX_EPOCH + Duration::from_secs(15_897_700)
    );
    assert_eq!(format!("{pair:?}"), "TokenPair([REDACTED])");
}

#[test]
fn token_exchange_distinguishes_provider_network_and_invalid_responses() {
    let verifier = || {
        oauth2::PkceCodeVerifier::new(
            "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-._~".into(),
        )
    };
    let provider = |_request: oauth2::HttpRequest| {
        Ok::<_, std::io::Error>(
            oauth2::http::Response::builder()
                .status(400)
                .header("content-type", "application/json")
                .body(br#"{"error":"bad_verification_code"}"#.to_vec())
                .unwrap(),
        )
    };
    assert_eq!(
        exchange_code_with(
            oauth2::AuthorizationCode::new("code".into()),
            verifier(),
            UNIX_EPOCH,
            &provider,
        ),
        Err(OAuthError::Provider)
    );

    let network = |_request: oauth2::HttpRequest| {
        Err::<oauth2::HttpResponse, _>(std::io::Error::other("offline"))
    };
    assert_eq!(
        exchange_code_with(
            oauth2::AuthorizationCode::new("code".into()),
            verifier(),
            UNIX_EPOCH,
            &network,
        ),
        Err(OAuthError::Network)
    );

    let invalid = |_request: oauth2::HttpRequest| {
        Ok::<_, std::io::Error>(
            oauth2::http::Response::builder()
                .status(200)
                .header("content-type", "application/json")
                .body(br#"{"access_token":"partial","token_type":"bearer"}"#.to_vec())
                .unwrap(),
        )
    };
    assert_eq!(
        exchange_code_with(
            oauth2::AuthorizationCode::new("code".into()),
            verifier(),
            UNIX_EPOCH,
            &invalid,
        ),
        Err(OAuthError::InvalidResponse)
    );
}

#[test]
fn refresh_exchange_rotates_the_complete_token_pair_without_a_client_secret() {
    let http = |request: oauth2::HttpRequest| {
        let fields: BTreeMap<String, String> =
            serde_urlencoded::from_bytes(request.body()).unwrap();
        assert_eq!(
            fields.get("grant_type").map(String::as_str),
            Some("refresh_token")
        );
        assert_eq!(
            fields.get("refresh_token").map(String::as_str),
            Some("old-refresh")
        );
        assert_eq!(
            fields.get("client_id").map(String::as_str),
            Some("Ov23lidoL3QovWyfxnA4")
        );
        assert!(!fields.contains_key("client_secret"));
        Ok::<_, std::io::Error>(
            oauth2::http::Response::builder()
                .status(200)
                .header("content-type", "application/json")
                .body(
                    br#"{
                      "access_token":"new-access",
                      "expires_in":28800,
                      "refresh_token":"new-refresh",
                      "refresh_token_expires_in":15897600,
                      "token_type":"bearer",
                      "scope":""
                    }"#
                    .to_vec(),
                )
                .unwrap(),
        )
    };

    let pair = pr_sniper_lib::github::oauth::refresh_token_with(
        oauth2::RefreshToken::new("old-refresh".into()),
        UNIX_EPOCH,
        &http,
    )
    .unwrap();

    assert_eq!(pair.access_token(), "new-access");
    assert_eq!(pair.refresh_token(), "new-refresh");
}

#[test]
fn loopback_bind_failure_is_typed_and_does_not_open_the_browser() {
    let occupied = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = occupied.local_addr().unwrap();
    let mut opened = false;

    let result = prepare_authorization_with(
        address,
        &format!("http://{address}/oauth/github/callback"),
        false,
        |_| {
            opened = true;
            Ok(())
        },
    );

    assert!(matches!(result, Err(OAuthError::Bind)));
    assert!(!opened);
}

#[test]
fn browser_open_failure_releases_the_loopback_port() {
    let probe = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = probe.local_addr().unwrap();
    drop(probe);

    let result = prepare_authorization_with(
        address,
        &format!("http://{address}/oauth/github/callback"),
        false,
        |_| Err(()),
    );

    assert!(matches!(result, Err(OAuthError::BrowserOpen)));
    TcpListener::bind(address).expect("failed browser launch must release callback port");
}

fn prepared_callback() -> pr_sniper_lib::github::oauth::PreparedAuthorization {
    let probe = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = probe.local_addr().unwrap();
    drop(probe);
    prepare_authorization_with(
        address,
        &format!("http://{address}/oauth/github/callback"),
        false,
        |_| Ok(()),
    )
    .unwrap()
}

#[tokio::test]
async fn loopback_callback_returns_a_secret_free_success_page_once() {
    let prepared = prepared_callback();
    let address = prepared.listener.local_addr().unwrap();
    let state = query(prepared.attempt.authorization_url())["state"].clone();
    let request_state = state.clone();
    let (_cancel_tx, cancel_rx) = tokio::sync::oneshot::channel();
    let completion = tokio::spawn(receive_callback(prepared, cancel_rx));

    let response = tokio::task::spawn_blocking(move || {
        reqwest::blocking::get(format!(
            "http://{address}/oauth/github/callback?code=secret-code&state={request_state}"
        ))
        .unwrap()
        .text()
        .unwrap()
    })
    .await
    .unwrap();
    let result = completion.await.unwrap().unwrap();

    assert_eq!(result.code.secret(), "secret-code");
    assert!(response.contains("Authorization response received"));
    assert!(response.contains("Return to PR Sniper"));
    assert!(!response.contains("Connected"));
    assert!(!response.contains("secret-code"));
    assert!(!response.contains(&state));
}

#[tokio::test]
async fn loopback_callback_rejects_state_mismatch_without_echoing_provider_errors() {
    let prepared = prepared_callback();
    let address = prepared.listener.local_addr().unwrap();
    let (_cancel_tx, cancel_rx) = tokio::sync::oneshot::channel();
    let completion = tokio::spawn(receive_callback(prepared, cancel_rx));

    let response = tokio::task::spawn_blocking(move || {
        reqwest::blocking::get(format!(
            "http://{address}/oauth/github/callback?error=access_denied&error_description=raw-provider-secret&state=wrong"
        ))
        .unwrap()
        .text()
        .unwrap()
    })
    .await
    .unwrap();

    assert!(matches!(
        completion.await.unwrap(),
        Err(OAuthError::StateMismatch)
    ));
    assert!(response.contains("Could not connect"));
    assert!(!response.contains("raw-provider-secret"));
    assert!(!response.contains("access_denied"));
}

#[tokio::test]
async fn loopback_callback_honors_cancellation_and_timeout() {
    let mut timed_out = prepared_callback();
    timed_out.deadline = std::time::Instant::now() + Duration::from_millis(20);
    let (_cancel_tx, cancel_rx) = tokio::sync::oneshot::channel();
    assert!(matches!(
        receive_callback(timed_out, cancel_rx).await,
        Err(OAuthError::Timeout)
    ));

    let cancelled = prepared_callback();
    let (cancel_tx, cancel_rx) = tokio::sync::oneshot::channel();
    cancel_tx.send(()).unwrap();
    assert!(matches!(
        receive_callback(cancelled, cancel_rx).await,
        Err(OAuthError::Cancelled)
    ));
}
