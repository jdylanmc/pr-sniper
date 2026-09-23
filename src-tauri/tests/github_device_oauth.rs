use pr_sniper_lib::github::oauth::{
    poll_device_authorization_with, request_device_authorization_with, OAuthError,
    GITHUB_DEVICE_AUTHORIZATION_URL,
};
use std::{
    cell::RefCell,
    collections::{BTreeMap, VecDeque},
    sync::atomic::{AtomicBool, AtomicUsize, Ordering},
    time::{Duration, UNIX_EPOCH},
};

fn prepared(interval: u64) -> pr_sniper_lib::github::oauth::PreparedDeviceAuthorization {
    let http = move |_request: oauth2::HttpRequest| {
        Ok::<_, std::io::Error>(
            oauth2::http::Response::builder()
                .status(200)
                .header("content-type", "application/json")
                .body(
                    format!(
                        r#"{{
                          "device_code":"secret-device-code",
                          "user_code":"ABCD-EFGH",
                          "verification_uri":"https://github.com/login/device",
                          "expires_in":900,
                          "interval":{interval}
                        }}"#
                    )
                    .into_bytes(),
                )
                .unwrap(),
        )
    };
    request_device_authorization_with(&http, |_| Ok(())).unwrap()
}

#[test]
fn device_authorization_requests_secretless_repo_access_and_opens_verified_github_uri() {
    let http = |request: oauth2::HttpRequest| {
        assert_eq!(request.uri().to_string(), GITHUB_DEVICE_AUTHORIZATION_URL);
        let fields: BTreeMap<String, String> =
            serde_urlencoded::from_bytes(request.body()).unwrap();
        assert_eq!(
            fields.get("client_id").map(String::as_str),
            Some("Ov23lidoL3QovWyfxnA4")
        );
        assert_eq!(
            fields.get("scope").map(String::as_str),
            Some("repo offline_access")
        );
        assert!(!fields.contains_key("client_secret"));
        Ok::<_, std::io::Error>(
            oauth2::http::Response::builder()
                .status(200)
                .header("content-type", "application/json")
                .body(
                    br#"{
                      "device_code":"secret-device-code",
                      "user_code":"ABCD-EFGH",
                      "verification_uri":"https://github.com/login/device",
                      "verification_uri_complete":"https://github.com/login/device?user_code=ABCD-EFGH",
                      "expires_in":900,
                      "interval":5
                    }"#
                    .to_vec(),
                )
                .unwrap(),
        )
    };
    let opened = RefCell::new(None);

    let authorization = request_device_authorization_with(&http, |url| {
        opened.replace(Some(url.as_str().to_owned()));
        Ok(())
    })
    .unwrap();

    assert_eq!(authorization.user_code(), "ABCD-EFGH");
    assert_eq!(
        authorization.verification_uri(),
        "https://github.com/login/device"
    );
    assert_eq!(authorization.expires_in(), Duration::from_secs(900));
    assert_eq!(
        opened.into_inner().as_deref(),
        Some("https://github.com/login/device?user_code=ABCD-EFGH")
    );
    assert!(!format!("{authorization:?}").contains("secret-device-code"));
}

#[test]
fn device_authorization_rejects_provider_errors_from_http_200_envelopes() {
    let http = |_request: oauth2::HttpRequest| {
        Ok::<_, std::io::Error>(
            oauth2::http::Response::builder()
                .status(200)
                .header("content-type", "application/json")
                .body(
                    br#"{"error":"device_flow_disabled","error_description":"private detail"}"#
                        .to_vec(),
                )
                .unwrap(),
        )
    };

    assert!(matches!(
        request_device_authorization_with(&http, |_| panic!("must not open browser")),
        Err(OAuthError::Provider)
    ));
}

