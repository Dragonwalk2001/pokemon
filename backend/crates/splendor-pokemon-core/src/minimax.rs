//! 极小化极大（minimax + alpha-beta 剪枝）搜索 bot。
//!
//! 信息边界与其它 bot 一致：搜索状态完全由 [`GameView`] 构造，只包含公开信息
//! 与自己的全部保留牌；对手从场上保留的牌牌面公开，从牌堆顶保留的牌只记录
//! 数量。牌堆顶未知，因此模拟中不补牌。
//! 搜索不使用墙钟时间，固定深度与固定分支宽度保证同一 `(seed, 局面)` 逐字节可复现。
//!
//! 搜索以一个完整回合为一步：主要行动 → 自动弃球 → 自动进化 → 换手。
//! 只在出现捕捉机会时启用搜索；拿球、保留、弃球与进化沿用启发式底座。
//! 根节点只评估与基准动作排序相近的捕捉，深层节点按启发式排序取前若干分支。
//! 规则层僵局（供应区无普通球且无牌可捕捉）在评估中按平局处理，避免搜索
//! 误把「让对手无行动」当成优势。

use std::rc::Rc;

use crate::bot::{Bot, BotKind, BotRng, HeuristicBot};
use crate::game::{MAX_RESERVED, TOKEN_LIMIT, WIN_POINTS};
use crate::types::{Action, Card, CardSource, Category, Color, Phase, PlayerId, TokenCounts};
use crate::view::GameView;

/// 根节点最多保留的捕捉动作数。捕捉直接改变分数与进化资格，值得全部评估。
const ROOT_CAPTURE_WIDTH: usize = 10;
/// 根节点候选相对基准动作的最小排序分差；差太多说明启发式已明确不看好。
const ROOT_PRIORITY_MARGIN: i64 = 40;
/// 搜索的回合层数：根行动之后展开 5 个回合。
const LOOKAHEAD_TURNS: usize = 5;
/// 终局分数必须远大于任何局面评估。
const TERMINAL_SCORE: i64 = 1_000_000;

/// 搜索用玩家状态。只保存公开信息与自己的保留牌。
#[derive(Clone, Debug)]
struct SearchPlayer {
    tokens: TokenCounts,
    bonuses: TokenCounts,
    captured: Vec<Rc<Card>>,
    reserved: Vec<Rc<Card>>,
    /// 对手从牌堆顶保留的牌数量；牌面未知，仅用于估值。
    hidden_reserved: usize,
    evolved_count: usize,
    turns_taken: u32,
}

impl SearchPlayer {
    fn points(&self) -> u16 {
        self.captured.iter().map(|card| card.points as u16).sum()
    }

    fn bonus_total(&self) -> i64 {
        self.captured
            .iter()
            .map(|card| card.bonus_count as i64)
            .sum()
    }
}

/// 由 `GameView` 重建的模拟状态。所有动作都不会修改权威对局。
#[derive(Clone, Debug)]
struct SearchState {
    players: Vec<SearchPlayer>,
    supply: TokenCounts,
    board: [Vec<Option<Rc<Card>>>; 5],
    current: PlayerId,
    end_triggered: bool,
    over: bool,
    winners: Option<Vec<PlayerId>>,
}

impl SearchState {
    /// 从查看者视角构造模拟状态。
    ///
    /// 对手从场上保留的牌在视图里可见，直接进入模拟；从牌堆顶保留的牌
    /// 只有数量，不伪造牌面。牌堆顺序不可见且不参与搜索。
    fn from_view(view: &GameView) -> Self {
        let players = view
            .players
            .iter()
            .map(|player| {
                let captured: Vec<Rc<Card>> =
                    player.captured.iter().cloned().map(Rc::new).collect();
                let reserved: Vec<Rc<Card>> = player
                    .reserved
                    .as_ref()
                    .map(|cards| cards.iter().cloned().map(Rc::new).collect())
                    .unwrap_or_default();
                let bonuses = bonus_sum(&captured);
                SearchPlayer {
                    tokens: player.tokens,
                    bonuses,
                    captured,
                    hidden_reserved: player.hidden_reserved_count,
                    reserved,
                    evolved_count: player.evolution_count,
                    turns_taken: player.turns_taken,
                }
            })
            .collect();

        let board = [
            rc_slots(&view.board.level1),
            rc_slots(&view.board.level2),
            rc_slots(&view.board.level3),
            rc_slots(&view.board.rare),
            rc_slots(&view.board.legendary),
        ];

        Self {
            players,
            supply: view.supply,
            board,
            current: view.current_player,
            end_triggered: view.end_triggered,
            over: false,
            winners: None,
        }
    }

    fn player_count(&self) -> usize {
        self.players.len()
    }

    fn points(&self, player: PlayerId) -> u16 {
        self.players[player].points()
    }

    fn bonus_sum(&self, player: PlayerId) -> TokenCounts {
        bonus_sum(&self.players[player].captured)
    }

