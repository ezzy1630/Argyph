#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};

fn copy_dir_all(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let ty = entry.file_type()?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        if ty.is_dir() {
            copy_dir_all(&src_path, &dst_path)?;
        } else if ty.is_symlink() {
            let target = std::fs::read_link(&src_path)?;
            #[cfg(unix)]
            {
                std::os::unix::fs::symlink(&target, &dst_path)?;
            }
            #[cfg(windows)]
            {
                let _ = target;
                std::fs::copy(&src_path, &dst_path)?;
            }
        } else {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

struct Fixture {
    _dir: tempfile::TempDir,
    root: std::path::PathBuf,
}

fn setup_fixture() -> Fixture {
    let dir = tempfile::tempdir().unwrap();
    let src = std::path::Path::new(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../examples/tiny-rust-app"
    ));
    let dst = dir.path().join("repo");
    copy_dir_all(src, &dst).unwrap();
    Fixture {
        _dir: dir,
        root: dst,
    }
}

fn send(w: &mut impl Write, msg: &serde_json::Value) {
    let mut payload = serde_json::to_vec(msg).unwrap();
    payload.push(b'\n');
    w.write_all(&payload).unwrap();
    w.flush().unwrap();
}

fn recv(r: &mut BufReader<impl std::io::Read>) -> serde_json::Value {
    let mut line = String::new();
    r.read_line(&mut line).unwrap();
    serde_json::from_str(&line).unwrap()
}

fn spawn_serve(
    root: &std::path::Path,
) -> (
    Child,
    BufReader<std::process::ChildStdout>,
    std::process::ChildStdin,
) {
    let bin = env!("CARGO_BIN_EXE_argyph");
    let mut child = Command::new(bin)
        .arg("serve")
        .current_dir(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();

    let reader = BufReader::new(child.stdout.take().unwrap());
    let writer = child.stdin.take().unwrap();
    (child, reader, writer)
}

#[test]
fn mcp_initialize_and_list_tools() {
    let fixture = setup_fixture();
    let (_child, mut reader, mut writer) = spawn_serve(&fixture.root);

    // 1. Initialize
    let init_req = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "smoke-test", "version": "1.0"}
        }
    });
    send(&mut writer, &init_req);
    let init_resp = recv(&mut reader);
    assert!(
        init_resp["result"].is_object(),
        "initialize failed: {init_resp}"
    );
    assert_eq!(
        init_resp["result"]["serverInfo"]["name"], "argyph",
        "wrong server name: {init_resp}"
    );

    // 2. Send initialized notification
    let initialized = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "notifications/initialized"
    });
    send(&mut writer, &initialized);

    // 3. List tools
    let list_req = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/list",
        "params": {}
    });
    send(&mut writer, &list_req);
    let list_resp = recv(&mut reader);
    let tools = list_resp["result"]["tools"]
        .as_array()
        .expect("tools missing");
    assert_eq!(tools.len(), 19, "expected 19 tools, got {tools:?}");
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert!(names.contains(&"get_index_status"));
    assert!(names.contains(&"ask"));
    assert!(names.contains(&"expand_span"));
    assert!(names.contains(&"get_repo_overview"));
    assert!(names.contains(&"search_text"));
    assert!(names.contains(&"search_semantic"));
    assert!(names.contains(&"find_definition"));
    assert!(names.contains(&"find_references"));
    assert!(names.contains(&"get_callers"));
    assert!(names.contains(&"get_callees"));
    assert!(names.contains(&"get_imports"));
    assert!(names.contains(&"get_symbol_outline"));
    assert!(names.contains(&"pack_repo"));
    assert!(names.contains(&"locate"));
    assert!(names.contains(&"locate_smart"));
    assert!(names.contains(&"memory_save"));
    assert!(names.contains(&"memory_search"));
    assert!(names.contains(&"memory_list"));
    assert!(names.contains(&"memory_forget"));
}

