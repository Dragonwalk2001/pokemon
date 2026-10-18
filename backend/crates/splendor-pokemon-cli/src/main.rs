//! CLI 入口：默认热座对局，另有 PvE、bot-vs-bot、arena 回测与回放渲染子命令。

mod arena;
mod bot_cmd;
mod html;
mod pve;
mod ui;

use std::env;
use std::fs;
use std::io::{self, Write};
use std::process::ExitCode;

use splendor_pokemon_core::Game;

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("pve") => pve::run(&args[1..]),
        Some("bot-vs-bot") | Some("bots") => bot_cmd::run_bots(&args[1..]),
        Some("arena") | Some("bench") => arena::run(&args[1..]),
        Some("replay") | Some("html") => bot_cmd::run_render(&args[1..]),
        Some("--help") | Some("-h") => {
            ui::print_usage();
            Ok(())
        }
        _ => run_hotseat(&args),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

fn run_hotseat(args: &[String]) -> Result<(), String> {
    let Some(config) = Config::from_args(args.to_vec())? else {
        return Ok(());
    };

    let mut game =
        Game::new(config.names, config.seed).map_err(|error| format!("无法创建对局: {error}"))?;

    println!("璀璨宝石：宝可梦 — Rust 规则引擎 CLI（本地热座）");
    println!("输入 help 查看命令，quit 退出。");
    ui::print_state(&game);

    let mut printed_events = game.log().len();
    let stdin = io::stdin();

    loop {
        print!("\n{}> ", game.player_name(game.current_player()));
        io::stdout().flush().ok();

        let mut line = String::new();
        match stdin.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {}
            Err(error) => {
                eprintln!("读取输入失败: {error}");
                break;
            }
        }

        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.is_empty() {
            continue;
        }

        match ui::parse_input(&parts) {
            Ok(ui::Input::Quit) => break,
            Ok(ui::Input::Help) => ui::print_help(),
            Ok(ui::Input::Show) => ui::print_state(&game),
            Ok(ui::Input::Legal) => ui::print_legal(&game),
            Ok(ui::Input::Save(path)) => match serde_json::to_string_pretty(&game) {
                Ok(json) => match fs::write(&path, json) {
                    Ok(()) => println!("已保存到 {path}"),
                    Err(error) => println!("保存失败: {error}"),
                },
                Err(error) => println!("序列化失败: {error}"),
            },
            Ok(ui::Input::Load(path)) => match fs::read_to_string(&path) {
                Ok(json) => match serde_json::from_str::<Game>(&json) {
                    Ok(loaded) => {
                        game = loaded;
                        printed_events = game.log().len();
                        println!("已从 {path} 读取对局");
                        ui::print_state(&game);
                    }
                    Err(error) => println!("解析失败: {error}"),
                },
                Err(error) => println!("读取失败: {error}"),
            },
            Ok(ui::Input::Action(action)) => {
                let current = game.current_player();
                match game.apply(current, action) {
                    Ok(()) => {
                        while printed_events < game.log().len() {
                            println!("· {}", game.log()[printed_events].describe(&game));
                            printed_events += 1;
                        }
                        ui::print_state(&game);
                        if game.is_over() {
                            break;
                        }
                    }
                    Err(error) => println!("动作不合法: {error}"),
                }
            }
            Err(message) => println!("命令错误: {message}（输入 help 查看用法）"),
        }
    }

    if game.is_over() {
        println!("\n对局已结束。");
    }
    Ok(())
}

struct Config {
    names: Vec<String>,
    seed: u64,
}

impl Config {
    fn from_args(args: Vec<String>) -> Result<Option<Self>, String> {
        let mut names: Option<Vec<String>> = None;
        let mut players = 2usize;
        let mut seed = 42u64;
        let mut index = 0;

        while index < args.len() {
            match args[index].as_str() {
                "--help" | "-h" => {
                    ui::print_usage();
                    return Ok(None);
                }
                "--seed" => {
                    index += 1;
                    seed = args
                        .get(index)
                        .ok_or("--seed 缺少数值")?
                        .parse()
                        .map_err(|_| "--seed 必须是整数".to_string())?;
                }
                "--players" => {
                    index += 1;
                    players = args
                        .get(index)
                        .ok_or("--players 缺少数值")?
                        .parse()
                        .map_err(|_| "--players 必须是 2、3 或 4".to_string())?;
                }
                "--names" => {
                    index += 1;
                    let raw = args.get(index).ok_or("--names 缺少名字列表")?;
                    names = Some(raw.split(',').map(|name| name.trim().to_string()).collect());
                }
                other => return Err(format!("未知参数 {other}")),
            }
            index += 1;
        }

        let names = match names {
            Some(names) => names,
            None => (1..=players).map(|index| format!("玩家{index}")).collect(),
        };
        if !(2..=4).contains(&names.len()) {
            return Err(format!("玩家数必须为 2–4，当前为 {}", names.len()));
        }

        Ok(Some(Self { names, seed }))
    }
}