    fn card_for_source(&self, player: PlayerId, source: &CardSource) -> Option<Rc<Card>> {
        match source {
            CardSource::Board { category, index } => self.board[category_index(*category)]
                .get(*index)
                .cloned()
                .flatten(),
            CardSource::Reserved { index } => self.players[player].reserved.get(*index).cloned(),
            CardSource::DeckTop { .. } => None,
        }
    }

    /// 与规则引擎一致的支付计算：彩色球优先，差额用大师球补，最后加印刷大师球费用。
    fn payment(&self, player: PlayerId, card: &Card) -> Option<TokenCounts> {
        let me = &self.players[player];
        let mut payment = TokenCounts::ZERO;
        let mut master_needed = card.cost.master as u16;

        for color in Color::ALL {
            let due = card.cost.get(color).saturating_sub(me.bonuses.get(color));
            let paid = due.min(me.tokens.get(color));
            payment.set(color, paid);
            master_needed += (due - paid) as u16;
        }

        if master_needed > me.tokens.master as u16 {
            return None;
        }
        payment.master = master_needed as u8;
        Some(payment)
    }

    fn can_afford(&self, player: PlayerId, card: &Card) -> bool {
        self.payment(player, card).is_some()
    }

    fn has_affordable_capture(&self, player: PlayerId) -> bool {
        if self.players[player]
            .reserved
            .iter()
            .any(|card| self.can_afford(player, card))
        {
            return true;
        }
        Category::ALL.iter().any(|category| {
            self.board[category_index(*category)]
                .iter()
                .flatten()
                .any(|card| self.can_afford(player, card))
        })
    }

    /// 当前玩家在主要行动阶段的合法动作。
    ///
    /// 不生成「从牌堆顶保留」：牌面未知，无法在模拟中评估；根节点也不会选择它。
    fn legal_main_actions(&self) -> Vec<Action> {
        self.legal_main_actions_for(self.current)
    }

    /// 指定玩家在主要行动阶段的合法动作（用于叶子评估）。
    fn legal_main_actions_for(&self, player: PlayerId) -> Vec<Action> {
        let mut actions = Vec::new();

        let available: Vec<Color> = Color::ALL
            .iter()
            .copied()
            .filter(|color| self.supply.get(*color) > 0)
            .collect();
        if available.len() >= 3 {
            for first in 0..available.len() {
                for second in (first + 1)..available.len() {
                    for third in (second + 1)..available.len() {
                        actions.push(Action::TakeThree {
                            colors: vec![available[first], available[second], available[third]],
                        });
                    }
                }
            }
        } else if !available.is_empty() {
            actions.push(Action::TakeThree {
                colors: available.clone(),
            });
        }

        for color in Color::ALL {
            if self.supply.get(color) >= 4 {
                actions.push(Action::TakeTwo { color });
            }
        }

        if self.players[player].reserved.len() < MAX_RESERVED {
            for category in Category::LEVELS {
                for (index, slot) in self.board[category_index(category)].iter().enumerate() {
                    if slot.is_some() {
                        actions.push(Action::Reserve {
                            source: CardSource::board(category, index),
                        });
                    }
                }
            }
        }

        for category in Category::ALL {
            for (index, slot) in self.board[category_index(category)].iter().enumerate() {
                if let Some(card) = slot {
                    if self.can_afford(player, card) {
                        actions.push(Action::Capture {
                            source: CardSource::board(category, index),
                        });
                    }
                }
            }
        }
        for (index, card) in self.players[player].reserved.iter().enumerate() {
            if self.can_afford(player, card) {
                actions.push(Action::Capture {
                    source: CardSource::reserved(index),
                });
            }
        }

        actions
    }

    fn take_three(&mut self, player: PlayerId, colors: &[Color]) -> Option<()> {
        let available: Vec<Color> = Color::ALL
            .iter()
            .copied()
            .filter(|color| self.supply.get(*color) > 0)
            .collect();
        if colors.is_empty() || colors.iter().any(|color| !available.contains(color)) {
            return None;
        }
        let mut seen = [false; 5];
        for color in colors {
            let slot = color_slot(*color);
            if seen[slot] {
                return None;
            }
            seen[slot] = true;
        }
        if available.len() >= 3 {
            if colors.len() != 3 {
                return None;
            }
        } else if colors.len() != available.len() {
            return None;
        }

        for color in colors {
            self.supply
                .set(*color, self.supply.get(*color).checked_sub(1)?);
            let held = self.players[player].tokens.get(*color).checked_add(1)?;
            self.players[player].tokens.set(*color, held);
        }
        Some(())
    }

    fn take_two(&mut self, player: PlayerId, color: Color) -> Option<()> {
        let supply = self.supply.get(color);
        if supply < 4 {
            return None;
        }
        self.supply.set(color, supply - 2);
        let held = self.players[player].tokens.get(color).checked_add(2)?;
        self.players[player].tokens.set(color, held);
        Some(())
    }

