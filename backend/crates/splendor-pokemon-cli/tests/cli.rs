//! CLI 端到端冒烟测试：bot-vs-bot / PvE --auto 能跑完并产出可离线打开的 HTML。

use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use splendor_pokemon_core::{Phase, Replay};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .expect("workspace root")
}

fn tmp_dir() -> PathBuf {
    let dir = workspace_root().join("tmp");
    fs::create_dir_all(&dir).expect("create tmp dir");
    dir
}

fn run(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_splendor-pokemon"))
        .current_dir(workspace_root())
        .args(args)
        .output()
        .expect("spawn CLI")
}

fn assert_success(output: &std::process::Output) {
    assert!(
        output.status.success(),
        "CLI failed\nstdout:\n{}\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn load_replay(path: &Path) -> Replay {
    let json = fs::read_to_string(path).expect("read replay json");
    Replay::from_json(&json).expect("parse replay json")
}

fn assert_self_contained_html(path: &Path) {
    let html = fs::read_to_string(path).expect("read replay html");
    assert!(html.contains(r#"id="replay-data""#));
    assert!(
        !html.contains("__REPLAY_JSON__"),
        "template placeholder must be replaced"
    );
    for forbidden in ["http://", "https://", "cdn.", "<link"] {
        assert!(
            !html.contains(forbidden),
            "HTML must not reference external resources: {forbidden}"
        );
    }
}

#[test]
fn bot_vs_bot_finishes_and_writes_offline_replay() {
    let dir = tmp_dir();
    let stem = format!("cli-bot-{}", std::process::id());
    let json = dir.join(format!("{stem}.json"));
    let html = dir.join(format!("{stem}.html"));

    let output = run(&[
        "bot-vs-bot",
        "--players",
        "3",
        "--seed",
        "5",
        "--bot",
        "heuristic",
        "--replay",
        json.to_str().unwrap(),
        "--html",
        html.to_str().unwrap(),
    ]);
    assert_success(&output);

    let replay = load_replay(&json);
    assert!(replay.result.is_some(), "bot game must finish");
    assert!(replay.frames.len() > 2);
    assert_eq!(replay.frames.last().unwrap().view.phase, Phase::GameOver);
    assert_self_contained_html(&html);

    let _ = fs::remove_file(json);
    let _ = fs::remove_file(html);
}

#[test]
fn arena_debug_mode_runs_twenty_seeds_with_swapped_seats() {
    let output = run(&[
        "arena",
        "--candidate",
        "heuristic",
        "--baseline",
        "heuristic-v1",
        "--seeds",
        "20",
        "--seed-start",
        "1",
        "--require",
        "0",
    ]);
    assert_success(&output);

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("自定义调试种子（1–20，不消耗增量游标） × 先后手互换 = 40 局"),
        "arena must run the requested debug range, got:\n{stdout}"
    );
    assert!(stdout.contains("得分率"));
    assert!(stdout.contains("先手："));
    assert!(stdout.contains("后手："));
    assert!(stdout.contains("判定：通过"));
}

#[test]
fn arena_official_mode_advances_incremental_ledger() {
    let dir = tmp_dir();
    let ledger = dir.join(format!("cli-arena-seeds-{}.txt", std::process::id()));
    fs::write(&ledger, "21\n").expect("write ledger");

    let output = run(&[
        "arena",
        "--candidate",
        "heuristic",
        "--baseline",
        "heuristic-v1",
        "--ledger",
        ledger.to_str().unwrap(),
    ]);
    assert_success(&output);

    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("10 个固定种子（1–10）+ 10 个增量种子（21–30）"),
        "official arena must use fixed + incremental seeds, got:\n{stdout}"
    );
    assert!(stdout.contains("固定种子："));
    assert!(stdout.contains("增量种子："));
    assert!(stdout.contains("下一轮从 31 开始"));

    let next = fs::read_to_string(&ledger).expect("read ledger");
    assert_eq!(next.trim(), "31", "ledger must advance by one block");

    let _ = fs::remove_file(ledger);
}

#[test]
fn pve_auto_finishes_and_records_replay() {
    let dir = tmp_dir();
    let stem = format!("cli-pve-{}", std::process::id());
    let json = dir.join(format!("{stem}.json"));
    let html = dir.join(format!("{stem}.html"));

    let output = run(&[
        "pve",
        "--bots",
        "2",
        "--auto",
        "--seed",
        "11",
        "--replay",
        json.to_str().unwrap(),
        "--html",
        html.to_str().unwrap(),
    ]);
    assert_success(&output);

    let replay = load_replay(&json);
    let result = replay.result.as_ref().expect("pve game must finish");
    assert_eq!(replay.players.len(), 3);
    assert_eq!(result.points.len(), 3);
    assert_eq!(replay.frames.last().unwrap().view.phase, Phase::GameOver);
    assert_eq!(
        replay.controllers.first().map(String::as_str),
        Some("human")
    );
    assert_self_contained_html(&html);

    let _ = fs::remove_file(json);
    let _ = fs::remove_file(html);
}

#[test]
fn serve_accepts_http_health_and_new_game() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_splendor-pokemon"))
        .current_dir(workspace_root())
        .args(["serve", "--port", "0", "--idle-timeout", "10"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn serve");
    let stdout = child.stdout.take().expect("serve stdout");
    let child = Arc::new(Mutex::new(child));
    let done = Arc::new(AtomicBool::new(false));

    // 看门狗：启动 / 读取 / 套接字任一环节卡住时，最多 10 秒后终止子进程。
    let watchdog = {
        let child = Arc::clone(&child);
        let done = Arc::clone(&done);
        thread::spawn(move || {
            for _ in 0..100 {
                if done.load(Ordering::Relaxed) {
                    return;
                }
                thread::sleep(Duration::from_millis(100));
            }
            if let Ok(mut child) = child.lock() {
                let _ = child.kill();
            }
        })
    };

    let mut reader = BufReader::new(stdout);
    let mut address = String::new();
    for _ in 0..4 {
        let mut line = String::new();
        if reader.read_line(&mut line).unwrap_or(0) == 0 {
            break;
        }
        if let Some(position) = line.find("http://") {
            address = line[position..].trim().to_string();
            break;
        }
    }
    assert!(
        address.starts_with("http://127.0.0.1:"),
        "serve 必须打印监听地址，实际 {address:?}"
    );
    let port: u16 = address
        .rsplit(':')
        .next()
        .and_then(|value| value.parse().ok())
        .expect("serve port");

    let new_game = r#"{"seed":9,"bots":2,"bot":"heuristic","name":"你"}"#;
    let response = http_request(
        port,
        "POST /api/game/new HTTP/1.1",
        &[("Content-Type", "application/json")],
        new_game,
    );
    assert!(response.starts_with("HTTP/1.1 200 OK"), "{response}");
    assert!(response.contains("\"ok\":true"), "{response}");
    assert!(response.contains("\"seed\":9"), "{response}");
    assert!(
        response.contains("\"human_turn\":true"),
        "new game response: {response}"
    );

    let health = http_request(port, "GET /api/health HTTP/1.1", &[], "");
    assert!(health.contains("\"ok\":true"), "{health}");
    assert!(health.contains("splendor-pokemon"), "{health}");

    done.store(true, Ordering::Relaxed);
    let _ = child.lock().unwrap().kill();
    let _ = child.lock().unwrap().wait();
    let _ = watchdog.join();
}

fn http_request(port: u16, request_line: &str, headers: &[(&str, &str)], body: &str) -> String {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).expect("connect serve");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .expect("read timeout");
    let mut request = format!(
        "{request_line}\r\nHost: 127.0.0.1\r\nConnection: close\r\nContent-Length: {}\r\n",
        body.len()
    );
    for (name, value) in headers {
        request.push_str(&format!("{name}: {value}\r\n"));
    }
    request.push_str("\r\n");
    request.push_str(body);
    stream.write_all(request.as_bytes()).expect("write request");
    let mut response = String::new();
    stream.read_to_string(&mut response).expect("read response");
    response
}
