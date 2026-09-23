use pr_sniper_lib::github::{
    device_flow::{DeviceFlowError, ProviderPoll},
    device_http::{parse_authorization, parse_poll},
};

#[test]
fn parses_github_device_authorization_without_exposing_device_code() {
    let authorization = parse_authorization(
        br#"{
          "device_code": "device-secret",
          "user_code": "ABCD-EFGH",
          "verification_uri": "https://github.com/login/device",
          "expires_in": 900,
          "interval": 5
        }"#,
    )
    .unwrap();
    assert_eq!(authorization.prompt().user_code, "ABCD-EFGH");
    assert_eq!(
        authorization.prompt().verification_uri,
        "https://github.com/login/device"
    );
    assert!(!format!("{:?}", authorization.prompt()).contains("device-secret"));
}

#[test]
fn maps_all_github_poll_responses() {
    for (body, expected) in [
        (
            br#"{"error":"authorization_pending"}"#.as_slice(),
            ProviderPoll::AuthorizationPending,
        ),
        (
            br#"{"error":"slow_down"}"#.as_slice(),
            ProviderPoll::SlowDown,
        ),
        (
            br#"{"error":"access_denied"}"#.as_slice(),
            ProviderPoll::AccessDenied,
        ),
        (
            br#"{"error":"expired_token"}"#.as_slice(),
            ProviderPoll::ExpiredToken,
        ),
    ] {
        assert_eq!(parse_poll(body), Ok(expected));
    }
}

#[test]
fn parses_rotating_token_pair_and_rejects_partial_success() {
    let poll = parse_poll(
        br#"{
          "access_token": "access-secret",
          "expires_in": 28800,
          "refresh_token": "refresh-secret",
          "refresh_token_expires_in": 15552000,
          "token_type": "bearer"
        }"#,
    )
    .unwrap();
    let ProviderPoll::Authorized(pair) = poll else {
        panic!("expected token pair");
    };
    assert_eq!(pair.access_token(), "access-secret");
    assert_eq!(pair.refresh_token(), "refresh-secret");

    assert_eq!(
        parse_poll(br#"{"access_token":"partial","expires_in":28800}"#),
        Err(DeviceFlowError::InvalidResponse)
    );
}
