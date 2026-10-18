//! 离线 bot：只通过 [`GameView`] 决策，不接触权威状态，也不修改任何规则。
//!
//! - [`RandomBot`]：在合法动作中均匀随机；弃球阶段由 bot 自行构造合法弃球。
//! - [`HeuristicBot`]：以 v1 为底座，只在捕捉选择上加入进化推进与反制。
//! - [`HeuristicV1Bot`](crate::heuristic_v1::HeuristicV1Bot)：冻结的回测对照组。
//!
//! 所有随机数来自 [`BotRng`]，种子由对局 seed 与玩家 ID 派生，同一 seed 可完整复现。

use serde::{Deserialize, Serialize};

use crate::game::{Game, TOKEN_LIMIT, WIN_POINTS};
use crate::heuristic_v1::HeuristicV1Bot;
use crate::replay::{Replay, ReplayRecorder};
use crate::types::{Action, Card, CardSource, Phase, PlayerId, TokenCounts};
use crate::view::{GameView, PlayerView};

/// 单局 bot 对局的最大动作数。超过即视为状态机异常，避免测试或 CLI 无界运行。
pub const MAX_BOT_STEPS: usize = 50_000;

/// 可复现的 bot 随机源（SplitMix64）。
#[derive(Clone, Debug)]
pub struct BotRng {
    state: u64,
}

impl BotRng {
    pub fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    pub fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// `[0, upper)` 内的均匀下标；`upper == 0` 时返回 0。
    pub fn below(&mut self, upper: usize) -> usize {
        if upper <= 1 {
            0
        } else {
            (self.next_u64() % upper as u64) as usize
        }
    }

    pub fn shuffle<T>(&mut self, items: &mut [T]) {
        if items.len() < 2 {
            return;
        }
        for index in (1..items.len()).rev() {
            let swap = self.below(index + 1);
            items.swap(index, swap);
        }
    }
}

/// 由对局 seed 与玩家 ID 派生独立随机流，保证玩家之间互不干扰。
pub fn derive_seed(seed: u64, player: PlayerId) -> u64 {
    let mut z = seed ^ 0x9E37_79B9_7F4A_7C15u64.wrapping_mul(player as u64 + 1);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// bot 策略种类。序列化后可直接写入回放元数据。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BotKind {
    Random,
    Heuristic,
    HeuristicV1,
}

impl BotKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            BotKind::Random => "random",
            BotKind::Heuristic => "heuristic",
            BotKind::HeuristicV1 => "heuristic-v1",
        }
    }

    pub const fn label_zh(self) -> &'static str {
        match self {
            BotKind::Random => "随机 bot",
            BotKind::Heuristic => "启发式 bot",
            BotKind::HeuristicV1 => "启发式 v1（回测基线）",
        }
    }

    pub fn parse(input: &str) -> Option<Self> {
        match input.trim().to_ascii_lowercase().as_str() {
            "random" | "rand" | "随机" => Some(BotKind::Random),
            "heuristic" | "smart" | "启发" | "启发式" => Some(BotKind::Heuristic),
            "heuristic-v1" | "heuristic_v1" | "v1" | "baseline" | "基线" => {
                Some(BotKind::HeuristicV1)
            }
            _ => None,
        }
    }
}

impl std::fmt::Display for BotKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// bot 的唯一入口：给定自己视角的只读快照，返回一个动作。
///
/// 返回 `None` 表示当前没有可选动作（通常意味着权威状态异常），runner 会报错而不是 panic。
pub trait Bot {
    fn kind(&self) -> BotKind;

    fn choose_action(&mut self, view: &GameView) -> Option<Action>;
}

/// 在合法动作中均匀随机。
pub struct RandomBot {
    rng: BotRng,
}

impl RandomBot {
    pub fn new(seed: u64) -> Self {
        Self {
            rng: BotRng::new(seed),
        }
    }

    fn choose_discard(&mut self, view: &GameView) -> Action {
        let tokens = view.players[view.current_player].tokens;
        let mut slots: Vec<usize> = (0..6).collect();
        self.rng.shuffle(&mut slots);
        build_discard(tokens, &slots)
    }
}

impl Bot for RandomBot {
    fn kind(&self) -> BotKind {
        BotKind::Random
    }

    fn choose_action(&mut self, view: &GameView) -> Option<Action> {
        if view.phase == Phase::Discard {
            return Some(self.choose_discard(view));
        }
        if view.legal_actions.is_empty() {
            return None;
        }
        let index = self.rng.below(view.legal_actions.len());
        Some(view.legal_actions[index].clone())
    }
}

/// 默认启发式：以 v1 为基础，只改写「捕捉哪张牌」。
///
/// 拿球、保留、弃球与进化阶段完全沿用 v1；有捕捉机会时，优先抢能推进自己
/// 进化门槛、或能阻止对手进化的牌。进化是免费的，因此围绕进化做取舍的收益
/// 高于单纯按分数贪心。
pub struct HeuristicBot {
    base: HeuristicV1Bot,
    rng: BotRng,
}

