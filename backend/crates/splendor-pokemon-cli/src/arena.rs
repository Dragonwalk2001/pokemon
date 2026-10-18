//! `arena` 子命令：10 个固定种子 + 10 个增量种子的 bot 回测。
//!
//! 官方赛制与 `.clinerules/ai-backtest.md` 一致：固定种子 `1..=10` 永不变，
//! 增量种子每次取 10 个从未用过的种子；每个种子交换先后手各一局，共 40 局。
//! 增量游标保存在 `backend/arena-seeds.txt`，官方回测完成后自动前进 10。

use std::fs;
use std::path::{Component, Path, PathBuf};

use splendor_pokemon_core::{play_bot_game, BotKind, ReplayResult};

const FIXED_SEED_START: u64 = 1;
const FIXED_SEED_COUNT: u64 = 10;
const INCREMENTAL_SEED_COUNT: u64 = 10;
const TOTAL_SEED_COUNT: u64 = FIXED_SEED_COUNT + INCREMENTAL_SEED_COUNT;
const LEDGER_FILE: &str = "arena-seeds.txt";

#[derive(Default)]
struct Tally {
    wins: u32,
    losses: u32,
    draws: u32,
}

impl Tally {
    fn record(&mut self, candidate_won: bool, baseline_won: bool) {
        if candidate_won && baseline_won {
            self.draws += 1;
        } else if candidate_won {
            self.wins += 1;
        } else if baseline_won {
            self.losses += 1;
        } else {
            self.draws += 1;
        }
    }

    fn record_stalemate(&mut self) {
        self.draws += 1;
    }

    fn score_rate(&self, games: u64) -> f64 {
        (self.wins as f64 + 0.5 * self.draws as f64) / games as f64 * 100.0
    }

    fn win_rate(&self) -> f64 {
        let decisive = self.wins + self.losses;
        if decisive == 0 {
            0.0
        } else {
            self.wins as f64 / decisive as f64 * 100.0
        }
    }
}

#[derive(Clone, Copy)]
enum SeedPlan {
    /// 10 个固定种子 + 从游标开始的 10 个增量种子；完成后推进游标。
    Official { incremental_start: u64 },
    /// 任意种子区间，仅用于调试，不读写游标。
    Debug { start: u64, count: u64 },
}

impl SeedPlan {
    fn seeds(&self) -> Vec<u64> {
        match *self {
            SeedPlan::Official { incremental_start } => (FIXED_SEED_START
                ..FIXED_SEED_START + FIXED_SEED_COUNT)
                .chain(incremental_start..incremental_start + INCREMENTAL_SEED_COUNT)
                .collect(),
            SeedPlan::Debug { start, count } => (start..start + count).collect(),
        }
    }

    fn fixed_count(&self) -> usize {
        match self {
            SeedPlan::Official { .. } => FIXED_SEED_COUNT as usize,
            SeedPlan::Debug { .. } => 0,
        }
    }

    fn is_official(&self) -> bool {
        matches!(self, SeedPlan::Official { .. })
    }

    fn label(&self) -> String {
        match *self {
            SeedPlan::Official { incremental_start } => format!(
                "{FIXED_SEED_COUNT} 个固定种子（{FIXED_SEED_START}–{}）+ {INCREMENTAL_SEED_COUNT} 个增量种子（{incremental_start}–{}）",
                FIXED_SEED_START + FIXED_SEED_COUNT - 1,
                incremental_start + INCREMENTAL_SEED_COUNT - 1
            ),
            SeedPlan::Debug { start, count } => format!(
                "自定义调试种子（{start}–{}，不消耗增量游标）",
                start + count - 1
            ),
        }
    }
}

