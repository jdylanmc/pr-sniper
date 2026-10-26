//! Live, read-only Azure DevOps proof for the Azure CLI token source.
//! Ignored by default. Needs a signed-in Azure CLI and `PR_SNIPER_ADO_ORG`.
//! Prints statuses and timings only.

use pr_sniper_lib::azure_devops::tenant::{self, HttpHeadTransport};
use pr_sniper_lib::azure_devops::token_source::{AzCliTokenSource, TokenSource};
use std::time::Instant;

#[tokio::test]
#[ignore = "live: needs a signed-in Azure CLI and PR_SNIPER_ADO_ORG"]
async fn azure_cli_token_reads_azure_devops_connection_data() {
    let organization = std::env::var("PR_SNIPER_ADO_ORG").expect("PR_SNIPER_ADO_ORG is required");

    let started = Instant::now();
    let transport = HttpHeadTransport::new().expect("HTTP client");
    let tenant = tenant::discover(&transport, &organization)
        .await
        .expect("tenant discovery");
    println!("tenant_discovery_ms={}", started.elapsed().as_millis());

    let source = AzCliTokenSource::discover();
    let started = Instant::now();
    let token = source.token(&tenant).await.expect("Azure CLI token");
    let az_ms = started.elapsed().as_millis();
    let started = Instant::now();
    source.token(&tenant).await.expect("cached token");
    println!(
        "az_token_ms={az_ms} cached_token_us={}",
        started.elapsed().as_micros()
    );

    let mut url = reqwest::Url::parse("https://dev.azure.com/").unwrap();
    url.path_segments_mut().unwrap().pop_if_empty().extend([
        organization.as_str(),
        "_apis",
        "connectionData",
    ]);
    let response = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap()
        .get(url)
        .bearer_auth(token.secret())
        .header("Accept", "application/json")
        // Without this, a rejected token is a 302 to an HTML sign-in page, not a 401.
        .header("X-TFS-FedAuthRedirect", "Suppress")
        .send()
        .await
        .expect("connectionData");
    let status = response.status().as_u16();
    println!("connection_data_status={status}");
    assert_eq!(status, 200);
}