    fn reserve(&mut self, player: PlayerId, category: Category, index: usize) -> Option<()> {
        if !category.is_level() || self.players[player].reserved.len() >= MAX_RESERVED {
            return None;
        }
        let card = self.board[category_index(category)]
            .get_mut(index)?
            .take()?;
        self.players[player].reserved.push(card);

        if self.supply.master > 0 {
            self.supply.master -= 1;
            let held = self.players[player].tokens.master.checked_add(1)?;
            self.players[player].tokens.master = held;
        }
        Some(())
    }

    fn capture(&mut self, player: PlayerId, source: &CardSource) -> Option<()> {
        let card = self.card_for_source(player, source)?;
        let payment = self.payment(player, &card)?;

        let mut tokens = self.players[player].tokens;
        tokens.checked_sub(payment)?;
        self.players[player].tokens = tokens;
        self.supply.checked_add(payment)?;

        match source {
            CardSource::Board { category, index } => {
                *self.board[category_index(*category)].get_mut(*index)? = None;
            }
            CardSource::Reserved { index } => {
                if *index >= self.players[player].reserved.len() {
                    return None;
                }
                self.players[player].reserved.remove(*index);
            }
            CardSource::DeckTop { .. } => return None,
        }

        self.players[player].captured.push(card);
        self.players[player].bonuses = self.bonus_sum(player);
        Some(())
    }

    fn apply_main_action(&mut self, action: &Action) -> Option<()> {
        let player = self.current;
        match action {
            Action::TakeThree { colors } => self.take_three(player, colors),
            Action::TakeTwo { color } => self.take_two(player, *color),
            Action::Reserve {
                source: CardSource::Board { category, index },
            } => self.reserve(player, *category, *index),
            Action::Reserve { .. } => None,
            Action::Capture { source } => self.capture(player, source),
            _ => None,
        }
    }

    /// 找到给定进化源牌的可见目标（场上或自己的保留牌）。
    fn find_evolution_target(&self, player: PlayerId, source: &Card) -> Option<CardSource> {
        let next = source.category.next_level()?;
        let name = source.evolves_to_zh.as_deref()?;
        for (index, slot) in self.board[category_index(next)].iter().enumerate() {
            if let Some(card) = slot {
                if card.name_zh == name {
                    return Some(CardSource::board(next, index));
                }
            }
        }
        for (index, card) in self.players[player].reserved.iter().enumerate() {
            if card.category == next && card.name_zh == name {
                return Some(CardSource::reserved(index));
            }
        }
        None
    }

    /// 当前玩家本回合最值得执行的一次进化。进化不付费用，满足条件即执行。
    fn best_evolution(&self, player: PlayerId) -> Option<(usize, CardSource)> {
        let me = &self.players[player];
        let mut best: Option<(i64, usize, CardSource)> = None;

        for (captured_index, source) in me.captured.iter().enumerate() {
            let (Some(requirement), Some(_)) =
                (&source.evolution_requirement, &source.evolves_to_zh)
            else {
                continue;
            };
            if me.bonuses.get(requirement.color) < requirement.count {
                continue;
            }
            let Some(target_source) = self.find_evolution_target(player, source) else {
                continue;
            };
            let Some(target) = self.card_for_source(player, &target_source) else {
                continue;
            };
            let score = (target.points as i64 - source.points as i64) * 120
                + (target.bonus_count as i64 - source.bonus_count as i64) * 18;
            if best
                .as_ref()
                .map_or(true, |(current, _, _)| score > *current)
            {
                best = Some((score, captured_index, target_source));
            }
        }

        best.map(|(_, captured_index, target)| (captured_index, target))
    }

    fn apply_evolution(
        &mut self,
        player: PlayerId,
        captured_index: usize,
        target: &CardSource,
    ) -> Option<()> {
        if captured_index >= self.players[player].captured.len() {
            return None;
        }
        let target_card = match target {
            CardSource::Board { category, index } => self.board[category_index(*category)]
                .get_mut(*index)?
                .take()?,
            CardSource::Reserved { index } => {
                if *index >= self.players[player].reserved.len() {
                    return None;
                }
                self.players[player].reserved.remove(*index)
            }
            CardSource::DeckTop { .. } => return None,
        };

        self.players[player].captured.remove(captured_index);
        self.players[player].captured.push(target_card);
        self.players[player].evolved_count += 1;
        self.players[player].bonuses = self.bonus_sum(player);
        Some(())
    }