pub(crate) fn run(args: &[String]) -> Result<(), String> {
    let Some(config) = Config::from_args(args)? else {
        return Ok(());
    };

    let seeds = config.plan.seeds();
    let games = seeds.len() as u64 * 2;
    let fixed_count = config.plan.fixed_count();

    println!(
        "AI 回测：candidate={}（{}） vs baseline={}（{}）",
        config.candidate,
        config.candidate.label_zh(),
        config.baseline,
        config.baseline.label_zh()
    );
    println!(
        "赛制：{} × 先后手互换 = {} 局 / 2 人",
        config.plan.label(),
        games
    );

    let mut total = Tally::default();
    let mut fixed = Tally::default();
    let mut incremental = Tally::default();
    let mut first = Tally::default();
    let mut second = Tally::default();
    let mut stalemates = 0u32;
    let mut candidate_points = 0u64;
    let mut baseline_points = 0u64;
    let mut max_turns = 0u32;

    for (seed_index, seed) in seeds.iter().enumerate() {
        let is_fixed = seed_index < fixed_count;
        let seed = *seed;
        for candidate_first in [true, false] {
            let (names, kinds, candidate_seat) = if candidate_first {
                (
                    vec!["Candidate".to_string(), "Baseline".to_string()],
                    vec![config.candidate, config.baseline],
                    0usize,
                )
            } else {
                (
                    vec!["Baseline".to_string(), "Candidate".to_string()],
                    vec![config.baseline, config.candidate],
                    1usize,
                )
            };
            let baseline_seat = 1 - candidate_seat;

            let replay = play_bot_game(names, seed, &kinds)
                .map_err(|error| format!("seed {seed} 对局失败: {error}"))?;
            let order = if candidate_first { "先手" } else { "后手" };
            let tag = if !config.plan.is_official() {
                "调试"
            } else if is_fixed {
                "固定"
            } else {
                "增量"
            };

            if replay.stalemate {
                stalemates += 1;
                total.record_stalemate();
                if is_fixed {
                    fixed.record_stalemate();
                } else if config.plan.is_official() {
                    incremental.record_stalemate();
                }
                if config.verbose {
                    println!("seed {seed}（{tag}）{order}：僵局");
                }
                continue;
            }

            let result = replay
                .result
                .as_ref()
                .ok_or_else(|| format!("seed {seed} {order} 既未结束也未僵局"))?;
            let candidate_won = result.winners.contains(&candidate_seat);
            let baseline_won = result.winners.contains(&baseline_seat);
            total.record(candidate_won, baseline_won);
            if is_fixed {
                fixed.record(candidate_won, baseline_won);
            } else if config.plan.is_official() {
                incremental.record(candidate_won, baseline_won);
            }
            if candidate_first {
                first.record(candidate_won, baseline_won);
            } else {
                second.record(candidate_won, baseline_won);
            }

            let candidate_score = points(result, candidate_seat);
            let baseline_score = points(result, baseline_seat);
            candidate_points += candidate_score as u64;
            baseline_points += baseline_score as u64;
            max_turns = max_turns.max(result.turns);

            if config.verbose {
                let outcome = if candidate_won && baseline_won {
                    "平"
                } else if candidate_won {
                    "胜"
                } else if baseline_won {
                    "负"
                } else {
                    "平"
                };
                println!(
                    "seed {seed}（{tag}）{order}：候选 {candidate_score}:{baseline_score} 基线 → {outcome}（{} 回合）",
                    result.turns
                );
            }
        }
    }

    let score_rate = total.score_rate(games);
    println!(
        "候选：{} 胜 {} 负 {} 平 | 得分率 {:.1}% | 不含平局胜率 {:.1}%",
        total.wins,
        total.losses,
        total.draws,
        score_rate,
        total.win_rate()
    );
    if config.plan.is_official() {
        println!(
            "固定种子：{} 胜 {} 负 {} 平 | 增量种子：{} 胜 {} 负 {} 平",
            fixed.wins,
            fixed.losses,
            fixed.draws,
            incremental.wins,
            incremental.losses,
            incremental.draws
        );
    }
    println!(
        "平均分：候选 {:.2} / 基线 {:.2}",
        candidate_points as f64 / games as f64,
        baseline_points as f64 / games as f64
    );
    println!(
        "先手：{} 胜 {} 负 {} 平 | 后手：{} 胜 {} 负 {} 平",
        first.wins, first.losses, first.draws, second.wins, second.losses, second.draws
    );
    println!("僵局：{stalemates} | 最长对局：{max_turns} 回合");

    if let SeedPlan::Official { incremental_start } = config.plan {
        let next = incremental_start + INCREMENTAL_SEED_COUNT;
        write_ledger(&config.ledger, next)?;
        println!(
            "增量种子 {incremental_start}–{} 已登记；下一轮从 {next} 开始",
            incremental_start + INCREMENTAL_SEED_COUNT - 1
        );
    }

    if let Some(require) = config.require {
        if stalemates > 0 {
            return Err(format!("回测未达标：出现 {stalemates} 局僵局"));
        }
        if score_rate < require {
            return Err(format!(
                "回测未达标：得分率 {score_rate:.1}% < 门槛 {require:.1}%"
            ));
        }
        println!("判定：通过（得分率 {score_rate:.1}% ≥ {require:.1}%）");
    } else {
        println!("判定：未设门槛（加 --require 50 启用准入检查）");
    }
    Ok(())
}

fn points(result: &ReplayResult, player: usize) -> u16 {
    result.points.get(player).copied().unwrap_or(0)
}

fn default_ledger_path() -> PathBuf {
    normalize(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../..")
            .join(LEDGER_FILE),
    )
}