impl HeuristicBot {
    pub fn new(seed: u64) -> Self {
        Self {
            base: HeuristicV1Bot::new(seed),
            rng: BotRng::new(seed),
        }
    }

    fn refine_main(&mut self, view: &GameView, base: Action) -> Action {
        let relevance =
            |card: &Card| evolution_progress(view, card) + opponent_evolution_pressure(view, card);
        let base_relevance = match &base {
            Action::Capture { source } => card_for_source(view, source).map(relevance).unwrap_or(0),
            _ => 0,
        };

        let options: Vec<(i64, Action)> = view
            .legal_actions
            .iter()
            .filter_map(|action| {
                let Action::Capture { source } = action else {
                    return None;
                };
                let card = card_for_source(view, source)?;
                if relevance(card) <= base_relevance {
                    return None;
                }
                Some((capture_priority(view, card), action.clone()))
            })
            .collect();

        pick_best(&mut self.rng, options).unwrap_or(base)
    }
}

impl Bot for HeuristicBot {
    fn kind(&self) -> BotKind {
        BotKind::Heuristic
    }

    fn choose_action(&mut self, view: &GameView) -> Option<Action> {
        match view.phase {
            Phase::MainAction => {
                let base = self.base.choose_action(view)?;
                Some(self.refine_main(view, base))
            }
            Phase::Discard | Phase::Evolution | Phase::GameOver => self.base.choose_action(view),
        }
    }
}

/// 捕捉这张牌会给进化门槛带来多少额外价值。
fn evolution_progress(view: &GameView, card: &Card) -> i64 {
    let me = &view.players[view.current_player];
    let current = me.bonuses.get(card.bonus_color);
    let mut bonus = 0;

    for source in &me.captured {
        let Some(requirement) = source.evolution_requirement.as_ref() else {
            continue;
        };
        if requirement.color != card.bonus_color || source.evolves_to_zh.is_none() {
            continue;
        }
        let remaining = requirement.count.saturating_sub(current);
        if remaining == 0 {
            continue;
        }
        if card.bonus_count >= remaining {
            // 这张牌直接补满门槛：目标在场时价值更高。
            bonus += if visible_evolution_target(view, source) {
                70
            } else {
                30
            };
        } else {
            bonus += 15 * card.bonus_count.min(remaining) as i64;
        }
    }

    if card.category.is_level()
        && card.evolves_to_zh.is_some()
        && visible_evolution_target(view, card)
    {
        bonus += 20;
    }
    bonus
}

/// 与 v1 相同的捕捉优先级，再加上进化推进与阻止对手进化的价值。
fn capture_priority(view: &GameView, card: &Card) -> i64 {
    let me = &view.players[view.current_player];
    let mut score = card.points as i64 * 20 + card.bonus_count as i64 * 3;
    if let (Some(_), Some(requirement)) = (&card.evolves_to_zh, &card.evolution_requirement) {
        score += 10;
        if me.bonuses.get(requirement.color) >= requirement.count {
            score += 20;
        }
    }
    if me.points.saturating_add(card.points as u16) >= WIN_POINTS {
        score += 1_000;
    }
    score - card.cost.total() as i64 / 2
        + evolution_progress(view, card)
        + opponent_evolution_pressure(view, card)
}

/// 抢走这张牌会让对手少掉多少进化机会。
fn opponent_evolution_pressure(view: &GameView, card: &Card) -> i64 {
    let me = &view.players[view.current_player];
    let mut pressure = 0;
    for opponent in &view.players {
        if opponent.id == me.id {
            continue;
        }
        for source in &opponent.captured {
            if !is_evolution_pair(source, card) {
                continue;
            }
            pressure += if requirement_met(opponent, source) {
                80
            } else {
                20
            };
        }
    }
    pressure
}

fn visible_evolution_target(view: &GameView, source: &Card) -> bool {
    let me = &view.players[view.current_player];
    let Some(name) = source.evolves_to_zh.as_deref() else {
        return false;
    };
    let Some(category) = source.category.next_level() else {
        return false;
    };
    view.board
        .slots(category)
        .iter()
        .flatten()
        .any(|card| card.name_zh == name)
        || me.reserved.as_ref().is_some_and(|cards| {
            cards
                .iter()
                .any(|card| card.category == category && card.name_zh == name)
        })
}

/// 按种类创建 bot；相同 `(kind, player, seed)` 产生完全相同的随机流。
pub fn make_bot(kind: BotKind, player: PlayerId, game_seed: u64) -> Box<dyn Bot> {
    let seed = derive_seed(game_seed, player);
    match kind {
        BotKind::Random => Box::new(RandomBot::new(seed)),
        BotKind::Heuristic => Box::new(HeuristicBot::new(seed)),
        BotKind::HeuristicV1 => Box::new(HeuristicV1Bot::new(seed)),
    }
}

