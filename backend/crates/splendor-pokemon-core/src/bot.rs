//! 离线 bot：只通过 [`GameView`] 决策，不接触权威状态，也不修改任何规则。
//!
//! - [`RandomBot`]：在合法动作中均匀随机；弃球阶段由 bot 自行构造合法弃球。
//! - [`HeuristicBot`]：优先捕捉高分 / 可进化牌，其次拿需要的球或保留高价值牌。
//!
//! 所有随机数来自 [`BotRng`]，种子由对局 seed 与玩家 ID 派生，同一 seed 可完整复现。

use serde::{Deserialize, Serialize};

use crate::game::{Game, TOKEN_LIMIT, WIN_POINTS};
use crate::replay::{Replay, ReplayRecorder};
use crate::types::{Action, Card, CardSource, Color, Phase, PlayerId, TokenCounts};
use crate::view::GameView;

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
}

impl BotKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            BotKind::Random => "random",
            BotKind::Heuristic => "heuristic",
        }
    }

    pub const fn label_zh(self) -> &'static str {
        match self {
            BotKind::Random => "随机 bot",
            BotKind::Heuristic => "启发式 bot",
        }
    }

    pub fn parse(input: &str) -> Option<Self> {
        match input.trim().to_ascii_lowercase().as_str() {
            "random" | "rand" | "随机" => Some(BotKind::Random),
            "heuristic" | "smart" | "启发" | "启发式" => Some(BotKind::Heuristic),
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

/// 简单启发式：
/// 1. 有捕捉机会时，优先高分 / 可进化 / 能直接获胜的牌；
/// 2. 进化阶段优先进化，否则结束回合；
/// 3. 其余情况在「拿球」与「保留高价值牌」之间按需求打分。
pub struct HeuristicBot {
    rng: BotRng,
}

impl HeuristicBot {
    pub fn new(seed: u64) -> Self {
        Self {
            rng: BotRng::new(seed),
        }
    }

    fn choose_discard(&self, view: &GameView) -> Action {
        let me = &view.players[view.current_player];
        let needs = desired_needs(view);
        let mut order: Vec<usize> = (0..5).collect();
        // 先丢「持有量 - 目标需求」最大的颜色；大师球最后丢。
        order.sort_by_key(|&slot| {
            let color = Color::ALL[slot];
            let surplus = me.tokens.get(color) as i32 - needs[slot] as i32;
            std::cmp::Reverse(surplus)
        });
        build_discard(me.tokens, &order)
    }

    fn choose_evolution(&mut self, view: &GameView) -> Option<Action> {
        let candidates: Vec<(i64, Action)> = view
            .legal_actions
            .iter()
            .filter(|action| matches!(action, Action::Evolve { .. }))
            .map(|action| (evolve_score(view, action), action.clone()))
            .collect();
        if candidates.is_empty() {
            return Some(Action::EndTurn);
        }
        pick_best(&mut self.rng, candidates)
    }

    fn choose_main_action(&mut self, view: &GameView) -> Option<Action> {
        if view.legal_actions.is_empty() {
            return None;
        }

        let captures: Vec<(i64, Action)> = view
            .legal_actions
            .iter()
            .filter(|action| matches!(action, Action::Capture { .. }))
            .map(|action| (capture_score(view, action), action.clone()))
            .collect();
        if !captures.is_empty() {
            return pick_best(&mut self.rng, captures);
        }

        let mut options: Vec<(i64, Action)> = Vec::new();
        for action in &view.legal_actions {
            match action {
                Action::TakeThree { .. } | Action::TakeTwo { .. } => {
                    options.push((take_score(view, action), action.clone()));
                }
                Action::Reserve { source } => {
                    options.push((reserve_score(view, source), action.clone()));
                }
                _ => {}
            }
        }
        pick_best(&mut self.rng, options)
    }
}

impl Bot for HeuristicBot {
    fn kind(&self) -> BotKind {
        BotKind::Heuristic
    }

    fn choose_action(&mut self, view: &GameView) -> Option<Action> {
        match view.phase {
            Phase::MainAction => self.choose_main_action(view),
            Phase::Discard => Some(self.choose_discard(view)),
            Phase::Evolution => self.choose_evolution(view),
            Phase::GameOver => None,
        }
    }
}

/// 按种类创建 bot；相同 `(kind, player, seed)` 产生完全相同的随机流。
pub fn make_bot(kind: BotKind, player: PlayerId, game_seed: u64) -> Box<dyn Bot> {
    let seed = derive_seed(game_seed, player);
    match kind {
        BotKind::Random => Box::new(RandomBot::new(seed)),
        BotKind::Heuristic => Box::new(HeuristicBot::new(seed)),
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

/// 卡牌自身价值：分数、减免、进化潜力。
fn card_value(view: &GameView, card: &Card) -> i64 {
    let me = &view.players[view.current_player];
    let mut value = card.points as i64 * 10 + card.bonus_count as i64 * 2;
    if let (Some(_), Some(requirement)) = (&card.evolves_to_zh, &card.evolution_requirement) {
        value += 8;
        if me.bonuses.get(requirement.color) >= requirement.count {
            value += 16;
        } else {
            value += me.bonuses.get(requirement.color) as i64 * 2;
        }
    }
    value
}

fn capture_score(view: &GameView, action: &Action) -> i64 {
    let Action::Capture { source } = action else {
        return i64::MIN;
    };
    let Some(card) = card_for_source(view, source) else {
        return 0;
    };
    let me = &view.players[view.current_player];
    let mut score = card.points as i64 * 20 + card.bonus_count as i64 * 3;
    if let (Some(_), Some(requirement)) = (&card.evolves_to_zh, &card.evolution_requirement) {
        score += 10;
        if me.bonuses.get(requirement.color) >= requirement.count {
            score += 20;
        }
    }
    if me.points + card.points as u16 >= WIN_POINTS {
        score += 1_000;
    }
    // 同分时略偏向便宜、能立刻形成减免的牌。
    score - card.cost.total() as i64 / 2
}

fn evolve_score(view: &GameView, action: &Action) -> i64 {
    let Action::Evolve {
        captured_index,
        target,
    } = action
    else {
        return i64::MIN;
    };
    let me = &view.players[view.current_player];
    let Some(from) = me.captured.get(*captured_index) else {
        return 0;
    };
    let Some(to) = card_for_source(view, target) else {
        return 0;
    };
    let mut score = to.points as i64 * 20 - from.points as i64 * 6 + to.bonus_count as i64 * 3;
    if me.points.saturating_sub(from.points as u16) + to.points as u16 >= WIN_POINTS {
        score += 1_000;
    }
    score
}

fn reserve_score(view: &GameView, source: &CardSource) -> i64 {
    match card_for_source(view, source) {
        Some(card) => {
            let value = card_value(view, card);
            // 保留会占位置，但没有更好的行动时仍值得抢高价值牌。
            4 + value / 2
        }
        None => 3,
    }
}

fn take_score(view: &GameView, action: &Action) -> i64 {
    let me = &view.players[view.current_player];
    let needs = desired_needs(view);
    let (colors, count) = match action {
        Action::TakeThree { colors } => (
            colors
                .iter()
                .map(|color| color_slot(*color))
                .collect::<Vec<_>>(),
            3i64,
        ),
        Action::TakeTwo { color } => (vec![color_slot(*color)], 2i64),
        _ => return i64::MIN,
    };

    let mut score = 8 + count;
    for slot in colors {
        score += needs[slot] as i64 * 5;
    }
    let after = me.tokens.total() as i64 + count;
    if after > TOKEN_LIMIT as i64 {
        score -= 20;
    }
    score
}

/// 当前最值得追求的牌还缺哪些球（已扣除永久减免与手上已有的球）。
fn desired_needs(view: &GameView) -> [u8; 5] {
    let me = &view.players[view.current_player];
    let mut best: Option<&Card> = None;
    let mut best_value = i64::MIN;

    for category in crate::types::Category::ALL {
        for card in view.board.slots(category).iter().flatten() {
            let value = card_value(view, card);
            if value > best_value {
                best_value = value;
                best = Some(card);
            }
        }
    }
    if let Some(cards) = me.reserved.as_ref() {
        for card in cards {
            let value = card_value(view, card);
            if value > best_value {
                best_value = value;
                best = Some(card);
            }
        }
    }

    let mut needs = [0u8; 5];
    if let Some(card) = best {
        for (slot, color) in Color::ALL.iter().enumerate() {
            let due = card.cost.get(*color).saturating_sub(me.bonuses.get(*color));
            needs[slot] = due.saturating_sub(me.tokens.get(*color));
        }
    }
    needs
}

fn color_slot(color: Color) -> usize {
    match color {
        Color::Red => 0,
        Color::Blue => 1,
        Color::Yellow => 2,
        Color::Pink => 3,
        Color::Black => 4,
    }
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
/// 调用方保证顺序中每个槽位的持有量足够，且总弃球数恰好为 `held.total() - TOKEN_LIMIT`。
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

    // 理论上不会走到这里；若顺序未覆盖大师球则补上，保证动作合法。
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

    #[test]
    fn discard_has_exact_excess_and_never_exceeds_held() {
        let held = TokenCounts::new(3, 4, 2, 0, 5, 2);
        let order = [0, 1, 2, 3, 4, 5];
        let Action::Discard { tokens } = build_discard(held, &order) else {
            panic!("expected discard");
        };
        assert_eq!(
            held.total() - tokens.total(),
            TOKEN_LIMIT,
            "弃球后应正好等于上限"
        );
        for color in Color::ALL {
            assert!(tokens.get(color) <= held.get(color));
        }
        assert!(tokens.master <= held.master);
    }
}