    /// 弃球策略：优先丢掉「超出目标需求」最多的颜色，大师球最后丢。
    fn auto_discard(&mut self) -> Option<()> {
        let player = self.current;
        let held = self.players[player].tokens;
        let excess = held.total().saturating_sub(TOKEN_LIMIT);
        if excess == 0 {
            return Some(());
        }

        let needs = self.desired_needs(player);
        let mut order: Vec<usize> = (0..5).collect();
        order.sort_by_key(|&slot| {
            let surplus = token_at(held, slot) as i32 - needs[slot] as i32;
            std::cmp::Reverse(surplus)
        });
        order.push(5);

        let mut discard = TokenCounts::ZERO;
        let mut remaining = excess as i32;
        for slot in order {
            if remaining == 0 {
                break;
            }
            let take = token_at(held, slot).min(remaining as u8);
            set_token_at(&mut discard, slot, take);
            remaining -= take as i32;
        }
        if remaining > 0 {
            return None;
        }

        let mut tokens = held;
        tokens.checked_sub(discard)?;
        self.players[player].tokens = tokens;
        self.supply.checked_add(discard)?;
        Some(())
    }

    /// 执行当前玩家的完整回合；动作非法时返回 `None`。
    fn play_turn(&self, action: &Action) -> Option<Self> {
        let mut next = self.clone();
        next.apply_main_action(action)?;
        if next.players[next.current].tokens.total() > TOKEN_LIMIT {
            next.auto_discard()?;
        }
        if let Some((captured_index, target)) = next.best_evolution(next.current) {
            next.apply_evolution(next.current, captured_index, &target)?;
        }
        next.end_turn();
        Some(next)
    }

    fn end_turn(&mut self) {
        let player = self.current;
        self.players[player].turns_taken = self.players[player].turns_taken.saturating_add(1);

        if !self.end_triggered
            && self
                .players
                .iter()
                .any(|player| player.points() >= WIN_POINTS)
        {
            self.end_triggered = true;
        }

        if self.end_triggered {
            let target_turns = self.players[0].turns_taken;
            if self
                .players
                .iter()
                .all(|player| player.turns_taken == target_turns)
            {
                self.finish();
                return;
            }
        }

        self.current = (self.current + 1) % self.player_count();
    }

    fn finish(&mut self) {
        let points: Vec<u16> = self.players.iter().map(SearchPlayer::points).collect();
        let max_points = points.iter().copied().max().unwrap_or(0);
        let mut winners: Vec<PlayerId> = (0..self.player_count())
            .filter(|&player| points[player] == max_points)
            .collect();

        if winners.len() > 1 {
            let max_evolutions = winners
                .iter()
                .map(|&player| self.players[player].evolved_count)
                .max()
                .unwrap_or(0);
            winners.retain(|&player| self.players[player].evolved_count == max_evolutions);
        }
        if winners.len() > 1 {
            let max_cards = winners
                .iter()
                .map(|&player| self.players[player].captured.len())
                .max()
                .unwrap_or(0);
            winners.retain(|&player| self.players[player].captured.len() == max_cards);
        }

        self.winners = Some(winners);
        self.over = true;
    }

    /// 距离最近一张目标牌还缺多少球（大师球可补任意颜色）。
    fn capture_gap(&self, player: PlayerId, card: &Card) -> i64 {
        let me = &self.players[player];
        let mut need = card.cost.master as i64;
        for color in Color::ALL {
            let due = card.cost.get(color).saturating_sub(me.bonuses.get(color));
            need += (due as i64 - me.tokens.get(color) as i64).max(0);
        }
        (need - me.tokens.master as i64).max(0)
    }

    /// 当前局面下最值得追求的牌，按「牌面价值 - 缺口」排序。
    fn best_target_card(&self, player: PlayerId) -> Option<Rc<Card>> {
        let mut best: Option<(i64, Rc<Card>)> = None;
        for category in Category::ALL {
            for card in self.board[category_index(category)].iter().flatten() {
                let score = self.target_score(player, card);
                if best.as_ref().map_or(true, |(current, _)| score > *current) {
                    best = Some((score, card.clone()));
                }
            }
        }
        for card in &self.players[player].reserved {
            let score = self.target_score(player, card);
            if best.as_ref().map_or(true, |(current, _)| score > *current) {
                best = Some((score, card.clone()));
            }
        }
        best.map(|(_, card)| card)
    }

    fn target_score(&self, player: PlayerId, card: &Card) -> i64 {
        let me = &self.players[player];
        let mut base = card.points as i64 * 20 + card.bonus_count as i64 * 10;
        if let (Some(_), Some(requirement)) = (&card.evolves_to_zh, &card.evolution_requirement) {
            base += if me.bonuses.get(requirement.color) >= requirement.count {
                20
            } else {
                10
            };
        }
        (base - self.capture_gap(player, card) * 5).max(0)
    }

    /// 弃球时希望保留的颜色数量：围绕当前最值得追求的牌计算。
    fn desired_needs(&self, player: PlayerId) -> [u8; 5] {
        let mut needs = [0u8; 5];
        let Some(card) = self.best_target_card(player) else {
            return needs;
        };
        for (slot, color) in Color::ALL.iter().enumerate() {
            let due = card
                .cost
                .get(*color)
                .saturating_sub(self.players[player].bonuses.get(*color));
            needs[slot] = due.saturating_sub(self.players[player].tokens.get(*color));
        }
        needs
    }

    fn visible_evolution_target(&self, player: PlayerId, source: &Card) -> bool {
        self.find_evolution_target(player, source).is_some()
    }

