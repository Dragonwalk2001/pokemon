//! `bot-vs-bot` 与 `replay` 子命令。

use std::fs;
use std::path::{Component, Path, PathBuf};

use splendor_pokemon_core::{play_bot_game, BotKind, Replay};

/// 默认输出始终落在仓库根的 `tmp/`，不受 `cargo run` 时的当前目录影响。
pub(crate) fn default_tmp_file(name: &str) -> PathBuf {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .join("tmp")
        .join(name);
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                normalized.pop();
            }
            Component::CurDir => {}
            other => normalized.push(other.as_os_str()),
        }
    }
    normalized
}

pub(crate) fn run_bots(args: &[String]) -> Result<(), String> {
    let Some(config) = BotsConfig::from_args(args)? else {
        return Ok(());
    };
    let names: Vec<String> = (1..=config.players)
        .map(|index| format!("Bot{index}"))
        .collect();
    let kinds = vec![config.bot; config.players];

    println!(
        "bot-vs-bot：{} 人，{}，seed={}",
        config.players,
        config.bot.label_zh(),
        config.seed
    );
    let replay =
        play_bot_game(names, config.seed, &kinds).map_err(|error| format!("对局失败: {error}"))?;
    print_result(&replay);
    write_replay(&replay, &config.replay, &config.html)
}

pub(crate) fn run_render(args: &[String]) -> Result<(), String> {
    let mut input: Option<PathBuf> = None;
    let mut output: Option<PathBuf> = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--html" | "--out" => {
                index += 1;
                output = Some(PathBuf::from(args.get(index).ok_or("--html 缺少输出路径")?));
            }
            "--help" | "-h" => {
                crate::ui::print_usage();
                return Ok(());
            }
            value if input.is_none() && !value.starts_with('-') => {
                input = Some(PathBuf::from(value));
            }
            other => return Err(format!("未知参数 {other}")),
        }
        index += 1;
    }

    let input = input.ok_or("replay 需要回放 JSON 路径")?;
    let output = output.unwrap_or_else(|| input.with_extension("html"));
    let json = fs::read_to_string(&input)
        .map_err(|error| format!("读取 {} 失败: {error}", input.display()))?;
    let replay = Replay::from_json(&json)
        .map_err(|error| format!("解析 {} 失败: {error}", input.display()))?;
    let html = crate::html::render(&replay)?;
    write_file(&output, html.as_bytes())?;

    println!("已生成自包含回放: {}", output.display());
    println!("用浏览器打开该文件即可播放（无需服务器 / 联网）。");
    Ok(())
}

pub(crate) fn write_replay(
    replay: &Replay,
    json_path: &Path,
    html_path: &Path,
) -> Result<(), String> {
    let json = replay
        .to_json()
        .map_err(|error| format!("回放序列化失败: {error}"))?;
    write_file(json_path, json.as_bytes())?;
    let html = crate::html::render(replay)?;
    write_file(html_path, html.as_bytes())?;

    println!("回放 JSON: {}", json_path.display());
    println!("回放 HTML: {}", html_path.display());
    println!(
        "提示：回放 HTML 可在浏览器 file:// 直接打开：{}",
        html_path.display()
    );
    Ok(())
}

fn write_file(path: &Path, contents: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("创建目录 {} 失败: {error}", parent.display()))?;
        }
    }
    fs::write(path, contents).map_err(|error| format!("写入 {} 失败: {error}", path.display()))
}

pub(crate) fn print_result(replay: &Replay) {
    if replay.stalemate {
        println!("对局僵局：供应区耗尽且当前玩家没有任何合法主行动，规则未定义 pass，无胜者。");
        println!("动作帧：{}（含开局帧）", replay.frames.len());
        return;
    }
    let Some(result) = replay.result.as_ref() else {
        println!("对局未结束，共记录 {} 帧。", replay.frames.len());
        return;
    };
    let names: Vec<String> = result
        .winners
        .iter()
        .map(|&id| {
            let name = replay.players.get(id).map(String::as_str).unwrap_or("?");
            format!("{name}({}分)", result.points.get(id).copied().unwrap_or(0))
        })
        .collect();
    let scores: Vec<String> = replay
        .players
        .iter()
        .enumerate()
        .map(|(id, name)| format!("{name}={}", result.points.get(id).copied().unwrap_or(0)))
        .collect();
    println!("对局结束：{}", scores.join(" "));
    println!("胜者：{}", names.join("、"));
    println!("动作帧：{}（含开局帧）", replay.frames.len());
}

struct BotsConfig {
    players: usize,
    seed: u64,
    bot: BotKind,
    replay: PathBuf,
    html: PathBuf,
}

impl BotsConfig {
    fn from_args(args: &[String]) -> Result<Option<Self>, String> {
        let mut players = 2usize;
        let mut seed = 42u64;
        let mut bot = BotKind::Minimax;
        let mut replay: Option<PathBuf> = None;
        let mut html: Option<PathBuf> = None;

        let mut index = 0;
        while index < args.len() {
            match args[index].as_str() {
                "--help" | "-h" => {
                    crate::ui::print_usage();
                    return Ok(None);
                }
                "--players" => {
                    index += 1;
                    players = args
                        .get(index)
                        .ok_or("--players 缺少数值")?
                        .parse()
                        .map_err(|_| "--players 必须是 2、3 或 4".to_string())?;
                }
                "--seed" => {
                    index += 1;
                    seed = args
                        .get(index)
                        .ok_or("--seed 缺少数值")?
                        .parse()
                        .map_err(|_| "--seed 必须是整数".to_string())?;
                }
                "--bot" | "--bot-style" => {
                    index += 1;
                    let value = args.get(index).ok_or("--bot 缺少数值")?;
                    bot = BotKind::parse(value).ok_or_else(|| {
                        format!(
                            "未知 bot {value:?}，可用 heuristic / minimax / heuristic-v1 / random"
                        )
                    })?;
                }
                "--replay" => {
                    index += 1;
                    replay = Some(PathBuf::from(args.get(index).ok_or("--replay 缺少路径")?));
                }
                "--html" => {
                    index += 1;
                    html = Some(PathBuf::from(args.get(index).ok_or("--html 缺少路径")?));
                }
                other => return Err(format!("未知参数 {other}")),
            }
            index += 1;
        }

        if !(2..=4).contains(&players) {
            return Err(format!("players 必须为 2–4，当前为 {players}"));
        }

        let default_stem = format!("bot-{seed}-{players}p");
        Ok(Some(Self {
            players,
            seed,
            bot,
            replay: replay.unwrap_or_else(|| default_tmp_file(&format!("{default_stem}.json"))),
            html: html.unwrap_or_else(|| default_tmp_file(&format!("{default_stem}.html"))),
        }))
    }
}
