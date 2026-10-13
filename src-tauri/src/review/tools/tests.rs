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
        tool_set(&granted, &[Exit::ReadyForHuman, Exit::PanicExit]),
        [
            "read_changes",
            "search_code",
            "ready_for_human",
            "panic_exit"
        ]
    );
    assert!(tool_set(&[], &[]).is_empty());
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
