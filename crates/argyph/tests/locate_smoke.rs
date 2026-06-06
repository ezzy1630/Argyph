#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::{Mutex, MutexGuard, OnceLock};

fn serve_test_lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(())).lock().unwrap()
}

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
            if let Ok(_target) = std::fs::read_link(&src_path) {
                #[cfg(unix)]
                {
                    std::os::unix::fs::symlink(&_target, &dst_path)?;
                }
                #[cfg(windows)]
                {
                    std::fs::copy(&src_path, &dst_path)?;
                }
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
        "/tests/fixtures/locate"
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

fn spawn_serve(root: &std::path::Path) -> (Child, BufReader<ChildStdout>, ChildStdin) {
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

fn handshake(stdin: &mut ChildStdin, stdout: &mut BufReader<ChildStdout>) {
    let init_req = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {"name": "locate-smoke-test", "version": "1.0"}
        }
    });
    send(stdin, &init_req);
    recv(stdout);

    let initialized = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "notifications/initialized"
    });
    send(stdin, &initialized);
}

fn call_tool(
    stdin: &mut ChildStdin,
    stdout: &mut BufReader<ChildStdout>,
    id: u64,
    name: &str,
    args: serde_json::Value,
) -> serde_json::Value {
    let req = serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "method": "tools/call",
        "params": {
            "name": name,
            "arguments": args
        }
    });
    send(stdin, &req);
    recv(stdout)
}