    /// 捕捉这张牌对当前玩家的进化推进价值。
    fn capture_evolution_value(&self, player: PlayerId, card: &Card) -> i64 {
        let me = &self.players[player];
        let mut value = 0;

        for source in &me.captured {
            let Some(requirement) = source.evolution_requirement.as_ref() else {
                continue;
            };
            if requirement.color != card.bonus_color {
                continue;
            }
            let current = me.bonuses.get(requirement.color);
            if current >= requirement.count {
                continue;
            }
            let remaining = requirement.count - current;
            let gain = evolution_gain_estimate(source);
            if card.bonus_count >= remaining {
                value += if self.visible_evolution_target(player, source) {
                    gain
                } else {
                    gain / 3
                };
            } else {
                value += 30;
            }
        }

        if let Some(requirement) = card.evolution_requirement.as_ref() {
            if me.bonuses.get(requirement.color) + card.bonus_count >= requirement.count {
                value += 150;
            }
        }
        value
    }

    /// 抢走这张牌会让哪个对手少掉多少进化价值。
    fn denial_value(&self, player: PlayerId, card: &Card) -> i64 {
        let mut value = 0;
        for (other, opponent) in self.players.iter().enumerate() {
            if other == player {
                continue;
            }
            for source in &opponent.captured {
                if source.category.next_level() != Some(card.category) {
                    continue;
                }
                if source.evolves_to_zh.as_deref() != Some(card.name_zh.as_str()) {
                    continue;
                }
                let ready = source
                    .evolution_requirement
                    .as_ref()
                    .is_some_and(|requirement| {
                        opponent.bonuses.get(requirement.color) >= requirement.count
                    });
                value += if ready { 600 } else { 120 };
            }
        }
        value
    }

    /// 动作排序分：只用于候选裁剪，不参与叶子评估。
    fn action_priority(&self, player: PlayerId, action: &Action) -> Option<i64> {
        match action {
            Action::Capture { source } => {
                let card = self.card_for_source(player, source)?;
                let mut score = card.points as i64 * 120 + card.bonus_count as i64 * 25;
                if self.points(player).saturating_add(card.points as u16) >= WIN_POINTS {
                    score += 5_000;
                }
                score -= card.cost.total() as i64 * 3;
                score += self.capture_evolution_value(player, &card);
                score += self.denial_value(player, &card);
                Some(score)
            }
            Action::TakeThree { .. } | Action::TakeTwo { .. } => {
                let needs = self.desired_needs(player);
                let slots: Vec<usize> = match action {
                    Action::TakeThree { colors } => {
                        colors.iter().map(|color| color_slot(*color)).collect()
                    }
                    Action::TakeTwo { color } => vec![color_slot(*color)],
                    _ => return None,
                };
                let count = slots.len() as i64;
                let mut score = 4 + count;
                for slot in &slots {
                    score += if needs[*slot] > 0 { 45 } else { 6 };
                }
                let after = self.players[player].tokens.total() as i64 + count;
                if after > TOKEN_LIMIT as i64 {
                    score -= 60;
                }
                let supply: i64 = Color::ALL
                    .iter()
                    .map(|color| self.supply.get(*color) as i64)
                    .sum();
                if supply - count <= 1 {
                    score -= 300;
                }
                Some(score)
            }
            Action::Reserve { source } => {
                let card = self.card_for_source(player, source)?;
                Some(20 + card_value(&card) / 6 + self.denial_value(player, &card))
            }
            _ => None,
        }
    }

    /// 按排序分取前 `width` 个动作。
    fn ordered_actions(&self, player: PlayerId, width: usize) -> Vec<Action> {
        let legal = self.legal_main_actions();
        let mut scored: Vec<(i64, usize)> = legal
            .iter()
            .enumerate()
            .filter_map(|(index, action)| {
                self.action_priority(player, action)
                    .map(|score| (score, index))
            })
            .collect();
        scored.sort_by(|left, right| right.0.cmp(&left.0).then(left.1.cmp(&right.1)));
        scored
            .into_iter()
            .take(width)
            .map(|(_, index)| legal[index].clone())
            .collect()
    }

    /// 局面评估：当前玩家视角的「我方局面分 - 最强对手局面分」。
    fn evaluate(&self, me: PlayerId) -> i64 {
        if self.over {
            return self.terminal_value(me);
        }
        // 规则层僵局：供应区没有普通球且当前玩家无牌可捕捉，按无胜者收尾。
        if self.legal_main_actions_for(self.current).is_empty() {
            return 0;
        }

        let mine = self.player_value(me);
        let opponent = (0..self.player_count())
            .filter(|&player| player != me)
            .map(|player| self.player_value(player))
            .max()
            .unwrap_or(0);
        mine - opponent
    }