#[test]
fn device_authorization_uses_only_provider_returned_verified_browser_destinations() {
    for (verification_uri, complete_uri, expected) in [
        (
            "https://github.com/login/device",
            None,
            Ok("https://github.com/login/device"),
        ),
        (
            "https://github.com/login/device",
            Some("https://attacker.example/device?user_code=ABCD-EFGH"),
            Ok("https://github.com/login/device"),
        ),
        (
            "http://github.com/login/device",
            None,
            Err(OAuthError::InvalidResponse),
        ),
    ] {
        let body = format!(
            r#"{{
              "device_code":"secret-device-code",
              "user_code":"ABCD-EFGH",
              "verification_uri":"{verification_uri}",
              {}"expires_in":900,
              "interval":5
            }}"#,
            complete_uri
                .map(|uri| format!(r#""verification_uri_complete":"{uri}","#))
                .unwrap_or_default()
        );
        let http = |_request: oauth2::HttpRequest| {
            Ok::<_, std::io::Error>(
                oauth2::http::Response::builder()
                    .status(200)
                    .header("content-type", "application/json")
                    .body(body.as_bytes().to_vec())
                    .unwrap(),
            )
        };
        let opened = RefCell::new(None);
        let result = request_device_authorization_with(&http, |url| {
            opened.replace(Some(url.as_str().to_owned()));
            Ok(())
        });
        match expected {
            Ok(expected) => {
                result.unwrap();
                assert_eq!(opened.into_inner().as_deref(), Some(expected));
            }
            Err(expected) => assert!(matches!(result, Err(error) if error == expected)),
        }
    }
}

#[test]
fn device_authorization_surfaces_browser_open_failure_without_exposing_codes() {
    assert!(matches!(
        request_device_authorization_with(
            &|_request: oauth2::HttpRequest| {
                Ok::<_, std::io::Error>(
                    oauth2::http::Response::builder()
                        .status(200)
                        .header("content-type", "application/json")
                        .body(
                            br#"{
                          "device_code":"secret-device-code",
                          "user_code":"ABCD-EFGH",
                          "verification_uri":"https://github.com/login/device",
                          "expires_in":900,
                          "interval":5
                        }"#
                            .to_vec(),
                        )
                        .unwrap(),
                )
            },
            |_| Err(OAuthError::BrowserOpen)
        ),
        Err(OAuthError::BrowserOpen)
    ));
}

#[test]
fn device_poll_adapts_http_200_pending_and_slow_down_for_the_sdk_poller() {
    let responses = RefCell::new(VecDeque::from([
        br#"{"error":"authorization_pending"}"#.to_vec(),
        br#"{"error":"slow_down","interval":10}"#.to_vec(),
        br#"{
          "access_token":"access-secret",
          "expires_in":28800,
          "refresh_token":"refresh-secret",
          "refresh_token_expires_in":15897600,
          "token_type":"bearer",
          "scope":"repo"
        }"#
        .to_vec(),
    ]));
    let http = |request: oauth2::HttpRequest| {
        let fields: BTreeMap<String, String> =
            serde_urlencoded::from_bytes(request.body()).unwrap();
        assert_eq!(
            fields.get("grant_type").map(String::as_str),
            Some("urn:ietf:params:oauth:grant-type:device_code")
        );
        assert_eq!(
            fields.get("device_code").map(String::as_str),
            Some("secret-device-code")
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
                .body(responses.borrow_mut().pop_front().unwrap())
                .unwrap(),
        )
    };
    let sleeps = RefCell::new(Vec::new());

    let pair = poll_device_authorization_with(
        prepared(5),
        UNIX_EPOCH + Duration::from_secs(100),
        &http,
        |duration| sleeps.borrow_mut().push(duration),
        &AtomicBool::new(false),
    )
    .unwrap();

    assert_eq!(
        sleeps.into_inner(),
        vec![Duration::from_secs(5), Duration::from_secs(10)]
    );
    assert_eq!(pair.access_token(), "access-secret");
    assert_eq!(pair.refresh_token(), "refresh-secret");
}

