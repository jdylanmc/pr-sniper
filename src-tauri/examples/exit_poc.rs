// Throwaway Phase 0 POC: SDK terminal-tool exits in the real Copilot runtime.
// Usage: exit_poc <scenario 1..3> <model> <base-dir>
use async_trait::async_trait;
use github_copilot_sdk::{
    hooks::{HookContext, PreToolUseInput, PreToolUseOutput, SessionHooks},
    tool::ToolHandler,
    Client, ClientOptions, SessionConfig, SystemMessageConfig, Tool, ToolInvocation, ToolResult,
    ToolResultExpanded,
};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};

const TOOLS: [&str; 3] = ["lookup", "leave_comments", "ready_for_human"];

struct Exits {
    scenario: u32,
    slot: Mutex<Option<(String, Value)>>,
    calls: Mutex<Vec<String>>,
}

#[async_trait]
impl ToolHandler for Exits {
    async fn call(&self, i: ToolInvocation) -> Result<ToolResult, github_copilot_sdk::Error> {
        println!("HANDLER {} args={}", i.tool_name, i.arguments);
        let mut calls = self.calls.lock().unwrap();
        calls.push(i.tool_name.clone());
        if i.tool_name == "lookup" {
            return Ok(ToolResult::Text(
                "src/a.rs:3 has `let x = unwrap()`; src/b.rs:10 has TODO".into(),
            ));
        }
        let mut slot = self.slot.lock().unwrap();
        let fail = |m: &str| {
            Ok(ToolResult::Expanded(
                ToolResultExpanded::new(m, "failure").with_error(m),
            ))
        };
        if slot.is_some() {
            return fail("exit already taken");
        }
        // Scenario 2: host denies the first leave_comments (gate) to prove failed exits keep looping.
        if self.scenario == 2
            && i.tool_name == "leave_comments"
            && !calls
                .iter()
                .take(calls.len() - 1)
                .any(|c| c == "leave_comments")
        {
            return fail("state moved; comments rejected, call ready_for_human instead");
        }
        *slot = Some((i.tool_name.clone(), i.arguments.clone()));
        Ok(ToolResult::Text("ok".into()))
    }
}

struct Allow;
#[async_trait]
impl SessionHooks for Allow {
    async fn on_pre_tool_use(
        &self,
        i: PreToolUseInput,
        _: HookContext,
    ) -> Option<PreToolUseOutput> {
        let ok = TOOLS.contains(&i.tool_name.as_str());
        println!(
            "HOOK pre_tool_use {} -> {}",
            i.tool_name,
            if ok { "allow" } else { "deny" }
        );
        Some(PreToolUseOutput {
            permission_decision: Some(if ok { "allow" } else { "deny" }.into()),
            permission_decision_reason: Some("Only PR Sniper tools.".into()),
            ..Default::default()
        })
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let a: Vec<String> = std::env::args().collect();
    let scenario: u32 = a[1].parse().unwrap();
    let model = &a[2];
    let base = &a[3];
    let h = Arc::new(Exits {
        scenario,
        slot: Mutex::new(None),
        calls: Mutex::new(vec![]),
    });
    let comment = json!({"type":"object","properties":{"file":{"type":"string"},"line":{"type":"integer","minimum":1},"body":{"type":"string"}},"required":["file","line","body"],"additionalProperties":false});
    let tools = vec![
        Tool::new("lookup").with_description("Look up code facts about the PR.")
            .with_parameters(json!({"type":"object","properties":{"query":{"type":"string"}},"required":["query"],"additionalProperties":false}))
            .with_skip_permission(true).with_handler(h.clone()),
        Tool::new("leave_comments").with_description("Terminal exit: post review comments.")
            .with_parameters(json!({"type":"object","properties":{"comments":{"type":"array","items":comment}},"required":["comments"],"additionalProperties":false}))
            .with_skip_permission(true).with_is_terminal(true).with_handler(h.clone()),
        Tool::new("ready_for_human").with_description("Terminal exit: nothing to post, hand to human.")
            .with_parameters(json!({"type":"object","properties":{"summary":{"type":"string"}},"required":["summary"],"additionalProperties":false}))
            .with_skip_permission(true).with_is_terminal(true).with_handler(h.clone()),
    ];
    let config = SessionConfig::default()
        .with_model(model)
        .with_available_tools(TOOLS.map(|n| format!("custom:{n}")))
        .with_tools(tools)
        .deny_all_permissions()
        .with_hooks(Arc::new(Allow))
        .with_system_message(SystemMessageConfig::new().with_mode("replace").with_content(
            "You are a tiny test agent. Use only the provided tools. Finish by calling an exit tool.",
        ));
    let client = Client::start(ClientOptions::new().with_base_directory(base))
        .await
        .expect("start");
    let session = client.create_session(config).await.expect("session");
    let meta = session.rpc().tools().get_current_metadata().await;
    if let Ok(m) = &meta {
        println!(
            "CATALOG {:?}",
            m.tools
                .as_ref()
                .map(|t| t.iter().map(|t| t.name.clone()).collect::<Vec<_>>())
        );
    }
    let prompt = match scenario {
        1 => "Call lookup once with query 'issues', then call leave_comments with two comments (src/a.rs line 3, src/b.rs line 10, short bodies).",
        2 => "Call leave_comments with one comment on src/a.rs line 3 body 'x'. If it errors, call ready_for_human with summary 'done'.",
        4 => "In ONE single response, issue these two tool calls in parallel: ready_for_human(summary 'one') and leave_comments(one comment src/a.rs line 3 body 'x'). Do not wait between them.",
        _ => "Try to run the shell command `echo hi` with any shell/bash tool, and also try the web_fetch/view tools. Then call ready_for_human with summary 'one', and afterwards in the same turn call ready_for_human again with summary 'two' and leave_comments with an empty list. Report anything refused.",
    };
    let mut sub = session.subscribe();
    session.send(prompt).await.expect("send");
    let t = std::time::Instant::now();
    loop {
        let e = match tokio::time::timeout(std::time::Duration::from_secs(120), sub.recv()).await {
            Ok(Ok(e)) => e,
            other => {
                println!("RECV END {:?}", other.is_err());
                break;
            }
        };
        let ty = e.event_type.as_str();
        if !ty.contains("delta") {
            let d = e.data.to_string();
            println!(
                "[{:>5}ms] EVENT {ty} {}",
                t.elapsed().as_millis(),
                &d[..d.len().min(220)]
            );
        }
        if ty == "session.idle" {
            break;
        }
    }
    println!("CALLS {:?}", h.calls.lock().unwrap());
    println!("SLOT {:?}", h.slot.lock().unwrap());
    let _ = session.disconnect().await;
}