    fn terminal_value(&self, me: PlayerId) -> i64 {
        let winners = self.winners.as_deref().unwrap_or(&[]);
        if winners.is_empty() {
            return 0;
        }
        let mine = self.points(me) as i64;
        let other = (0..self.player_count())
            .filter(|&player| player != me)
            .map(|player| self.points(player) as i64)
            .max()
            .unwrap_or(0);
        let margin = (mine - other) * 40;
        if winners.contains(&me) {
            TERMINAL_SCORE + margin
        } else {
            -TERMINAL_SCORE + margin
        }
    }

    fn player_value(&self, player: PlayerId) -> i64 {
        let me = &self.players[player];
        let points = me.points() as i64;
        let mut value = points * 20 + me.bonus_total() * 3;

        for card in &me.reserved {
            let gap = self.capture_gap(player, card);
            let value_here = card.points as i64 * 15 + card.bonus_count as i64 * 5;
            value += match gap {
                0 => value_here,
                1..=3 => value_here / 2,
                _ => -5,
            };
        }
        value += me.hidden_reserved as i64 * 3;
        value += self.ready_evolution_value(player);

        // 囤球但抓不到牌会同时浪费回合并把供应区抽干，容易触发规则僵局。
        let held = me.tokens.total();
        if held >= 8 && !self.has_affordable_capture(player) {
            value -= (held as i64 - 7) * 30;
            if held >= TOKEN_LIMIT {
                value -= 300;
            }
        }

        let best_action = self
            .legal_main_actions_for(player)
            .iter()
            .map(|action| self.heuristic_action_score(player, action))
            .max()
            .unwrap_or(0);
        value += best_action;
        value
    }

    /// 与 v1 同尺度的动作价值，用于叶子评估「下一手能做什么」。
    fn heuristic_action_score(&self, player: PlayerId, action: &Action) -> i64 {
        let me = &self.players[player];
        match action {
            Action::Capture { source } => {
                let Some(card) = self.card_for_source(player, source) else {
                    return 0;
                };
                let mut score = card.points as i64 * 20 + card.bonus_count as i64 * 3;
                if let (Some(_), Some(requirement)) =
                    (&card.evolves_to_zh, &card.evolution_requirement)
                {
                    score += 10;
                    if me.bonuses.get(requirement.color) >= requirement.count {
                        score += 20;
                    }
                }
                if me.points().saturating_add(card.points as u16) >= WIN_POINTS {
                    score += 1_000;
                }
                score - card.cost.total() as i64 / 2
            }
            Action::TakeThree { colors } => {
                let needs = self.desired_needs(player);
                let mut score = 8 + colors.len() as i64;
                for color in colors {
                    score += needs[color_slot(*color)] as i64 * 5;
                }
                if me.tokens.total() as i64 + colors.len() as i64 > TOKEN_LIMIT as i64 {
                    score -= 20;
                }
                score
            }
            Action::TakeTwo { color } => {
                let needs = self.desired_needs(player);
                let mut score = 10 + needs[color_slot(*color)] as i64 * 5;
                if me.tokens.total() as i64 + 2 > TOKEN_LIMIT as i64 {
                    score -= 20;
                }
                score
            }
            Action::Reserve { source } => {
                let Some(card) = self.card_for_source(player, source) else {
                    return 3;
                };
                let mut value = card.points as i64 * 10 + card.bonus_count as i64 * 2;
                if let (Some(_), Some(requirement)) =
                    (&card.evolves_to_zh, &card.evolution_requirement)
                {
                    value += 8;
                    if me.bonuses.get(requirement.color) >= requirement.count {
                        value += 16;
                    }
                }
                4 + value / 2
            }
            _ => 0,
        }
    }

    /// 已满足条件且目标可见的免费进化收益；目标暂时不可见时给少量期待值。
    fn ready_evolution_value(&self, player: PlayerId) -> i64 {
        let me = &self.players[player];
        let mut value = 0;

        for source in &me.captured {
            let Some(requirement) = source.evolution_requirement.as_ref() else {
                continue;
            };
            if source.evolves_to_zh.is_none() {
                continue;
            }
            if me.bonuses.get(requirement.color) < requirement.count {
                continue;
            }
            if let Some(target_source) = self.find_evolution_target(player, source) {
                if let Some(target) = self.card_for_source(player, &target_source) {
                    value += (target.points as i64 - source.points as i64).max(0) * 20
                        + (target.bonus_count as i64 - source.bonus_count as i64) * 3;
                }
            } else {
                value += 6;
            }
        }
        value
    }
}

/// 叶节点用局面评估的搜索主体。
struct Searcher;