#[test]
fn device_poll_classifies_http_200_denial_expiry_and_malformed_responses() {
    for (body, expected) in [
        (
            br#"{"error":"access_denied","error_description":"private"}"#.as_slice(),
            OAuthError::Denied,
        ),
        (
            br#"{"error":"expired_token","error_description":"private"}"#.as_slice(),
            OAuthError::Expired,
        ),
        (br#"{"error":"#.as_slice(), OAuthError::InvalidResponse),
    ] {
        let http = |_request: oauth2::HttpRequest| {
            Ok::<_, std::io::Error>(
                oauth2::http::Response::builder()
                    .status(200)
                    .header("content-type", "application/json")
                    .body(body.to_vec())
                    .unwrap(),
            )
        };
        assert_eq!(
            poll_device_authorization_with(
                prepared(5),
                UNIX_EPOCH,
                &http,
                |_| {},
                &AtomicBool::new(false),
            ),
            Err(expected)
        );
    }
}

#[test]
fn device_poll_cancellation_stops_before_contacting_the_provider() {
    let requests = AtomicUsize::new(0);
    let http = |_request: oauth2::HttpRequest| -> Result<oauth2::HttpResponse, std::io::Error> {
        requests.fetch_add(1, Ordering::SeqCst);
        panic!("cancelled polling must not contact GitHub")
    };
    let cancelled = AtomicBool::new(true);

    assert_eq!(
        poll_device_authorization_with(prepared(5), UNIX_EPOCH, &http, |_| {}, &cancelled,),
        Err(OAuthError::Cancelled)
    );
    assert_eq!(requests.load(Ordering::SeqCst), 0);
}

#[test]
fn device_poll_cancellation_interrupts_pending_polling_before_another_provider_request() {
    let requests = AtomicUsize::new(0);
    let cancelled = AtomicBool::new(false);
    let http = |_request: oauth2::HttpRequest| {
        requests.fetch_add(1, Ordering::SeqCst);
        Ok::<_, std::io::Error>(
            oauth2::http::Response::builder()
                .status(200)
                .header("content-type", "application/json")
                .body(br#"{"error":"authorization_pending"}"#.to_vec())
                .unwrap(),
        )
    };

    assert_eq!(
        poll_device_authorization_with(
            prepared(5),
            UNIX_EPOCH,
            &http,
            |_| cancelled.store(true, Ordering::SeqCst),
            &cancelled,
        ),
        Err(OAuthError::Cancelled)
    );
    assert_eq!(requests.load(Ordering::SeqCst), 1);
}

#[test]
fn device_poll_network_backoff_never_shortens_the_provider_interval() {
    let attempts = AtomicUsize::new(0);
    let http = |_request: oauth2::HttpRequest| {
        if attempts.fetch_add(1, Ordering::SeqCst) == 0 {
            return Err(std::io::Error::other("offline"));
        }
        Ok(oauth2::http::Response::builder()
            .status(200)
            .header("content-type", "application/json")
            .body(
                br#"{
                      "access_token":"access-secret",
                      "expires_in":28800,
                      "refresh_token":"refresh-secret",
                      "refresh_token_expires_in":15897600,
                      "token_type":"bearer",
                      "scope":"repo"
                    }"#
                .to_vec(),
            )
            .unwrap())
    };
    let sleeps = RefCell::new(Vec::new());

    poll_device_authorization_with(
        prepared(12),
        UNIX_EPOCH,
        &http,
        |duration| sleeps.borrow_mut().push(duration),
        &AtomicBool::new(false),
    )
    .unwrap();

    assert_eq!(sleeps.into_inner(), vec![Duration::from_secs(24)]);
}

#[test]
fn refresh_classifies_github_http_200_error_envelopes_without_a_client_secret() {
    let http = |request: oauth2::HttpRequest| {
        let fields: BTreeMap<String, String> =
            serde_urlencoded::from_bytes(request.body()).unwrap();
        assert_eq!(
            fields.get("grant_type").map(String::as_str),
            Some("refresh_token")
        );
        assert!(!fields.contains_key("client_secret"));
        Ok::<_, std::io::Error>(
            oauth2::http::Response::builder()
                .status(200)
                .header("content-type", "application/json")
                .body(
                    br#"{"error":"incorrect_client_credentials","error_description":"private"}"#
                        .to_vec(),
                )
                .unwrap(),
        )
    };

    assert_eq!(
        pr_sniper_lib::github::oauth::refresh_token_with(
            oauth2::RefreshToken::new("refresh-secret".into()),
            UNIX_EPOCH,
            &http,
        ),
        Err(OAuthError::Provider)
    );
}

#[test]
fn refresh_rotates_the_complete_device_flow_token_pair_without_a_client_secret() {
    let http = |request: oauth2::HttpRequest| {
        let fields: BTreeMap<String, String> =
            serde_urlencoded::from_bytes(request.body()).unwrap();
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
                      "scope":"repo"
                    }"#
                    .to_vec(),
                )
                .unwrap(),
        )
    };

    let pair = pr_sniper_lib::github::oauth::refresh_token_with(
        oauth2::RefreshToken::new("old-refresh".into()),
        UNIX_EPOCH + Duration::from_secs(100),
        &http,
    )
    .unwrap();

    assert_eq!(pair.access_token(), "new-access");
    assert_eq!(pair.refresh_token(), "new-refresh");
    assert_eq!(
        pair.access_expires_at(),
        UNIX_EPOCH + Duration::from_secs(28_900)
    );
    assert_eq!(format!("{pair:?}"), "TokenPair([REDACTED])");
}
