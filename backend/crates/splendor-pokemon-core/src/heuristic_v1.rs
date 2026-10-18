//! 冻结的启发式 v1：只作为 40 局回测的固定对照组。
//!
//! 这里刻意保留旧版决策逻辑，不跟随默认 `HeuristicBot` 的调整；否则历史回测结果
//! 会失去可比性。除修复编译错误外，不要改动本模块的评分。

use crate::bot::{Bot, BotKind, BotRng};
use crate::game::{TOKEN_LIMIT, WIN_POINTS};
use crate::types::{Action, Card, CardSource, Color, Phase, TokenCounts};
use crate::view::GameView;

/// v1 版启发式：单目标贪心。
pub struct HeuristicV1Bot {
    rng: BotRng,
}

impl HeuristicV1Bot {
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

impl Bot for HeuristicV1Bot {
    fn kind(&self) -> BotKind {
        BotKind::HeuristicV1
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