impl Searcher {
    fn search(
        &mut self,
        state: &SearchState,
        me: PlayerId,
        depth: usize,
        mut alpha: i64,
        mut beta: i64,
    ) -> i64 {
        if state.over {
            return state.terminal_value(me);
        }
        if depth == 0 {
            return state.evaluate(me);
        }

        let current = state.current;
        let width = match depth {
            5 => 6,
            4 => 5,
            3 => 4,
            2 => 3,
            1 => 2,
            _ => 2,
        };
        let actions = state.ordered_actions(current, width);
        if actions.is_empty() {
            // 没有合法主行动 = 规则层僵局，判定为无胜者平局。
            return 0;
        }

        if current == me {
            let mut best = i64::MIN;
            for action in actions {
                let Some(next) = state.play_turn(&action) else {
                    continue;
                };
                let value = self.search(&next, me, depth - 1, alpha, beta);
                best = best.max(value);
                alpha = alpha.max(value);
                if alpha >= beta {
                    break;
                }
            }
            if best == i64::MIN {
                state.evaluate(me)
            } else {
                best
            }
        } else {
            let mut best = i64::MAX;
            for action in actions {
                let Some(next) = state.play_turn(&action) else {
                    continue;
                };
                let value = self.search(&next, me, depth - 1, alpha, beta);
                best = best.min(value);
                beta = beta.min(value);
                if alpha >= beta {
                    break;
                }
            }
            if best == i64::MAX {
                state.evaluate(me)
            } else {
                best
            }
        }
    }
}

/// Minimax bot：只在出现捕捉机会时用搜索树选点；拿球、保留、弃球与进化
/// 沿用启发式底座。搜索只使用公开信息，固定深度保证可复现。
pub struct MinimaxBot {
    base: HeuristicBot,
    rng: BotRng,
}

impl MinimaxBot {
    pub fn new(seed: u64) -> Self {
        Self {
            base: HeuristicBot::new(seed),
            rng: BotRng::new(seed),
        }
    }

    fn choose_main(&mut self, view: &GameView) -> Option<Action> {
        let base = self.base.choose_action(view)?;
        // 保留牌的取舍交给稳定的启发式底座：搜索对「提前囤 3 阶牌」的
        // 长期收益估计不可靠，容易把供应区抽干并触发规则僵局。
        if matches!(base, Action::Reserve { .. }) {
            return Some(base);
        }
        // 没有可捕捉的牌时直接沿用启发式底座：拿球 / 保留的长期取舍交给
        // 稳定策略，搜索只负责在捕捉候选中做对抗推演。
        if !view
            .legal_actions
            .iter()
            .any(|action| matches!(action, Action::Capture { .. }))
        {
            return Some(base);
        }
        let state = SearchState::from_view(view);
        let me = view.current_player;
        let candidates = self.root_candidates(&state, view, me, &base);

        let mut best_score = i64::MIN;
        let mut best: Vec<Action> = Vec::new();
        for action in candidates {
            let Some(after) = state.play_turn(&action) else {
                continue;
            };
            let score = self.search_after(&after, me);
            if score > best_score {
                best_score = score;
                best.clear();
                best.push(action);
            } else if score == best_score {
                best.push(action);
            }
        }

        if best.is_empty() {
            return Some(base);
        }
        let index = self.rng.below(best.len());
        Some(best[index].clone())
    }

    fn search_after(&mut self, state: &SearchState, me: PlayerId) -> i64 {
        Searcher.search(state, me, LOOKAHEAD_TURNS, i64::MIN / 2, i64::MAX / 2)
    }

    fn root_candidates(
        &self,
        state: &SearchState,
        view: &GameView,
        me: PlayerId,
        base: &Action,
    ) -> Vec<Action> {
        let base_priority = state.action_priority(me, base).unwrap_or(i64::MIN);
        let mut captures: Vec<(i64, usize)> = view
            .legal_actions
            .iter()
            .enumerate()
            .filter(|(_, action)| matches!(action, Action::Capture { .. }))
            .filter_map(|(index, action)| {
                let score = state.action_priority(me, action)?;
                (score >= base_priority.saturating_sub(ROOT_PRIORITY_MARGIN))
                    .then_some((score, index))
            })
            .collect();

        captures.sort_by(|left, right| right.0.cmp(&left.0).then(left.1.cmp(&right.1)));
        captures.truncate(ROOT_CAPTURE_WIDTH);

        let mut actions: Vec<Action> = captures
            .into_iter()
            .map(|(_, index)| view.legal_actions[index].clone())
            .collect();

        // 基准策略的动作保留在候选里，避免搜索裁剪掉稳定选择。
        if !actions.contains(base) {
            actions.push(base.clone());
        }
        actions
    }

    fn choose_discard(&mut self, view: &GameView) -> Option<Action> {
        self.base.choose_action(view)
    }

    fn choose_evolution(&mut self, view: &GameView) -> Option<Action> {
        self.base.choose_action(view)
    }
}

impl Bot for MinimaxBot {
    fn kind(&self) -> BotKind {
        BotKind::Minimax
    }

    fn choose_action(&mut self, view: &GameView) -> Option<Action> {
        match view.phase {
            Phase::MainAction => self.choose_main(view),
            Phase::Discard => self.choose_discard(view),
            Phase::Evolution => self.choose_evolution(view),
            Phase::GameOver => None,
        }
    }
}

// ---------------------------------------------------------------------------
// 工具函数
// ---------------------------------------------------------------------------

