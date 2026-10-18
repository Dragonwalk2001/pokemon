//! PvE：1 名人类 + 2–3 个 bot，终端交互。
//!
//! 人类始终以玩家 0 的视角查看，bot 的保留牌不会泄露。
//! `--auto` 让启发式 bot 代替人类席位，走的是同一条循环，便于演示与自动化测试。

use std::io::{self, Write};
use std::path::PathBuf;

use splendor_pokemon_core::{make_bot, Bot, BotKind, Game, Phase, ReplayRecorder};

use crate::ui;

pub(crate) fn run(args: &[String]) -> Result<(), String> {
    let Some(config) = Config::from_args(args)? else {
        return Ok(());
    };
    let players = config.bots + 1;
    let names: Vec<String> = std::iter::once(config.name.clone())
        .chain((1..=config.bots).map(|index| format!("Bot{index}")))
        .collect();
    let mut game =
        Game::new(names, config.seed).map_err(|error| format!("无法创建对局: {error}"))?;

    let mut bots: Vec<Option<Box<dyn Bot>>> = (0..players)
        .map(|player| {
            if player == 0 {
                None
            } else {
                Some(make_bot(config.bot, player, config.seed))
            }
        })
        .collect();
    let mut auto_human = config
        .auto
        .then(|| make_bot(BotKind::Heuristic, 0, config.seed));

    let controllers: Vec<String> = std::iter::once("human".to_string())
        .chain((1..players).map(|_| config.bot.as_str().to_string()))
        .collect();
    let mut recorder = ReplayRecorder::new(&game, controllers);

    println!(
        "璀璨宝石：宝可梦 — PvE（{} 人：你 + {} 个{}，seed={}）",
        players,
        config.bots,
        config.bot.label_zh(),
        config.seed
    );
    if config.auto {
        println!("--auto 已开启：人类席位由启发式 bot 代打。");
    } else {
        println!("输入 help 查看命令，quit 退出。");
    }
    ui::print_view(&game.view_for(0), 0);

    let mut printed_events = game.log().len();
    let stdin = io::stdin();
    let mut stalemate = false;

    loop {
        if game.is_over() {
            break;
        }
        // 规则层僵局：供应区没有普通球且无人能捕捉。引擎不提供 pass，直接收尾。
        if game.phase() == Phase::MainAction && game.legal_actions().is_empty() {
            stalemate = true;
            break;
        }
        let current = game.current_player();

        if current != 0 {
            let view = game.view_for(current);
            let bot = bots[current]
                .as_mut()
                .ok_or_else(|| format!("玩家 {current} 不是 bot"))?;
            let Some(action) = bot.choose_action(&view) else {
                stalemate = true;
                break;
            };

            println!("\n[{}] {}", game.player_name(current), action);
            game.apply(current, action.clone())
                .map_err(|error| format!("bot 动作不合法: {error}"))?;
            recorder.record(&game, current, &action);
            printed_events = print_events(&game, printed_events);
            if game.is_over() {
                break;
            }
            ui::print_view(&game.view_for(0), 0);
            continue;
        }

        if let Some(bot) = auto_human.as_mut() {
            let view = game.view_for(0);
            let Some(action) = bot.choose_action(&view) else {
                stalemate = true;
                break;
            };
            println!("\n[你 · auto] {action}");
            game.apply(0, action.clone())
                .map_err(|error| format!("auto 动作不合法: {error}"))?;
            recorder.record(&game, 0, &action);
            printed_events = print_events(&game, printed_events);
            if game.is_over() {
                break;
            }
            ui::print_view(&game.view_for(0), 0);
            continue;
        }

        print!("\n你> ");
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
            Ok(ui::Input::Quit) => {
                println!("已退出 PvE（未结束的对局不写入回放）");
                return Ok(());
            }
            Ok(ui::Input::Help) => ui::print_help(),
            Ok(ui::Input::Show) => ui::print_view(&game.view_for(0), 0),
            Ok(ui::Input::Legal) => ui::print_legal(&game),
            Ok(ui::Input::Save(_)) | Ok(ui::Input::Load(_)) => {
                println!("PvE 模式下不支持 save / load（回放 JSON 会在终局自动生成）");
            }
            Ok(ui::Input::Action(action)) => match game.apply(0, action.clone()) {
                Ok(()) => {
                    recorder.record(&game, 0, &action);
                    printed_events = print_events(&game, printed_events);
                    if game.is_over() {
                        break;
                    }
                    ui::print_view(&game.view_for(0), 0);
                }
                Err(error) => println!("动作不合法: {error}"),
            },
            Err(message) => println!("命令错误: {message}（输入 help 查看用法）"),
        }
    }

    let replay = if stalemate {
        recorder.finish_stalemate(&game)
    } else {
        recorder.finish(&game)
    };
    if game.is_over() || stalemate {
        if stalemate {
            println!("\n对局进入规则僵局。");
        } else {
            println!("\n对局已结束。");
        }
        crate::bot_cmd::print_result(&replay);
        crate::bot_cmd::write_replay(&replay, &config.replay, &config.html)?;
    } else {
        println!("\n对局中止，未写入回放。");
    }
    Ok(())
}

fn print_events(game: &Game, from: usize) -> usize {
    let mut index = from;
    while index < game.log().len() {
        println!("· {}", game.log()[index].describe(game));
        index += 1;
    }
    index
}

struct Config {
    seed: u64,
    bots: usize,
    bot: BotKind,
    name: String,
    auto: bool,
    replay: PathBuf,
    html: PathBuf,
}

impl Config {
    fn from_args(args: &[String]) -> Result<Option<Self>, String> {
        let mut seed = 42u64;
        let mut bots = 2usize;
        let mut bot = BotKind::Heuristic;
        let mut name = "你".to_string();
        let mut auto = false;
        let mut replay: Option<PathBuf> = None;
        let mut html: Option<PathBuf> = None;

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
                "--bots" => {
                    index += 1;
                    bots = args
                        .get(index)
                        .ok_or("--bots 缺少数值")?
                        .parse()
                        .map_err(|_| "--bots 必须是 2 或 3".to_string())?;
                }
                "--bot" => {
                    index += 1;
                    let value = args.get(index).ok_or("--bot 缺少数值")?;
                    bot = BotKind::parse(value).ok_or_else(|| {
                        format!("未知 bot {value:?}，可用 heuristic / heuristic-v1 / random")
                    })?;
                }
                "--name" => {
                    index += 1;
                    name = args.get(index).ok_or("--name 缺少名字")?.clone();
                }
                "--auto" => auto = true,
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

        if !(2..=3).contains(&bots) {
            return Err(format!("bots 必须为 2 或 3，当前为 {bots}"));
        }
        let players = bots + 1;
        let default_stem = format!("pve-{seed}-{players}p");

        Ok(Some(Self {
            seed,
            bots,
            bot,
            name,
            auto,
            replay: replay.unwrap_or_else(|| {
                crate::bot_cmd::default_tmp_file(&format!("{default_stem}.json"))
            }),
            html: html.unwrap_or_else(|| {
                crate::bot_cmd::default_tmp_file(&format!("{default_stem}.html"))
            }),
        }))
    }
}
