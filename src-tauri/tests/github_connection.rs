use pr_sniper_lib::github::{verify_identity, Identity};
use serde_json::json;

#[test]
fn verified_account_uses_decimal_id_not_graphql_node_id() {
    let response = json!({
        "id": 6954990,
        "login": "jdylanmc",
        "node_id": "MDQ6VXNlcjY5NTQ5OTA="
    });

    let result = verify_identity(&response, Some("6954990"));

    assert_eq!(
        result,
        Ok(Identity {
            id: "6954990".into(),
            login: "jdylanmc".into(),
        })
    );
}