fn rc_slots(slots: &[Option<Card>]) -> Vec<Option<Rc<Card>>> {
    slots
        .iter()
        .map(|slot| slot.as_ref().map(|card| Rc::new(card.clone())))
        .collect()
}

fn category_index(category: Category) -> usize {
    match category {
        Category::Level1 => 0,
        Category::Level2 => 1,
        Category::Level3 => 2,
        Category::Rare => 3,
        Category::Legendary => 4,
    }
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

fn set_token_at(tokens: &mut TokenCounts, slot: usize, count: u8) {
    match slot {
        0 => tokens.red = count,
        1 => tokens.blue = count,
        2 => tokens.yellow = count,
        3 => tokens.pink = count,
        4 => tokens.black = count,
        _ => tokens.master = count,
    }
}

fn bonus_sum(captured: &[Rc<Card>]) -> TokenCounts {
    let mut bonuses = TokenCounts::ZERO;
    for card in captured {
        let current = bonuses.get(card.bonus_color);
        bonuses.set(card.bonus_color, current.saturating_add(card.bonus_count));
    }
    bonuses
}

fn card_value(card: &Card) -> i64 {
    card.points as i64 * 120 + card.bonus_count as i64 * 25
}

/// 完成一次进化的大致收益：下一阶段通常为 2 / 4 分。
fn evolution_gain_estimate(source: &Card) -> i64 {
    let target_points = match source.category {
        Category::Level1 => 2,
        Category::Level2 => 4,
        _ => 0,
    };
    (target_points - source.points as i64).max(0) * 120 + 60
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::Game;

    #[test]
    fn search_state_mirrors_public_view() {
        let game = Game::new(vec!["A".to_string(), "B".to_string()], 42).unwrap();
        let view = game.view_for(0);
        let state = SearchState::from_view(&view);

        assert_eq!(state.player_count(), 2);
        assert_eq!(state.current, 0);
        assert_eq!(state.supply, view.supply);
        assert_eq!(state.players[0].tokens, view.players[0].tokens);
        assert_eq!(state.players[1].tokens, view.players[1].tokens);
        assert_eq!(state.players[1].hidden_reserved, 0);
        assert_eq!(state.points(0), view.players[0].points);
        assert!(!state.legal_main_actions().is_empty());
    }

    #[test]
    fn search_state_keeps_board_reserved_cards_visible() {
        let mut game = Game::new(vec!["A".to_string(), "B".to_string()], 42).unwrap();
        game.apply(
            0,
            Action::TakeThree {
                colors: vec![Color::Red, Color::Blue, Color::Yellow],
            },
        )
        .unwrap();
        game.apply(0, Action::EndTurn).unwrap();
        game.apply(
            1,
            Action::Reserve {
                source: CardSource::board(Category::Level1, 0),
            },
        )
        .unwrap();
        game.apply(1, Action::EndTurn).unwrap();

        let state = SearchState::from_view(&game.view_for(0));
        assert_eq!(state.players[1].reserved.len(), 1);
        assert_eq!(state.players[1].hidden_reserved, 0);
    }

    #[test]
    fn search_state_only_counts_deck_reserves_as_hidden() {
        let mut game = Game::new(vec!["A".to_string(), "B".to_string()], 42).unwrap();
        game.apply(
            0,
            Action::TakeThree {
                colors: vec![Color::Red, Color::Blue, Color::Yellow],
            },
        )
        .unwrap();
        game.apply(0, Action::EndTurn).unwrap();
        game.apply(
            1,
            Action::Reserve {
                source: CardSource::deck_top(Category::Level1),
            },
        )
        .unwrap();
        game.apply(1, Action::EndTurn).unwrap();

        let state = SearchState::from_view(&game.view_for(0));
        assert!(state.players[1].reserved.is_empty());
        assert_eq!(state.players[1].hidden_reserved, 1);
    }

    #[test]
    fn minimax_choice_is_legal_and_deterministic() {
        let mut game = Game::new(vec!["A".to_string(), "B".to_string()], 7).unwrap();
        let view = game.view_for(0);
        let mut first = MinimaxBot::new(123);
        let mut second = MinimaxBot::new(123);

        let action = first.choose_action(&view).expect("minimax action");
        let repeat = second.choose_action(&view).expect("repeat action");
        assert_eq!(action, repeat);
        assert!(view.legal_actions.contains(&action));

        game.apply(0, action).expect("legal action");
    }

    #[test]
    fn simulated_turn_never_exceeds_token_limit() {
        let game = Game::new(vec!["A".to_string(), "B".to_string()], 11).unwrap();
        let view = game.view_for(0);
        let state = SearchState::from_view(&view);
        let actions = state.ordered_actions(0, 8);
        assert!(!actions.is_empty());

        for action in actions {
            let next = state.play_turn(&action).expect("simulated turn");
            assert!(next
                .players
                .iter()
                .all(|player| player.tokens.total() <= TOKEN_LIMIT));
        }
    }
}
