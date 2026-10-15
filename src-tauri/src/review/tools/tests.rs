use super::*;
use crate::exits::Exit;

#[test]
fn the_original_read_definitions_are_byte_compatible() {
    let schema = |name: &str| definition(name).unwrap().schema().to_string();
    assert_eq!(
        schema("read_changes"),
        r#"{"additionalProperties":false,"properties":{"max_bytes":{"minimum":1,"type":"integer"},"offset":{"minimum":0,"type":"integer"},"part":{"enum":["patch","before","after"],"type":"string"},"path":{"type":"string"}},"required":["path","part"],"type":"object"}"#
    );
    assert_eq!(
        schema("read_source"),
        r#"{"additionalProperties":false,"properties":{"max_bytes":{"minimum":1,"type":"integer"},"offset":{"minimum":0,"type":"integer"},"path":{"type":"string"},"side":{"enum":["base","head"],"type":"string"}},"required":["side","path"],"type":"object"}"#
    );
    assert!(definition("search_paths").is_none());
    assert!(definition("read_changes")
        .unwrap()
        .description
        .starts_with("Read one part (patch, before or after) of one changed file in byte chunks."));
}

#[test]
fn every_packet_investigation_tool_has_a_registry_entry() {
    for name in INVESTIGATION {
        let def = definition(name).unwrap();
        assert_eq!(def.name, name);
        assert_eq!(def.schema()["additionalProperties"], false);
        assert!(crate::storage::PACKET_TOOLS.contains(&name));
    }
    assert!(definition("bash").is_none());
    assert!(definition("leave_comments").is_none());
}

#[test]
fn a_runs_tool_set_is_granted_investigation_tools_then_offered_exits() {
    let granted: Vec<String> = ["search_code", "ready_for_human", "read_changes", "bash"]
        .map(String::from)
        .to_vec();
    assert_eq!(
        tool_set(&granted, false, &[Exit::ReadyForHuman, Exit::PanicExit]),
        [
            "read_changes",
            "search_code",
            "ready_for_human",
            "panic_exit"
        ]
    );
    assert!(tool_set(&[], false, &[]).is_empty());
}

#[test]
fn lists_page_without_loss_and_reject_bad_offsets() {
    let items: Vec<Value> = (0..5_000).map(|n| json!(format!("path/{n}.rs"))).collect();
    let mut limit = false;
    let mut offset = 0;
    let mut seen = 0;
    loop {
        let page = page_items("paths", &items, &json!({"offset": offset}), &mut limit).unwrap();
        assert!(page.to_string().len() <= MAX_TOOL_BYTES);
        seen += page["paths"].as_array().unwrap().len();
        match page["next_offset"].as_u64() {
            Some(next) => offset = next as usize,
            None => break,
        }
    }
    assert_eq!((seen, limit), (5_000, false));
    assert!(page_items("paths", &items, &json!({"offset": 5_001}), &mut limit).is_err());
    assert_eq!(
        page_items("paths", &[], &json!({}), &mut limit).unwrap()["complete"],
        true
    );
    let huge = vec![json!("x".repeat(MAX_TOOL_BYTES + 1))];
    assert!(page_items("paths", &huge, &json!({}), &mut limit).is_err());
    assert!(limit);
}

#[test]
fn snippets_cut_on_char_boundaries_and_say_so() {
    assert_eq!(snippet("short"), ("short".into(), false));
    let (cut, truncated) = snippet(&"é".repeat(150));
    assert!(truncated && cut.len() <= SNIPPET_BYTES && cut.chars().all(|c| c == 'é'));
}

#[test]
fn logs_lose_ansi_codes_and_leading_timestamps_only() {
    let raw = "\u{feff}2026-10-13T09:56:17.8361234Z \u{1b}[36;1mRun cargo test\u{1b}[0m\r\n\
        2026-10-13T09:56:18Z \u{1b}]8;;https://x\u{7}link\u{1b}]8;;\u{1b}\\ done\n\
        2026-10-13T09:56:19.1Z\n\
        no stamp 2026-10-13T09:56:17Z kept\n\
        2026-10-13 09:56:17Z not iso\n\
        \u{1b}(Bplain";
    assert_eq!(
        clean_log(raw.as_bytes(), false),
        "Run cargo test\nlink done\n\nno stamp 2026-10-13T09:56:17Z kept\n2026-10-13 09:56:17Z not iso\nplain"
    );
    assert_eq!(clean_log(b"partial line\nwhole\n", true), "whole\n");
    assert_eq!(
        clean_log(b"<untrusted>x</UNTRUSTED>", false),
        "&lt;untrusted>x&lt;/UNTRUSTED>"
    );
    assert_eq!(clean_log(&[0xff, b'a'], false), "\u{fffd}a");
}

#[test]
fn log_pages_from_the_end_fit_the_limit_and_reach_the_last_byte() {
    let check = CheckFact {
        name: "build".into(),
        status: "COMPLETED".into(),
        conclusion: "FAILURE".into(),
        id: Some("7".into()),
        log: true,
    };
    // Quotes double on JSON escape: the first window from the end cannot fit.
    let text = format!("{}tail", "\"".repeat(100_000));
    let mut limit = false;
    let last = log_page(&check, &text, true, &json!({"from_end": true}), &mut limit).unwrap();
    assert!(last.to_string().len() <= MAX_TOOL_BYTES);
    assert_eq!(last["complete"], true);
    assert_eq!(last["truncated_head"], true);
    assert!(last["content"].as_str().unwrap().ends_with("tail"));
    assert!(last["previous_offset"].as_u64().is_some());
    let first = log_page(&check, &text, true, &json!({}), &mut limit).unwrap();
    assert!(first["previous_offset"].is_null());
    assert_eq!(
        first["next_offset"],
        first["content"].as_str().unwrap().len()
    );
    let short = log_page(
        &check,
        "ok\n",
        false,
        &json!({"from_end": true}),
        &mut limit,
    )
    .unwrap();
    assert_eq!(
        (short["offset"].as_u64(), short["previous_offset"].is_null()),
        (Some(0), true)
    );
    assert!(log_page(&check, "x", false, &json!({"from_end": "yes"}), &mut limit).is_err());
    assert!(!limit);
}

#[test]
fn read_check_log_is_an_investigation_tool_in_the_role_defaults() {
    use crate::storage::{default_tools, AssignmentAuthority};
    let defaults = |primary| {
        default_tools(&AssignmentAuthority {
            primary,
            comment: false,
            reply: false,
            approve: false,
            merge: false,
        })
    };
    assert!(INVESTIGATION.contains(&"read_check_log"));
    assert!(defaults(true).contains(&"read_check_log".to_string()));
    assert!(defaults(false).contains(&"read_check_log".to_string()));
    assert_eq!(
        crate::storage::diagnostics::ToolName::from_name("read_check_log"),
        crate::storage::diagnostics::ToolName::ReadCheckLog
    );
}