fn normalize(path: PathBuf) -> PathBuf {
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

fn read_ledger(path: &Path) -> Result<u64, String> {
    let raw = fs::read_to_string(path).map_err(|error| {
        format!(
            "读取增量种子游标 {} 失败: {error}。该文件必须随仓库提交，禁止重置或回退。",
            path.display()
        )
    })?;
    raw.trim()
        .parse()
        .map_err(|_| format!("{} 必须只包含一个整数", path.display()))
}

fn write_ledger(path: &Path, next: u64) -> Result<(), String> {
    fs::write(path, format!("{next}\n"))
        .map_err(|error| format!("写入增量种子游标 {} 失败: {error}", path.display()))
}

struct Config {
    candidate: BotKind,
    baseline: BotKind,
    plan: SeedPlan,
    ledger: PathBuf,
    require: Option<f64>,
    verbose: bool,
}

impl Config {
    fn from_args(args: &[String]) -> Result<Option<Self>, String> {
        let mut candidate = BotKind::Minimax;
        let mut baseline = BotKind::HeuristicV1;
        let mut seeds: Option<u64> = None;
        let mut seed_start: Option<u64> = None;
        let mut ledger: Option<PathBuf> = None;
        let mut require: Option<f64> = None;
        let mut verbose = false;

        let mut index = 0;
        while index < args.len() {
            match args[index].as_str() {
                "--help" | "-h" => {
                    crate::ui::print_usage();
                    return Ok(None);
                }
                "--candidate" => {
                    index += 1;
                    let value = args.get(index).ok_or("--candidate 缺少数值")?;
                    candidate = BotKind::parse(value).ok_or_else(|| {
                        format!("未知候选 bot {value:?}，可用 heuristic / minimax / heuristic-v1 / random")
                    })?;
                }
                "--baseline" => {
                    index += 1;
                    let value = args.get(index).ok_or("--baseline 缺少数值")?;
                    baseline = BotKind::parse(value).ok_or_else(|| {
                        format!("未知基线 bot {value:?}，可用 heuristic / minimax / heuristic-v1 / random")
                    })?;
                }
                "--seeds" => {
                    index += 1;
                    seeds = Some(
                        args.get(index)
                            .ok_or("--seeds 缺少数值")?
                            .parse()
                            .map_err(|_| "--seeds 必须是正整数".to_string())?,
                    );
                }
                "--seed-start" => {
                    index += 1;
                    seed_start = Some(
                        args.get(index)
                            .ok_or("--seed-start 缺少数值")?
                            .parse()
                            .map_err(|_| "--seed-start 必须是非负整数".to_string())?,
                    );
                }
                "--ledger" => {
                    index += 1;
                    ledger = Some(PathBuf::from(args.get(index).ok_or("--ledger 缺少路径")?));
                }
                "--require" => {
                    index += 1;
                    require = Some(
                        args.get(index)
                            .ok_or("--require 缺少数值")?
                            .parse()
                            .map_err(|_| "--require 必须是 0–100 的百分数".to_string())?,
                    );
                }
                "--verbose" | "-v" => verbose = true,
                other => return Err(format!("未知参数 {other}")),
            }
            index += 1;
        }

        if let Some(value) = require {
            if !(0.0..=100.0).contains(&value) {
                return Err(format!("--require 必须在 0–100 之间，当前为 {value}"));
            }
        }

        let (plan, ledger) = if seeds.is_some() || seed_start.is_some() {
            if ledger.is_some() {
                return Err(
                    "--ledger 只用于官方模式；调试模式请去掉 --ledger，或去掉 --seeds/--seed-start"
                        .to_string(),
                );
            }
            let count = seeds.unwrap_or(TOTAL_SEED_COUNT);
            if count == 0 {
                return Err("--seeds 必须大于 0".to_string());
            }
            (
                SeedPlan::Debug {
                    start: seed_start.unwrap_or(FIXED_SEED_START),
                    count,
                },
                default_ledger_path(),
            )
        } else {
            let ledger = ledger.unwrap_or_else(default_ledger_path);
            let incremental_start = read_ledger(&ledger)?;
            let fixed_end = FIXED_SEED_START + FIXED_SEED_COUNT - 1;
            if incremental_start <= fixed_end {
                return Err(format!(
                    "增量游标必须大于固定种子末尾 {fixed_end}，当前为 {incremental_start}；禁止重置或回退游标"
                ));
            }
            (SeedPlan::Official { incremental_start }, ledger)
        };

        Ok(Some(Self {
            candidate,
            baseline,
            plan,
            ledger,
            require,
            verbose,
        }))
    }
}
