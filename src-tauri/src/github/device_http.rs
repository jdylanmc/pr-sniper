use super::device_flow::{
    DeviceAuthorization, DeviceFlowError, DeviceFlowTransport, ProviderPoll, TokenPair,
};
use reqwest::{
    blocking::Client,
    header::{ACCEPT, CONTENT_TYPE},
    redirect::Policy,
};
use serde::Deserialize;
use std::{collections::BTreeMap, time::Duration};

const DEVICE_CODE_URL: &str = "https://github.com/login/device/code";
const ACCESS_TOKEN_URL: &str = "https://github.com/login/oauth/access_token";

pub struct GithubDeviceHttp {
    client: Client,
}

impl GithubDeviceHttp {
    pub fn new() -> Result<Self, DeviceFlowError> {
        let client = Client::builder()
            .user_agent("PR-Sniper/0.1")
            .redirect(Policy::none())
            .connect_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(30))
            .build()
            .map_err(|_| DeviceFlowError::Network)?;
        Ok(Self { client })
    }

    fn post(&self, url: &str, fields: BTreeMap<&str, &str>) -> Result<Vec<u8>, DeviceFlowError> {
        let body =
            serde_urlencoded::to_string(fields).map_err(|_| DeviceFlowError::InvalidResponse)?;
        let response = self
            .client
            .post(url)
            .header(ACCEPT, "application/json")
            .header(CONTENT_TYPE, "application/x-www-form-urlencoded")
            .body(body)
            .send()
            .map_err(|_| DeviceFlowError::Network)?;
        if !response.status().is_success() {
            return Err(DeviceFlowError::Provider);
        }
        let bytes = response.bytes().map_err(|_| DeviceFlowError::Network)?;
        if bytes.len() > 64 * 1024 {
            return Err(DeviceFlowError::InvalidResponse);
        }
        Ok(bytes.to_vec())
    }

    pub fn refresh(&self, refresh_token: &str) -> Result<TokenPair, DeviceFlowError> {
        let body = self.post(
            ACCESS_TOKEN_URL,
            BTreeMap::from([
                ("client_id", super::device_flow::GITHUB_APP_CLIENT_ID),
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh_token),
            ]),
        )?;
        match parse_poll(&body)? {
            ProviderPoll::Authorized(pair) => Ok(pair),
            _ => Err(DeviceFlowError::Provider),
        }
    }
}

impl DeviceFlowTransport for GithubDeviceHttp {
    fn begin(&self, client_id: &str) -> Result<DeviceAuthorization, DeviceFlowError> {
        let body = self.post(DEVICE_CODE_URL, BTreeMap::from([("client_id", client_id)]))?;
        parse_authorization(&body)
    }

    fn poll(&self, client_id: &str, device_code: &str) -> Result<ProviderPoll, DeviceFlowError> {
        let body = self.post(
            ACCESS_TOKEN_URL,
            BTreeMap::from([
                ("client_id", client_id),
                ("device_code", device_code),
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
            ]),
        )?;
        parse_poll(&body)
    }
}

#[derive(Deserialize)]
struct AuthorizationResponse {
    device_code: String,
    user_code: String,
    verification_uri: String,
    expires_in: u64,
    interval: u64,
}

pub fn parse_authorization(bytes: &[u8]) -> Result<DeviceAuthorization, DeviceFlowError> {
    let response: AuthorizationResponse =
        serde_json::from_slice(bytes).map_err(|_| DeviceFlowError::InvalidResponse)?;
    Ok(DeviceAuthorization::new(
        response.device_code,
        response.user_code,
        response.verification_uri,
        Duration::from_secs(response.expires_in),
        Duration::from_secs(response.interval),
    ))
}

#[derive(Deserialize)]
struct PollResponse {
    access_token: Option<String>,
    expires_in: Option<u64>,
    refresh_token: Option<String>,
    refresh_token_expires_in: Option<u64>,
    token_type: Option<String>,
    error: Option<String>,
}

pub fn parse_poll(bytes: &[u8]) -> Result<ProviderPoll, DeviceFlowError> {
    let response: PollResponse =
        serde_json::from_slice(bytes).map_err(|_| DeviceFlowError::InvalidResponse)?;
    if let Some(error) = response.error.as_deref() {
        return match error {
            "authorization_pending" => Ok(ProviderPoll::AuthorizationPending),
            "slow_down" => Ok(ProviderPoll::SlowDown),
            "access_denied" => Ok(ProviderPoll::AccessDenied),
            "expired_token" => Ok(ProviderPoll::ExpiredToken),
            _ => Err(DeviceFlowError::Provider),
        };
    }
    if response.token_type.as_deref() != Some("bearer") {
        return Err(DeviceFlowError::InvalidResponse);
    }
    let access = response
        .access_token
        .filter(|value| !value.is_empty())
        .ok_or(DeviceFlowError::InvalidResponse)?;
    let refresh = response
        .refresh_token
        .filter(|value| !value.is_empty())
        .ok_or(DeviceFlowError::InvalidResponse)?;
    Ok(ProviderPoll::Authorized(TokenPair::new(
        access,
        refresh,
        Duration::from_secs(
            response
                .expires_in
                .filter(|value| *value > 0)
                .ok_or(DeviceFlowError::InvalidResponse)?,
        ),
        Duration::from_secs(
            response
                .refresh_token_expires_in
                .filter(|value| *value > 0)
                .ok_or(DeviceFlowError::InvalidResponse)?,
        ),
    )))
}
