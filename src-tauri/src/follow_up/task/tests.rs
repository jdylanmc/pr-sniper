use super::*;
use crate::github::{provider::Response, review::TreeEntry, ConnectionError};
use base64::{engine::general_purpose::STANDARD, Engine};
use std::collections::BTreeMap;

struct Source;
impl Transport for Source {
    fn get(&self, path: &str) -> Result<Response, ConnectionError> {
        assert_eq!(
            path,
            "/repos/example/repo/git/blobs/cccccccccccccccccccccccccccccccccccccccc"
        );
        let source = "return 42;\n";
        Ok(Response { status:200,headers:BTreeMap::new(),body:serde_json::to_vec(&json!({
            "sha":"c".repeat(40),"size":source.len(),"encoding":"base64","content":STANDARD.encode(source)
        })).unwrap() })
    }
}

fn validate(value: serde_json::Value, previous: &str) -> Result<ReplyOutput, Failure> {
    let thread = Thread {
        id: "thread".into(),
        resolved: false,
        can_reply: true,
        comments: vec![crate::github::threads::Comment {
            id: "1".into(),
            body: previous.into(),
            author_id: Some("22".into()),
            author_login: Some("owner".into()),
            reply_to: None,
            review_id: Some("42".into()),
            original_commit: Some("a".repeat(40)),
            created_at: "time".into(),
            published_at: "time".into(),
        }],
    };
    let context = ReviewContext {
        pull: crate::github::metadata::PullRequest {
            mentioned: false,
            id: "9".into(),
            number: 1,
            title: "Review".into(),
            author: None,
            requested_reviewers: vec![],
            requested_teams: vec![],
            state: crate::github::metadata::Lifecycle::Open,
            draft: false,
            head_sha: "a".repeat(40),
            base_sha: "b".repeat(40),
            head_repository_id: Some("100".into()),
            base_repository_id: "100".into(),
            updated_at: "time".into(),
            files: vec![],
        },
        base_revision: "b".repeat(40),
        files: vec![],
        base: BTreeMap::new(),
        head: BTreeMap::from([(
            "source.rs".into(),
            TreeEntry {
                path: "source.rs".into(),
                mode: "100644".into(),
                sha: "c".repeat(40),
                kind: "blob".into(),
            },
        )]),
    };
    ReplyTask {
        conversation: ConversationInput::Owned { thread },
        trigger_id: "2".into(),
        feedback: Vec::new(),
        owner_agent_id: String::new(),
    }
    .validate(
        &value.to_string(),
        &context,
        &GithubClient::new(Source),
        "example/repo",
    )
}

fn reply() -> serde_json::Value {
    json!({"decision":"reply","body":"The function returns 42 rather than the requested 0.",
        "new_information":"The function returns 42 rather than the requested 0.","reason":"A directly observed source fact.",
        "evidence":[{"path":"source.rs","side":"head","line":1,"quote":"return 42;"}]})
}

#[test]
fn reply_requires_exact_immutable_source_evidence() {
    assert_eq!(
        validate(reply(), "What value is returned?")
            .unwrap()
            .decision,
        ReplyDecision::Reply
    );
    for (key, value) in [
        ("line", json!(2)),
        ("path", json!("../secret")),
        ("quote", json!("return 0;")),
        ("side", json!("live")),
    ] {
        let mut output = reply();
        output["evidence"][0][key] = value;
        assert!(validate(output, "Question").is_err());
    }
}

#[test]
fn unchanged_speculative_acknowledgment_and_unsubstantiated_answers_stay_quiet() {
    assert_eq!(
        validate(
            reply(),
            "The function returns 42 rather than the requested 0."
        )
        .unwrap()
        .decision,
        ReplyDecision::Quiet
    );
    for body in [
        "Thanks!",
        "Agreed.",
        "Thanks for the clarification.",
        "Got it.",
        "Perhaps the function returns 42.",
        "It might return 42.",
        "I think the value is 42.",
    ] {
        let mut output = reply();
        output["body"] = json!(body);
        output["new_information"] = json!(body);
        assert_eq!(
            validate(output, "Question").unwrap().decision,
            ReplyDecision::Quiet
        );
    }
    let mut output = reply();
    output["evidence"] = json!([]);
    assert_eq!(
        validate(output, "Question").unwrap().decision,
        ReplyDecision::Quiet
    );
}

#[test]
fn human_judgment_and_quiet_decisions_cannot_hide_publishable_bodies() {
    for decision in ["quiet", "human_input_required"] {
        let value = json!({"decision":decision,"body":"","new_information":"","reason":"A human must choose the product behavior.","evidence":[]});
        assert!(validate(value.clone(), "Which behavior should we ship?").is_ok());
        let mut bad = value;
        bad["body"] = json!("We decided to ship it.");
        assert!(validate(bad, "Question").is_err());
    }
    let mut invalid = reply();
    invalid["unexpected"] = json!(true);
    assert!(validate(invalid, "Question").is_err());
}
