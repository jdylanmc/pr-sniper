//! Spawns a fake `az` script through the real runner. Single test: it sets an
//! agent-session variable in this test process's environment.
#![cfg(unix)]

use pr_sniper_lib::azure_devops::tenant::TenantId;
use pr_sniper_lib::azure_devops::token_source::{
    system_clock, AzCliTokenSource, TokenError, TokenSource,
};
use pr_sniper_lib::cli_process::SystemRunner;
use std::os::unix::fs::PermissionsExt;

// Answers like an agent-mode Azure CLI that would go interactive (here: fail)
// when the agent-session variable leaks, stdin is open or overrides are missing.
const FAKE_AZ: &str = r#"#!/bin/sh
if [ -n "${COPILOT_AGENT_SESSION_ID+x}" ]; then echo "ERROR: AADSTS50076: interactive" >&2; exit 1; fi
if read -r _line; then echo "stdin open" >&2; exit 1; fi
if [ "$AZURE_CORE_COLLECT_TELEMETRY" != false ] || ! env | grep -qx 'AZURE_AUTO-UPGRADE_ENABLE=no'; then exit 4; fi
case "$1" in
  version) printf '{"azure-cli":"2.90.0","extensions":{}}' ;;
  account) printf '{"accessToken":"fixture.token","expires_on":4102444800,"tokenType":"Bearer"}' ;;
  *) exit 2 ;;
esac
"#;

#[tokio::test]
async fn agent_session_in_parent_does_not_reach_azure_cli() {
    std::env::set_var("COPILOT_AGENT_SESSION_ID", "fixture-session");
    let directory = std::env::current_dir()
        .unwrap()
        .join(format!(".pr-sniper-az-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&directory).unwrap();
    let az = directory.join("az");
    std::fs::write(&az, FAKE_AZ).unwrap();
    std::fs::set_permissions(&az, std::fs::Permissions::from_mode(0o700)).unwrap();

    let source = AzCliTokenSource::new(Some(az), SystemRunner, system_clock());
    let tenant = TenantId::parse("00000000-0000-0000-0000-000000000001").unwrap();
    let result = source.token(&tenant).await.map(|token| token.expires_on);
    std::fs::remove_dir_all(&directory).unwrap();
    assert_eq!(result, Ok(4_102_444_800));

    let missing = AzCliTokenSource::new(Some(directory.join("az")), SystemRunner, system_clock());
    assert_eq!(
        missing.token(&tenant).await.unwrap_err(),
        TokenError::AzMissing
    );
}