/// 用 bot 跑完一局并记录逐动作回放。`kinds.len()` 必须与 `names.len()` 一致。
pub fn play_bot_game(
    names: Vec<String>,
    seed: u64,
    kinds: &[BotKind],
) -> Result<Replay, crate::error::GameError> {
    if names.len() != kinds.len() {
        return Err(crate::error::GameError::InvalidAction(format!(
            "玩家数 {} 与 bot 数 {} 不一致",
            names.len(),
            kinds.len()
        )));
    }

    let mut game = Game::new(names, seed)?;
    let mut bots: Vec<Box<dyn Bot>> = kinds
        .iter()
        .enumerate()
        .map(|(player, kind)| make_bot(*kind, player, seed))
        .collect();
    let controllers: Vec<String> = kinds.iter().map(|kind| kind.as_str().to_string()).collect();
    let mut recorder = ReplayRecorder::new(&game, controllers);

    let mut steps = 0usize;
    while !game.is_over() {
        if steps >= MAX_BOT_STEPS {
            return Err(crate::error::GameError::Internal(format!(
                "bot 对局超过 {MAX_BOT_STEPS} 个动作仍未结束"
            )));
        }
        let actor = game.current_player();
        let view = game.view_for(actor);
        let Some(action) = bots[actor].choose_action(&view) else {
            if game.phase() == Phase::MainAction && game.legal_actions().is_empty() {
                // 规则层僵局：所有普通球被持有、当前玩家无牌可捕捉。规则未定义 pass，
                // 因此按「无胜者僵局」收尾，而不是新增动作改变语义。
                return Ok(recorder.finish_stalemate(&game));
            }
            let tokens: Vec<String> = game
                .players()
                .iter()
                .map(|player| player.tokens().to_string())
                .collect();
            return Err(crate::error::GameError::Internal(format!(
                "玩家 {actor} 在阶段「{}」没有可用动作（供应区 {}；持有 {}）",
                game.phase(),
                game.supply(),
                tokens.join(" / ")
            )));
        };
        game.apply(actor, action.clone())?;
        recorder.record(&game, actor, &action);
        steps += 1;
    }

    Ok(recorder.finish(&game))
}

// ---------------------------------------------------------------------------
// 评分与动作构造
// ---------------------------------------------------------------------------

fn pick_best(rng: &mut BotRng, candidates: Vec<(i64, Action)>) -> Option<Action> {
    let best = candidates.iter().map(|(score, _)| *score).max()?;
    let tied: Vec<&Action> = candidates
        .iter()
        .filter(|(score, _)| *score == best)
        .map(|(_, action)| action)
        .collect();
    tied.get(rng.below(tied.len()))
        .map(|action| (*action).clone())
}

fn card_for_source<'a>(view: &'a GameView, source: &CardSource) -> Option<&'a Card> {
    let me = &view.players[view.current_player];
    match source {
        CardSource::Board { category, index } => view
            .board
            .slots(*category)
            .get(*index)
            .and_then(|card| card.as_ref()),
        CardSource::Reserved { index } => me.reserved.as_ref().and_then(|cards| cards.get(*index)),
        CardSource::DeckTop { .. } => None,
    }
}

fn is_evolution_pair(source: &Card, target: &Card) -> bool {
    source.category.next_level() == Some(target.category)
        && source.evolves_to_zh.as_deref() == Some(target.name_zh.as_str())
}

fn requirement_met(player: &PlayerView, source: &Card) -> bool {
    source
        .evolution_requirement
        .as_ref()
        .is_some_and(|requirement| player.bonuses.get(requirement.color) >= requirement.count)
}

fn token_at(tokens: TokenCounts, slot: usize) -> u8 {
    match slot {
        0 => tokens.red,
        1 => tokens.blue,
        2 => tokens.yellow,
        3 => tokens.pink,
        4 => tokens.black,
        _ => tokens.master,
    }
}

fn add_token(tokens: &mut TokenCounts, slot: usize, count: u8) {
    if count == 0 {
        return;
    }
    match slot {
        0 => tokens.red += count,
        1 => tokens.blue += count,
        2 => tokens.yellow += count,
        3 => tokens.pink += count,
        4 => tokens.black += count,
        _ => tokens.master += count,
    }
}

/// 按给定顺序从持有量中凑出「必须弃掉」的数量。
///
/// 调用方保证顺序覆盖全部持有槽位，且总弃球数恰好为 `held.total() - TOKEN_LIMIT`。
fn build_discard(held: TokenCounts, order: &[usize]) -> Action {
    let excess = held.total().saturating_sub(TOKEN_LIMIT);
    let mut discard = TokenCounts::ZERO;
    let mut remaining = excess;

    for &slot in order {
        if remaining == 0 {
            break;
        }
        let take = token_at(held, slot).min(remaining as u8);
        add_token(&mut discard, slot, take);
        remaining -= take as u16;
    }

    if remaining > 0 {
        add_token(&mut discard, 5, token_at(held, 5).min(remaining as u8));
    }

    Action::Discard { tokens: discard }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bot_rng_is_deterministic_and_seed_sensitive() {
        let mut first = BotRng::new(7);
        let mut second = BotRng::new(7);
        let a: Vec<u64> = (0..8).map(|_| first.next_u64()).collect();
        let b: Vec<u64> = (0..8).map(|_| second.next_u64()).collect();
        assert_eq!(a, b);

        let mut other = BotRng::new(8);
        assert_ne!(a[0], other.next_u64());
    }
}
