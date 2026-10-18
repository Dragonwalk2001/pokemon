//! 交互式 CLI：直接驱动规则引擎，用于人工验证与规则演示。

use std::env;
use std::fs;
use std::io::{self, Write};

use splendor_pokemon_core::{Action, Card, CardSource, Category, Color, Game, Phase, TokenCounts};

fn main() {
    let config = match Config::from_args(env::args().skip(1).collect()) {
        Ok(Some(config)) => config,
        Ok(None) => return,
        Err(message) => {
            eprintln!("参数错误: {message}");
            print_usage();
            return;
        }
    };

    let mut game = match Game::new(config.names, config.seed) {
        Ok(game) => game,
        Err(error) => {
            eprintln!("无法创建对局: {error}");
            return;
        }
    };

    println!("璀璨宝石：宝可梦 — Rust 规则引擎 CLI");
    println!("输入 help 查看命令，quit 退出。");
    print_state(&game);

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

        match parse_input(&parts) {
            Ok(Input::Quit) => break,
            Ok(Input::Help) => print_help(),
            Ok(Input::Show) => print_state(&game),
            Ok(Input::Legal) => print_legal(&game),
            Ok(Input::Save(path)) => match serde_json::to_string_pretty(&game) {
                Ok(json) => match fs::write(&path, json) {
                    Ok(()) => println!("已保存到 {path}"),
                    Err(error) => println!("保存失败: {error}"),
                },
                Err(error) => println!("序列化失败: {error}"),
            },
            Ok(Input::Load(path)) => match fs::read_to_string(&path) {
                Ok(json) => match serde_json::from_str::<Game>(&json) {
                    Ok(loaded) => {
                        game = loaded;
                        printed_events = game.log().len();
                        println!("已从 {path} 读取对局");
                        print_state(&game);
                    }
                    Err(error) => println!("解析失败: {error}"),
                },
                Err(error) => println!("读取失败: {error}"),
            },
            Ok(Input::Action(action)) => {
                let current = game.current_player();
                match game.apply(current, action) {
                    Ok(()) => {
                        while printed_events < game.log().len() {
                            println!("· {}", game.log()[printed_events].describe(&game));
                            printed_events += 1;
                        }
                        print_state(&game);
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
                    print_usage();
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

fn print_usage() {
    println!("用法: splendor-pokemon [--players 2|3|4] [--names 名字1,名字2,...] [--seed N]");
}

fn print_state(game: &Game) {
    println!(
        "\n=== 第 {} 回合 | 当前: {} | 阶段: {} ===",
        game.turn_number(),
        game.player_name(game.current_player()),
        game.phase()
    );
    println!("供应区: {}", game.supply());

    for category in Category::ALL {
        print!("{}: ", category.label_zh());
        for (index, slot) in game.board().slots(category).iter().enumerate() {
            match slot {
                Some(card_id) => {
                    let text = game
                        .card(*card_id)
                        .map(card_summary)
                        .unwrap_or_else(|| format!("#{card_id}"));
                    print!("[{index}] {text} | ");
                }
                None => print!("[{index}] — | "),
            }
        }
        println!();
    }

    println!("玩家:");
    for (index, player) in game.players().iter().enumerate() {
        let marker = if index == game.current_player() {
            "*"
        } else {
            " "
        };
        println!(
            "{marker} {index}. {} | {}分 | 球: {} | 减免: {} | 捕捉{} 保留{} 进化{}",
            player.name(),
            game.points_for(index),
            player.tokens(),
            game.bonuses_for(index),
            player.captured().len(),
            player.reserved().len(),
            player.evolved_away().len()
        );
    }

    let current = game.current_player();
    if let Some(player) = game.players().get(current) {
        if player.reserved().is_empty() {
            println!("你的保留牌: 无");
        } else {
            println!("你的保留牌（仅自己可见）:");
            for (index, card_id) in player.reserved().iter().enumerate() {
                if let Some(card) = game.card(*card_id) {
                    println!("  [{index}] {}", card_summary(card));
                }
            }
        }
    }

    if let Some(winners) = game.winners() {
        let names: Vec<&str> = winners.iter().map(|id| game.player_name(*id)).collect();
        let suffix = if names.len() > 1 { "（并列）" } else { "" };
        println!("对局结束，胜者：{}{}", names.join("、"), suffix);
    }
}

fn card_summary(card: &Card) -> String {
    let mut parts = vec![
        format!("{} {}分", card.name_zh, card.points),
        format!("减免{}{}", card.bonus_color.label_zh(), card.bonus_count),
    ];
    if !card.cost.is_zero() {
        parts.push(format!("费用 {}", card.cost));
    }
    if let (Some(target), Some(requirement)) = (&card.evolves_to_zh, &card.evolution_requirement) {
        parts.push(format!("→{target}({requirement})"));
    }
    parts.join(" ")
}

fn print_legal(game: &Game) {
    if game.phase() == Phase::Discard {
        println!("弃球阶段不会枚举组合，请使用：discard r 1 b 2 m 1（或 6 个数字）");
        return;
    }
    let actions = game.legal_actions();
    if actions.is_empty() {
        println!("当前没有可执行动作");
        return;
    }
    println!("合法动作（{} 个）:", actions.len());
    for action in actions {
        println!("  - {action}");
    }
}

enum Input {
    Action(Action),
    Show,
    Legal,
    Help,
    Save(String),
    Load(String),
    Quit,
}

fn parse_input(parts: &[&str]) -> Result<Input, String> {
    match parts[0].to_ascii_lowercase().as_str() {
        "help" | "h" | "?" => Ok(Input::Help),
        "show" | "s" => Ok(Input::Show),
        "legal" | "l" => Ok(Input::Legal),
        "quit" | "q" | "exit" => Ok(Input::Quit),
        "save" => Ok(Input::Save(
            parts.get(1).ok_or("save 需要文件路径")?.to_string(),
        )),
        "load" => Ok(Input::Load(
            parts.get(1).ok_or("load 需要文件路径")?.to_string(),
        )),
        "take3" | "t3" => {
            let colors = parts[1..]
                .iter()
                .map(|value| parse_color(value))
                .collect::<Result<Vec<_>, _>>()?;
            if colors.is_empty() {
                return Err("take3 至少需要一个颜色".into());
            }
            Ok(Input::Action(Action::TakeThree { colors }))
        }
        "take2" | "t2" => {
            let color = parse_color(parts.get(1).ok_or("take2 需要颜色")?)?;
            Ok(Input::Action(Action::TakeTwo { color }))
        }
        "reserve" | "r" => Ok(Input::Action(Action::Reserve {
            source: parse_source(&parts[1..], false, true)?,
        })),
        "capture" | "c" => Ok(Input::Action(Action::Capture {
            source: parse_source(&parts[1..], true, false)?,
        })),
        "discard" | "d" => Ok(Input::Action(Action::Discard {
            tokens: parse_discard(&parts[1..])?,
        })),
        "evolve" | "e" => {
            let captured_index = parse_index(parts.get(1).ok_or("evolve 需要已捕捉牌下标")?)?;
            let target = parse_source(&parts[2..], true, false)?;
            Ok(Input::Action(Action::Evolve {
                captured_index,
                target,
            }))
        }
        "end" | "eot" => Ok(Input::Action(Action::EndTurn)),
        other => Err(format!("未知命令 {other}")),
    }
}

fn print_help() {
    println!(
        "\
命令:
  show                     显示当前状态
  legal                    列出当前合法动作
  take3 r b y              拿 3 种不同普通球
  take2 r                  拿 2 个同色普通球（供应区至少 4 个）
  reserve board 1 0        保留场上 1 阶第 0 张
  reserve deck 2           保留 2 阶牌堆顶（牌面隐藏）
  capture board 2 1        捕捉场上 2 阶第 1 张
  capture reserved 0       捕捉自己的第 0 张保留牌
  discard r 1 b 2 m 1      弃球，也可写成 6 个数字（红 蓝 黄 粉 黑 大师）
  evolve 0 board 2 1       把第 0 张已捕捉牌进化为场上 2 阶第 1 张
  evolve 0 reserved 0      进化为自己的第 0 张保留牌
  end                      结束回合
  save <path>              保存对局 JSON
  load <path>              读取对局 JSON
  quit                     退出"
    );
}

fn parse_source(
    args: &[&str],
    allow_reserved: bool,
    allow_deck: bool,
) -> Result<CardSource, String> {
    let kind = args
        .first()
        .ok_or("缺少来源，可用 board / reserved / deck")?
        .to_ascii_lowercase();
    match kind.as_str() {
        "board" => {
            let category = parse_category(args.get(1).ok_or("board 需要类别（1/2/3）")?)?;
            let index = parse_index(args.get(2).ok_or("board 需要槽位下标")?)?;
            Ok(CardSource::board(category, index))
        }
        "reserved" | "mine" => {
            if !allow_reserved {
                return Err("这个动作不能使用保留牌作为来源".into());
            }
            let index = parse_index(args.get(1).ok_or("reserved 需要下标")?)?;
            Ok(CardSource::reserved(index))
        }
        "deck" | "decktop" => {
            if !allow_deck {
                return Err("这个动作不能使用牌堆顶作为来源".into());
            }
            let category = parse_category(args.get(1).ok_or("deck 需要类别（1/2/3）")?)?;
            Ok(CardSource::deck_top(category))
        }
        other => Err(format!("未知来源 {other:?}，可用 board / reserved / deck")),
    }
}

fn parse_color(value: &str) -> Result<Color, String> {
    Color::parse(value).ok_or_else(|| format!("未知颜色 {value:?}（r/b/y/p/k 或 红/蓝/黄/粉/黑）"))
}

fn parse_category(value: &str) -> Result<Category, String> {
    Category::parse(value).ok_or_else(|| format!("未知类别 {value:?}（可用 1/2/3/rare/legendary）"))
}

fn parse_index(value: &str) -> Result<usize, String> {
    value.parse().map_err(|_| format!("{value:?} 不是有效下标"))
}

fn parse_discard(args: &[&str]) -> Result<TokenCounts, String> {
    if args.is_empty() {
        return Err("discard 需要数量，例如 discard 1 0 0 0 0 0 或 discard r 1 m 1".into());
    }

    if args.len() == 6 {
        let mut values = [0u8; 6];
        let mut all_numbers = true;
        for (index, value) in args.iter().enumerate() {
            match value.parse::<u8>() {
                Ok(number) => values[index] = number,
                Err(_) => {
                    all_numbers = false;
                    break;
                }
            }
        }
        if all_numbers {
            return Ok(TokenCounts::new(
                values[0], values[1], values[2], values[3], values[4], values[5],
            ));
        }
    }

    let mut tokens = TokenCounts::ZERO;
    let mut index = 0;
    while index < args.len() {
        let arg = args[index];
        let (color_part, value_part) = if let Some(position) = arg.find(['=', ':']) {
            (&arg[..position], &arg[position + 1..])
        } else {
            let value = args
                .get(index + 1)
                .ok_or_else(|| format!("{arg} 缺少数量"))?;
            index += 1;
            (arg, *value)
        };

        let count: u8 = value_part
            .parse()
            .map_err(|_| format!("{value_part:?} 不是有效数量（0–255）"))?;
        let key = color_part.trim().to_ascii_lowercase();
        if matches!(key.as_str(), "m" | "master" | "大师" | "大师球") {
            tokens.master = tokens.master.checked_add(count).ok_or("大师球数量溢出")?;
        } else {
            let color = parse_color(color_part)?;
            let current = tokens.get(color);
            tokens.set(color, current.checked_add(count).ok_or("精灵球数量溢出")?);
        }
        index += 1;
    }

    Ok(tokens)
}
