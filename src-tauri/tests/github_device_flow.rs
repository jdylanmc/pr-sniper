use pr_sniper_lib::github::device_flow::{
    DeviceAuthorization, DeviceFlow, DeviceFlowError, DeviceFlowPoll, DeviceFlowTransport,
    ProviderPoll, TokenPair, GITHUB_APP_CLIENT_ID,
};
use std::{cell::RefCell, collections::VecDeque, time::Duration};

struct FixtureTransport {
    polls: RefCell<VecDeque<Result<ProviderPoll, DeviceFlowError>>>,
}

impl DeviceFlowTransport for FixtureTransport {
    fn begin(&self, client_id: &str) -> Result<DeviceAuthorization, DeviceFlowError> {
        assert_eq!(client_id, GITHUB_APP_CLIENT_ID);
        Ok(DeviceAuthorization::new(
            "device-secret",
            "ABCD-EFGH",
            "https://github.com/login/device",
            Duration::from_secs(900),
            Duration::from_secs(5),
        ))
    }

    fn poll(&self, client_id: &str, device_code: &str) -> Result<ProviderPoll, DeviceFlowError> {
        assert_eq!(client_id, GITHUB_APP_CLIENT_ID);
        assert_eq!(device_code, "device-secret");
        self.polls.borrow_mut().pop_front().unwrap()
    }
}

fn flow(
    polls: impl IntoIterator<Item = Result<ProviderPoll, DeviceFlowError>>,
) -> DeviceFlow<FixtureTransport> {
    DeviceFlow::begin(
        FixtureTransport {
            polls: RefCell::new(polls.into_iter().collect()),
        },
        Duration::ZERO,
    )
    .unwrap()
}

#[test]
fn pending_and_slow_down_preserve_provider_poll_timing() {
    let mut flow = flow([
        Ok(ProviderPoll::AuthorizationPending),
        Ok(ProviderPoll::SlowDown),
        Ok(ProviderPoll::AuthorizationPending),
    ]);

    assert_eq!(
        flow.poll(Duration::from_secs(4)),
        DeviceFlowPoll::WaitUntil(Duration::from_secs(5))
    );
    assert_eq!(
        flow.poll(Duration::from_secs(5)),
        DeviceFlowPoll::Pending {
            retry_at: Duration::from_secs(10)
        }
    );
    assert_eq!(
        flow.poll(Duration::from_secs(10)),
        DeviceFlowPoll::Pending {
            retry_at: Duration::from_secs(20)
        }
    );
    assert_eq!(
        flow.poll(Duration::from_secs(19)),
        DeviceFlowPoll::WaitUntil(Duration::from_secs(20))
    );
    assert_eq!(
        flow.poll(Duration::from_secs(20)),
        DeviceFlowPoll::Pending {
            retry_at: Duration::from_secs(30)
        }
    );
}

#[test]
fn terminal_provider_states_are_explicit() {
    for (provider, expected) in [
        (ProviderPoll::AccessDenied, DeviceFlowPoll::Denied),
        (ProviderPoll::ExpiredToken, DeviceFlowPoll::Expired),
    ] {
        let mut flow = flow([Ok(provider)]);
        assert_eq!(flow.poll(Duration::from_secs(5)), expected);
        assert_eq!(flow.poll(Duration::from_secs(6)), expected);
    }
}

#[test]
fn cancellation_and_local_expiry_do_not_contact_the_provider() {
    let mut cancelled = flow([]);
    cancelled.cancel();
    assert_eq!(
        cancelled.poll(Duration::from_secs(5)),
        DeviceFlowPoll::Cancelled
    );

    let mut expired = flow([]);
    assert_eq!(
        expired.poll(Duration::from_secs(901)),
        DeviceFlowPoll::Expired
    );
}

#[test]
fn success_returns_non_serializable_redacted_credentials() {
    let pair = TokenPair::new(
        "access-secret",
        "refresh-secret",
        Duration::from_secs(28_800),
        Duration::from_secs(15_552_000),
    );
    let mut flow = flow([Ok(ProviderPoll::Authorized(pair))]);
    let DeviceFlowPoll::Authorized(pair) = flow.poll(Duration::from_secs(5)) else {
        panic!("expected authorization");
    };
    assert_eq!(format!("{pair:?}"), "TokenPair([REDACTED])");
    assert_eq!(pair.access_token(), "access-secret");
    assert_eq!(pair.refresh_token(), "refresh-secret");
}

#[test]
fn network_and_provider_failures_are_honest_terminal_results() {
    for error in [DeviceFlowError::Network, DeviceFlowError::Provider] {
        let mut flow = flow([Err(error)]);
        assert_eq!(
            flow.poll(Duration::from_secs(5)),
            DeviceFlowPoll::Failed(error)
        );
    }
}
