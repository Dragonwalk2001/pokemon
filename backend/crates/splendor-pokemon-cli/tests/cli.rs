//! CLI 端到端冒烟测试：bot-vs-bot / PvE --auto 能跑完并产出可离线打开的 HTML。

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

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