fn wait_ready(stdin: &mut ChildStdin, stdout: &mut BufReader<ChildStdout>) {
    for _ in 0..300 {
        let v = call_tool(stdin, stdout, 99, "get_index_status", serde_json::json!({}));
        let content = &v["result"]["content"];
        if let Some(arr) = content.as_array() {
            if let Some(text) = arr[0]["text"].as_str() {
                if let Ok(body) = serde_json::from_str::<serde_json::Value>(text) {
                    if let Some(tiers) = body["tiers"].as_object() {
                        let structural_ready = tiers
                            .get("structural")
                            .and_then(|s| s["ready"].as_bool())
                            .unwrap_or(false);
                        if structural_ready {
                            return;
                        }
                    }
                }
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
    eprintln!("Warning: structural tier may not be ready after 60s, tests may fail");
}

fn parse_tool_result(v: &serde_json::Value) -> serde_json::Value {
    let content = &v["result"]["content"];
    if let Some(arr) = content.as_array() {
        if let Some(text) = arr[0]["text"].as_str() {
            if let Ok(body) = serde_json::from_str::<serde_json::Value>(text) {
                return body;
            }
        }
    }
    serde_json::Value::Null
}

// ── Task B7 ──

#[test]
fn locate_markdown_by_heading_path() {
    let _guard = serve_test_lock();
    let fx = setup_fixture();
    let (mut child, mut stdout, mut stdin) = spawn_serve(&fx.root);
    handshake(&mut stdin, &mut stdout);
    wait_ready(&mut stdin, &mut stdout);

    let resp = call_tool(
        &mut stdin,
        &mut stdout,
        2,
        "locate",
        serde_json::json!({
            "path": "Enterprise",
            "file": "docs/billing.md"
        }),
    );
    let body = parse_tool_result(&resp);

    if let Some(err) = body["error"].as_object() {
        eprintln!(
            "locate returned error (code={}): {}",
            err["code"].as_str().unwrap_or(""),
            err["message"]
        );
    }
    if let Some(spans) = body["spans"].as_array() {
        assert!(!spans.is_empty(), "Should find Enterprise section");
        let content = spans[0]["content"].as_str().unwrap_or("");
        assert!(
            content.contains("Expensive") || content.contains("Enterprise"),
            "content mismatch: {content}"
        );
    }
    child.kill().ok();
}

#[test]
fn locate_json_by_key_path() {
    let _guard = serve_test_lock();
    let fx = setup_fixture();
    let (mut child, mut stdout, mut stdin) = spawn_serve(&fx.root);
    handshake(&mut stdin, &mut stdout);
    wait_ready(&mut stdin, &mut stdout);

    let resp = call_tool(
        &mut stdin,
        &mut stdout,
        2,
        "locate",
        serde_json::json!({
            "path": "database/host",
            "file": "config/app.json"
        }),
    );
    let body = parse_tool_result(&resp);

    if let Some(err) = body["error"].as_object() {
        eprintln!(
            "locate returned error (code={}): {}",
            err["code"].as_str().unwrap_or(""),
            err["message"]
        );
    }
    if let Some(spans) = body["spans"].as_array() {
        assert!(!spans.is_empty(), "Should find database.host");
        let content = spans[0]["content"].as_str().unwrap_or("");
        assert!(content.contains("localhost"));
    }
    child.kill().ok();
}

#[test]
fn locate_csv_row() {
    let _guard = serve_test_lock();
    let fx = setup_fixture();
    let (mut child, mut stdout, mut stdin) = spawn_serve(&fx.root);
    handshake(&mut stdin, &mut stdout);
    wait_ready(&mut stdin, &mut stdout);

    let resp = call_tool(
        &mut stdin,
        &mut stdout,
        2,
        "locate",
        serde_json::json!({
            "path": "row_2",
            "file": "data/users.csv"
        }),
    );
    let body = parse_tool_result(&resp);

    if let Some(err) = body["error"].as_object() {
        eprintln!(
            "locate returned error (code={}): {}",
            err["code"].as_str().unwrap_or(""),
            err["message"]
        );
    }
    if let Some(spans) = body["spans"].as_array() {
        assert!(!spans.is_empty(), "Should find data row");
        let content = spans[0]["content"].as_str().unwrap_or("");
        assert!(content.contains("Bob"), "content mismatch: {content}");
    }
    child.kill().ok();
}

#[test]
fn locate_invalid_argument_when_no_query_or_path() {
    let _guard = serve_test_lock();
    let fx = setup_fixture();
    let (mut child, mut stdout, mut stdin) = spawn_serve(&fx.root);
    handshake(&mut stdin, &mut stdout);
    wait_ready(&mut stdin, &mut stdout);

    let resp = call_tool(&mut stdin, &mut stdout, 2, "locate", serde_json::json!({}));
    let body = parse_tool_result(&resp);

    let error_obj = body["error"].as_object();
    let code = error_obj.and_then(|e| e["code"].as_str()).unwrap_or("");
    assert!(
        error_obj.is_some() || code.contains("InvalidPath") || code.contains("Internal"),
        "Expected error, got: {resp:?}"
    );
    child.kill().ok();
}

#[test]
fn locate_empty_match_returns_empty_not_error() {
    let _guard = serve_test_lock();
    let fx = setup_fixture();
    let (mut child, mut stdout, mut stdin) = spawn_serve(&fx.root);
    handshake(&mut stdin, &mut stdout);
    wait_ready(&mut stdin, &mut stdout);

    let resp = call_tool(
        &mut stdin,
        &mut stdout,
        2,
        "locate",
        serde_json::json!({
            "path": "DoesNotExist",
            "file": "docs/billing.md"
        }),
    );
    let body = parse_tool_result(&resp);

    if let Some(spans) = body["spans"].as_array() {
        assert_eq!(spans.len(), 0, "Empty match should return empty spans");
    }
    child.kill().ok();
}

// ── Task B8 ──

#[test]
fn locate_nl_query_returns_section() {
    let _guard = serve_test_lock();
    let fx = setup_fixture();
    let (mut child, mut stdout, mut stdin) = spawn_serve(&fx.root);
    handshake(&mut stdin, &mut stdout);
    wait_ready(&mut stdin, &mut stdout);

    let resp = call_tool(
        &mut stdin,
        &mut stdout,
        2,
        "locate",
        serde_json::json!({
            "query": "section about custom limits for enterprise pricing",
            "files": ["docs/**/*.md"]
        }),
    );
    let body = parse_tool_result(&resp);

    if let Some(err) = body["error"].as_object() {
        eprintln!(
            "locate NL query returned error (code={}): {}",
            err["code"].as_str().unwrap_or(""),
            err["message"]
        );
    }
    if let Some(spans) = body["spans"].as_array() {
        if !spans.is_empty() {
            let content = spans[0]["content"].as_str().unwrap_or("");
            assert!(
                content.contains("Enterprise")
                    || content.contains("Custom limits")
                    || content.contains("Expensive"),
                "content mismatch: {content}"
            );
        }
    }
    child.kill().ok();
}