#[test]
fn call_get_index_status_returns_tier_info() {
    let fixture = setup_fixture();
    let (_child, mut reader, mut writer) = spawn_serve(&fixture.root);

    let init_req = serde_json::json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "smoke-test", "version": "1.0"}
        }
    });
    send(&mut writer, &init_req);
    recv(&mut reader);

    let initialized = serde_json::json!({
        "jsonrpc": "2.0", "method": "notifications/initialized"
    });
    send(&mut writer, &initialized);

    let call_req = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "tools/call",
        "params": {
            "name": "get_index_status",
            "arguments": {}
        }
    });
    send(&mut writer, &call_req);
    let call_resp = recv(&mut reader);
    assert!(
        call_resp["result"].is_object(),
        "tool call failed: {call_resp}"
    );
    let content = &call_resp["result"]["content"];
    assert!(content.is_array(), "expected content array: {content}");
    let text = content[0]["text"].as_str().expect("text field missing");
    let body: serde_json::Value = serde_json::from_str(text).unwrap();
    assert!(
        body["tiers"]["files"]["ready"].as_bool().unwrap_or(false),
        "expected Tier 0 ready, got: {body}"
    );
    assert!(
        body["tiers"]["files"]["count"].as_u64().unwrap_or(0) > 0,
        "expected files indexed"
    );
    assert!(
        body["tiers"]["symbols"].is_object(),
        "expected symbols tier info: {body}"
    );
    assert!(
        body["tiers"]["structural"].is_object(),
        "expected Tier 1.5 structural tier info: {body}"
    );
    assert!(
        body["tiers"]["structural"]["count"].is_u64(),
        "expected Tier 1.5 structural count to be reported: {body}"
    );
}

#[test]
fn call_search_text_finds_match() {
    let fixture = setup_fixture();
    let (_child, mut reader, mut writer) = spawn_serve(&fixture.root);

    let init_req = serde_json::json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "smoke-test", "version": "1.0"}
        }
    });
    send(&mut writer, &init_req);
    recv(&mut reader);

    let initialized = serde_json::json!({
        "jsonrpc": "2.0", "method": "notifications/initialized"
    });
    send(&mut writer, &initialized);

    let call_req = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 4,
        "method": "tools/call",
        "params": {
            "name": "search_text",
            "arguments": {
                "pattern": "fn",
                "regex": false,
                "case_sensitive": true,
                "max_results": 10
            }
        }
    });
    send(&mut writer, &call_req);
    let call_resp = recv(&mut reader);
    assert!(
        !call_resp["result"]["isError"].as_bool().unwrap_or(false),
        "tool returned error: {call_resp}"
    );
    let content = &call_resp["result"]["content"];
    let text = content[0]["text"].as_str().expect("text field missing");
    let body: serde_json::Value = serde_json::from_str(text).unwrap();
    let hits = body["hits"].as_array().expect("hits missing");
    assert!(
        !hits.is_empty(),
        "expected at least one hit for 'fn', got: {body}"
    );
}

#[test]
fn call_get_repo_overview_returns_languages_and_tree() {
    let fixture = setup_fixture();
    let (_child, mut reader, mut writer) = spawn_serve(&fixture.root);

    let init_req = serde_json::json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "smoke-test", "version": "1.0"}
        }
    });
    send(&mut writer, &init_req);
    recv(&mut reader);

    let initialized = serde_json::json!({
        "jsonrpc": "2.0", "method": "notifications/initialized"
    });
    send(&mut writer, &initialized);

    let call_req = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 5,
        "method": "tools/call",
        "params": {
            "name": "get_repo_overview",
            "arguments": { "max_tree_depth": 3 }
        }
    });
    send(&mut writer, &call_req);
    let call_resp = recv(&mut reader);
    assert!(
        !call_resp["result"]["isError"].as_bool().unwrap_or(false),
        "tool returned error: {call_resp}"
    );
    let content = &call_resp["result"]["content"];
    let text = content[0]["text"].as_str().expect("text field missing");
    let body: serde_json::Value = serde_json::from_str(text).unwrap();
    let languages = body["languages"].as_array().expect("languages missing");
    assert!(!languages.is_empty(), "expected languages: {body}");
    let tree = body["tree"].as_str().expect("tree missing");
    assert!(!tree.is_empty(), "expected non-empty tree");
}
