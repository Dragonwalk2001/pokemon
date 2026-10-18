//! 权威规则引擎。所有状态变更必须经过 [`Game::apply`]。

use serde::{Deserialize, Serialize};

use crate::catalog::Catalog;
use crate::error::GameError;
use crate::types::{
    Action, Card, CardId, CardSource, Category, Color, Event, Phase, Player, PlayerId, TokenCounts,
};

pub const MIN_PLAYERS: usize = 2;
pub const MAX_PLAYERS: usize = 4;
pub const MAX_RESERVED: usize = 3;
pub const TOKEN_LIMIT: u16 = 10;
pub const WIN_POINTS: u16 = 18;
pub const BOARD_SLOTS: usize = 4;

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Decks {
    level1: Vec<CardId>,
    level2: Vec<CardId>,
    level3: Vec<CardId>,
    rare: Vec<CardId>,
    legendary: Vec<CardId>,
}

impl Decks {
    fn pile(&self, category: Category) -> &Vec<CardId> {
        match category {
            Category::Level1 => &self.level1,
            Category::Level2 => &self.level2,
            Category::Level3 => &self.level3,
            Category::Rare => &self.rare,
            Category::Legendary => &self.legendary,
        }
    }

    fn pile_mut(&mut self, category: Category) -> &mut Vec<CardId> {
        match category {
            Category::Level1 => &mut self.level1,
            Category::Level2 => &mut self.level2,
            Category::Level3 => &mut self.level3,
            Category::Rare => &mut self.rare,
            Category::Legendary => &mut self.legendary,
        }
    }

    pub fn len(&self, category: Category) -> usize {
        self.pile(category).len()
    }

    pub fn is_empty(&self, category: Category) -> bool {
        self.pile(category).is_empty()
    }

    fn top(&self, category: Category) -> Option<CardId> {
        self.pile(category).last().copied()
    }

    fn draw(&mut self, category: Category) -> Option<CardId> {
        self.pile_mut(category).pop()
    }
}

/// 场上正面牌。每个槽位保存卡牌 ID；牌堆耗尽后槽位为 `None`。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Board {
    level1: Vec<Option<CardId>>,
    level2: Vec<Option<CardId>>,
    level3: Vec<Option<CardId>>,
    rare: Vec<Option<CardId>>,
    legendary: Vec<Option<CardId>>,
}

impl Board {
    fn empty() -> Self {
        Self {
            level1: vec![None; BOARD_SLOTS],
            level2: vec![None; BOARD_SLOTS],
            level3: vec![None; BOARD_SLOTS],
            rare: vec![None; 1],
            legendary: vec![None; 1],
        }
    }

    pub fn slots(&self, category: Category) -> &[Option<CardId>] {
        match category {
            Category::Level1 => &self.level1,
            Category::Level2 => &self.level2,
            Category::Level3 => &self.level3,
            Category::Rare => &self.rare,
            Category::Legendary => &self.legendary,
        }
    }

    fn slots_mut(&mut self, category: Category) -> &mut Vec<Option<CardId>> {
        match category {
            Category::Level1 => &mut self.level1,
            Category::Level2 => &mut self.level2,
            Category::Level3 => &mut self.level3,
            Category::Rare => &mut self.rare,
            Category::Legendary => &mut self.legendary,
        }
    }

    pub fn get(&self, category: Category, index: usize) -> Option<CardId> {
        self.slots(category).get(index).copied().flatten()
    }

    fn set(&mut self, category: Category, index: usize, card: Option<CardId>) {
        if let Some(slot) = self.slots_mut(category).get_mut(index) {
            *slot = card;
        }
    }

    fn take(&mut self, category: Category, index: usize) -> Option<CardId> {
        self.slots_mut(category)
            .get_mut(index)
            .and_then(|slot| slot.take())
    }
}

/// SplitMix64：确定性洗牌，保证同一 seed 可复现。
struct SplitMix64 {
    state: u64,
}

impl SplitMix64 {
    fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn shuffle<T>(&mut self, items: &mut [T]) {
        if items.len() < 2 {
            return;
        }
        for i in (1..items.len()).rev() {
            let j = (self.next_u64() % (i as u64 + 1)) as usize;
            items.swap(i, j);
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Game {
    catalog: Catalog,
    players: Vec<Player>,
    supply: TokenCounts,
    decks: Decks,
    board: Board,
    current: PlayerId,
    start_player: PlayerId,
    turn_number: u32,
    phase: Phase,
    evolved_this_turn: bool,
    end_triggered: bool,
    winners: Option<Vec<PlayerId>>,
    log: Vec<Event>,
    seed: u64,
}

impl Game {
    pub fn new(names: Vec<String>, seed: u64) -> Result<Self, GameError> {
        let catalog = Catalog::builtin()?;
        Self::with_catalog(catalog, names, seed)
    }

    pub fn with_catalog(
        catalog: Catalog,
        names: Vec<String>,
        seed: u64,
    ) -> Result<Self, GameError> {
        let player_count = names.len();
        if !(MIN_PLAYERS..=MAX_PLAYERS).contains(&player_count) {
            return Err(GameError::InvalidPlayerCount(player_count));
        }

        let mut rng = SplitMix64::new(seed);
        let mut decks = Decks::default();
        for category in Category::ALL {
            let mut ids: Vec<CardId> = catalog.by_category(category).map(|card| card.id).collect();
            ids.sort_unstable();
            rng.shuffle(&mut ids);
            *decks.pile_mut(category) = ids;
        }

        let mut board = Board::empty();
        for category in Category::ALL {
            let slots = board.slots(category).len();
            for index in 0..slots {
                let card = decks.draw(category);
                board.set(category, index, card);
            }
        }

        let per_color = match player_count {
            2 => 4,
            3 => 5,
            4 => 7,
            _ => unreachable!(),
        };
        let supply = TokenCounts::new(per_color, per_color, per_color, per_color, per_color, 5);

        let players: Vec<Player> = names
            .into_iter()
            .enumerate()
            .map(|(index, name)| {
                let trimmed = name.trim();
                let name = if trimmed.is_empty() {
                    format!("玩家{}", index + 1)
                } else {
                    trimmed.to_string()
                };
                Player::new(name)
            })
            .collect();

        let mut game = Self {
            catalog,
            players,
            supply,
            decks,
            board,
            current: 0,
            start_player: 0,
            turn_number: 1,
            phase: Phase::MainAction,
            evolved_this_turn: false,
            end_triggered: false,
            winners: None,
            log: Vec::new(),
            seed,
        };
        game.log.push(Event::GameStarted {
            players: player_count,
            seed,
        });
        game.log.push(Event::TurnStarted {
            player: game.current,
            turn: game.turn_number,
        });
        Ok(game)
    }

    pub fn catalog(&self) -> &Catalog {
        &self.catalog
    }

    pub fn card(&self, id: CardId) -> Option<&Card> {
        self.catalog.get(id)
    }

    pub fn players(&self) -> &[Player] {
        &self.players
    }

    pub fn player(&self, id: PlayerId) -> Option<&Player> {
        self.players.get(id)
    }

    pub fn player_name(&self, id: PlayerId) -> &str {
        self.players.get(id).map(Player::name).unwrap_or("?")
    }

    pub fn supply(&self) -> TokenCounts {
        self.supply
    }

    pub fn decks(&self) -> &Decks {
        &self.decks
    }

    pub fn board(&self) -> &Board {
        &self.board
    }

    pub fn current_player(&self) -> PlayerId {
        self.current
    }

    pub fn start_player(&self) -> PlayerId {
        self.start_player
    }

    pub fn turn_number(&self) -> u32 {
        self.turn_number
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    pub fn evolved_this_turn(&self) -> bool {
        self.evolved_this_turn
    }

    pub fn end_triggered(&self) -> bool {
        self.end_triggered
    }

    pub fn winners(&self) -> Option<&[PlayerId]> {
        self.winners.as_deref()
    }

    pub fn log(&self) -> &[Event] {
        &self.log
    }

    pub fn seed(&self) -> u64 {
        self.seed
    }

    pub fn is_over(&self) -> bool {
        self.phase == Phase::GameOver
    }

    pub fn points_for(&self, player: PlayerId) -> u16 {
        self.players
            .get(player)
            .map(|player| {
                player
                    .captured
                    .iter()
                    .filter_map(|id| self.catalog.get(*id))
                    .map(|card| card.points as u16)
                    .sum()
            })
            .unwrap_or(0)
    }

    pub fn bonuses_for(&self, player: PlayerId) -> TokenCounts {
        let mut bonuses = TokenCounts::ZERO;
        if let Some(player) = self.players.get(player) {
            for id in &player.captured {
                if let Some(card) = self.catalog.get(*id) {
                    let current = bonuses.get(card.bonus_color);
                    bonuses.set(card.bonus_color, current.saturating_add(card.bonus_count));
                }
            }
        }
        bonuses
    }

    /// 执行一个动作。调用方必须显式给出玩家 ID，非法动作不会改变任何状态。
    pub fn apply(&mut self, player: PlayerId, action: Action) -> Result<(), GameError> {
        if self.is_over() {
            return Err(GameError::GameOver);
        }
        if player >= self.players.len() {
            return Err(GameError::InvalidAction(format!("玩家 {player} 不存在")));
        }
        if player != self.current {
            return Err(GameError::NotYourTurn {
                player,
                current: self.current,
            });
        }

        match self.phase {
            Phase::MainAction => self.apply_main_action(action),
            Phase::Discard => self.apply_discard_action(action),
            Phase::Evolution => self.apply_evolution_action(action),
            Phase::GameOver => Err(GameError::GameOver),
        }
    }

    fn apply_main_action(&mut self, action: Action) -> Result<(), GameError> {
        let player = self.current;
        match action {
            Action::TakeThree { colors } => self.take_three(player, colors)?,
            Action::TakeTwo { color } => self.take_two(player, color)?,
            Action::Reserve { source } => self.reserve(player, source)?,
            Action::Capture { source } => self.capture(player, source)?,
            other => {
                return Err(GameError::WrongPhase {
                    phase: self.phase,
                    action: other.name(),
                })
            }
        }
        self.after_main_action();
        Ok(())
    }

    fn after_main_action(&mut self) {
        self.phase = if self.players[self.current].tokens.total() > TOKEN_LIMIT {
            Phase::Discard
        } else {
            Phase::Evolution
        };
    }

    fn take_three(&mut self, player: PlayerId, colors: Vec<Color>) -> Result<(), GameError> {
        if colors.is_empty() {
            return Err(GameError::InvalidAction("至少要选择一种精灵球".into()));
        }

        let mut seen = [false; 5];
        for color in &colors {
            if seen[color.index()] {
                return Err(GameError::InvalidAction(format!(
                    "拿取三种球时颜色不能重复（{} 出现了两次）",
                    color.label_zh()
                )));
            }
            seen[color.index()] = true;
        }

        let available: Vec<Color> = Color::ALL
            .iter()
            .copied()
            .filter(|color| self.supply.get(*color) > 0)
            .collect();

        if colors.iter().any(|color| self.supply.get(*color) == 0) {
            return Err(GameError::InvalidAction(
                "选择的颜色在供应区已经没有剩余".into(),
            ));
        }

        if available.len() >= 3 {
            if colors.len() != 3 {
                return Err(GameError::InvalidAction(format!(
                    "供应区有 {} 种可选球，必须拿取 3 种不同颜色",
                    available.len()
                )));
            }
        } else if colors.len() != available.len()
            || colors.iter().any(|color| !available.contains(color))
        {
            return Err(GameError::InvalidAction(format!(
                "供应区只剩 {} 种可选球，只能把仍可选的种类各拿 1 个",
                available.len()
            )));
        }

        let mut taken = TokenCounts::ZERO;
        for color in &colors {
            let supply = self.supply.get(*color);
            let held = self.players[player].tokens.get(*color);
            self.supply.set(*color, supply - 1);
            self.players[player].tokens.set(
                *color,
                held.checked_add(1).ok_or_else(|| {
                    GameError::Internal(format!("玩家 {player} 的{}球数量溢出", color.label_zh()))
                })?,
            );
            taken.set(*color, 1);
        }

        self.log.push(Event::BallsTaken {
            player,
            tokens: taken,
        });
        Ok(())
    }

    fn take_two(&mut self, player: PlayerId, color: Color) -> Result<(), GameError> {
        let supply = self.supply.get(color);
        if supply < 4 {
            return Err(GameError::InvalidAction(format!(
                "拿取两个同色球要求供应区至少有 4 个{}球，当前只有 {} 个",
                color.label_zh(),
                supply
            )));
        }
        let held = self.players[player].tokens.get(color);
        self.supply.set(color, supply - 2);
        self.players[player].tokens.set(
            color,
            held.checked_add(2).ok_or_else(|| {
                GameError::Internal(format!("玩家 {player} 的{}球数量溢出", color.label_zh()))
            })?,
        );

        let mut taken = TokenCounts::ZERO;
        taken.set(color, 2);
        self.log.push(Event::BallsTaken {
            player,
            tokens: taken,
        });
        Ok(())
    }

    fn reserve(&mut self, player: PlayerId, source: CardSource) -> Result<(), GameError> {
        if self.players[player].reserved.len() >= MAX_RESERVED {
            return Err(GameError::InvalidAction(format!(
                "最多只能保留 {MAX_RESERVED} 张牌"
            )));
        }

        let (card_id, board_slot, deck_category) = match &source {
            CardSource::Board { category, index } => {
                if !category.is_level() {
                    return Err(GameError::InvalidAction("稀有牌与传说牌不能保留".into()));
                }
                let card_id = self.board.get(*category, *index).ok_or_else(|| {
                    GameError::InvalidAction(format!(
                        "场上{}#{}没有可保留的牌",
                        category.label_zh(),
                        index
                    ))
                })?;
                (card_id, Some((*category, *index)), None)
            }
            CardSource::DeckTop { category } => {
                if !category.is_level() {
                    return Err(GameError::InvalidAction("稀有牌与传说牌不能保留".into()));
                }
                let card_id = self.decks.top(*category).ok_or_else(|| {
                    GameError::InvalidAction(format!("{}牌堆已经空了", category.label_zh()))
                })?;
                (card_id, None, Some(*category))
            }
            CardSource::Reserved { .. } => {
                return Err(GameError::InvalidAction("不能保留自己已经保留的牌".into()))
            }
        };

        if let Some((category, index)) = board_slot {
            self.board.take(category, index);
            self.refill_slot(category, index);
        }
        if let Some(category) = deck_category {
            self.decks.draw(category);
        }

        self.players[player].reserved.push(card_id);

        let got_master = if self.supply.master > 0 {
            self.supply.master -= 1;
            let held = self.players[player].tokens.master;
            self.players[player].tokens.master = held + 1;
            true
        } else {
            false
        };

        self.log.push(Event::CardReserved {
            player,
            from_deck: deck_category.is_some(),
            card: board_slot.map(|_| card_id),
            got_master,
        });
        Ok(())
    }

    fn capture(&mut self, player: PlayerId, source: CardSource) -> Result<(), GameError> {
        let (card_id, board_slot) = match &source {
            CardSource::Board { category, index } => {
                let card_id = self.board.get(*category, *index).ok_or_else(|| {
                    GameError::InvalidAction(format!(
                        "场上{}#{}没有可捕捉的牌",
                        category.label_zh(),
                        index
                    ))
                })?;
                (card_id, Some((*category, *index)))
            }
            CardSource::Reserved { index } => {
                let card_id = self.players[player]
                    .reserved
                    .get(*index)
                    .copied()
                    .ok_or_else(|| {
                        GameError::InvalidAction(format!("你没有第 {index} 张保留牌"))
                    })?;
                (card_id, None)
            }
            CardSource::DeckTop { .. } => {
                return Err(GameError::InvalidAction("不能直接捕捉牌堆顶的牌".into()))
            }
        };

        let card = self
            .catalog
            .get(card_id)
            .cloned()
            .ok_or_else(|| GameError::Internal(format!("卡牌 {card_id} 不在目录中")))?;
        let payment = self.compute_payment(player, &card)?;

        let mut next_tokens = self.players[player].tokens;
        next_tokens
            .checked_sub(payment)
            .ok_or_else(|| GameError::Internal("支付额超过玩家持有量".into()))?;
        let mut next_supply = self.supply;
        next_supply
            .checked_add(payment)
            .ok_or_else(|| GameError::Internal("供应区数量溢出".into()))?;

        self.players[player].tokens = next_tokens;
        self.supply = next_supply;

        if let Some((category, index)) = board_slot {
            self.board.take(category, index);
            self.refill_slot(category, index);
        }
        if let CardSource::Reserved { index } = source {
            self.players[player].reserved.remove(index);
        }
        self.players[player].captured.push(card_id);

        self.log.push(Event::CardCaptured {
            player,
            card: card_id,
            payment,
        });
        Ok(())
    }

    /// 支付策略：彩色球优先，颜色不足的差额用大师球补，最后再加印刷的大师球费用。
    fn compute_payment(&self, player: PlayerId, card: &Card) -> Result<TokenCounts, GameError> {
        let bonuses = self.bonuses_for(player);
        let tokens = self.players[player].tokens;
        let mut payment = TokenCounts::ZERO;
        let mut master_needed = card.cost.master as u16;

        for color in Color::ALL {
            let due = card.cost.get(color).saturating_sub(bonuses.get(color));
            let paid = due.min(tokens.get(color));
            payment.set(color, paid);
            master_needed += (due - paid) as u16;
        }

        if master_needed > tokens.master as u16 {
            return Err(GameError::InvalidAction(format!(
                "捕捉「{}」需要 {} 个大师球（含印刷费用），你只有 {} 个",
                card.name_zh, master_needed, tokens.master
            )));
        }
        payment.master = master_needed as u8;
        Ok(payment)
    }

    fn refill_slot(&mut self, category: Category, index: usize) {
        let next = self.decks.draw(category);
        self.board.set(category, index, next);
    }

    fn apply_discard_action(&mut self, action: Action) -> Result<(), GameError> {
        let tokens = match action {
            Action::Discard { tokens } => tokens,
            other => {
                return Err(GameError::WrongPhase {
                    phase: self.phase,
                    action: other.name(),
                })
            }
        };
        let player = self.current;
        let held = self.players[player].tokens;

        if tokens.is_zero() {
            return Err(GameError::InvalidAction(
                "回合末球数超过 10，必须弃掉至少一个精灵球".into(),
            ));
        }
        for color in Color::ALL {
            if tokens.get(color) > held.get(color) {
                return Err(GameError::InvalidAction(format!(
                    "你只有 {} 个{}球，不能弃掉 {} 个",
                    held.get(color),
                    color.label_zh(),
                    tokens.get(color)
                )));
            }
        }
        if tokens.master > held.master {
            return Err(GameError::InvalidAction(format!(
                "你只有 {} 个大师球，不能弃掉 {} 个",
                held.master, tokens.master
            )));
        }

        let after = held.total().saturating_sub(tokens.total());
        if after > TOKEN_LIMIT {
            return Err(GameError::InvalidAction(format!(
                "弃球后仍有 {after} 个精灵球，超过上限 {TOKEN_LIMIT}"
            )));
        }

        let mut next_tokens = held;
        next_tokens
            .checked_sub(tokens)
            .ok_or_else(|| GameError::Internal("弃球数量超过持有量".into()))?;
        let mut next_supply = self.supply;
        next_supply
            .checked_add(tokens)
            .ok_or_else(|| GameError::Internal("供应区数量溢出".into()))?;

        self.players[player].tokens = next_tokens;
        self.supply = next_supply;
        self.log.push(Event::BallsDiscarded { player, tokens });
        self.phase = Phase::Evolution;
        Ok(())
    }

    fn apply_evolution_action(&mut self, action: Action) -> Result<(), GameError> {
        match action {
            Action::Evolve {
                captured_index,
                target,
            } => self.evolve(self.current, captured_index, target),
            Action::EndTurn => self.end_turn(),
            other => Err(GameError::WrongPhase {
                phase: self.phase,
                action: other.name(),
            }),
        }
    }

    fn evolve(
        &mut self,
        player: PlayerId,
        captured_index: usize,
        target_source: CardSource,
    ) -> Result<(), GameError> {
        if self.evolved_this_turn {
            return Err(GameError::InvalidAction("每回合最多进化一只宝可梦".into()));
        }

        let source_id = *self.players[player]
            .captured
            .get(captured_index)
            .ok_or_else(|| {
                GameError::InvalidAction(format!("你没有第 {captured_index} 张已捕捉牌"))
            })?;
        let source = self
            .catalog
            .get(source_id)
            .cloned()
            .ok_or_else(|| GameError::Internal(format!("卡牌 {source_id} 不在目录中")))?;
        let next_category = source
            .category
            .next_level()
            .ok_or_else(|| GameError::InvalidAction("稀有牌与传说牌不能进化".into()))?;
        let expected_name = source
            .evolves_to_zh
            .clone()
            .ok_or_else(|| GameError::InvalidAction("这张牌没有进化目标".into()))?;
        let requirement = source
            .evolution_requirement
            .clone()
            .ok_or_else(|| GameError::InvalidAction("这张牌没有进化条件".into()))?;

        let (target_id, board_slot) = match &target_source {
            CardSource::Board { category, index } => {
                if *category != next_category {
                    return Err(GameError::InvalidAction(format!(
                        "「{}」只能进化为{}的「{}」",
                        source.name_zh,
                        next_category.label_zh(),
                        expected_name
                    )));
                }
                let card_id = self.board.get(*category, *index).ok_or_else(|| {
                    GameError::InvalidAction(format!(
                        "场上{}#{}没有可用于进化的牌",
                        category.label_zh(),
                        index
                    ))
                })?;
                (card_id, Some((*category, *index)))
            }
            CardSource::Reserved { index } => {
                let card_id = self.players[player]
                    .reserved
                    .get(*index)
                    .copied()
                    .ok_or_else(|| {
                        GameError::InvalidAction(format!("你没有第 {index} 张保留牌"))
                    })?;
                (card_id, None)
            }
            CardSource::DeckTop { .. } => {
                return Err(GameError::InvalidAction(
                    "进化目标必须位于场上或自己的保留牌中".into(),
                ))
            }
        };

        let target = self
            .catalog
            .get(target_id)
            .cloned()
            .ok_or_else(|| GameError::Internal(format!("卡牌 {target_id} 不在目录中")))?;
        if target.category != next_category || target.name_zh != expected_name {
            return Err(GameError::InvalidAction(format!(
                "「{}」只能进化为{}的「{}」，不能是「{}」",
                source.name_zh,
                next_category.label_zh(),
                expected_name,
                target.name_zh
            )));
        }

        let bonuses = self.bonuses_for(player);
        if bonuses.get(requirement.color) < requirement.count {
            return Err(GameError::InvalidAction(format!(
                "进化「{}」需要 {}{}减免，你只有 {} 个",
                source.name_zh,
                requirement.count,
                requirement.color.label_zh(),
                bonuses.get(requirement.color)
            )));
        }

        if let Some((category, index)) = board_slot {
            self.board.take(category, index);
            self.refill_slot(category, index);
        }
        if let CardSource::Reserved { index } = target_source {
            self.players[player].reserved.remove(index);
        }

        self.players[player].captured.remove(captured_index);
        self.players[player].evolved_away.push(source_id);
        self.players[player].captured.push(target_id);
        self.evolved_this_turn = true;

        self.log.push(Event::PokemonEvolved {
            player,
            from: source_id,
            to: target_id,
        });
        Ok(())
    }

    fn end_turn(&mut self) -> Result<(), GameError> {
        let player = self.current;
        if self.players[player].tokens.total() > TOKEN_LIMIT {
            return Err(GameError::InvalidAction(format!(
                "结束回合前必须把精灵球弃至 {TOKEN_LIMIT} 个以内"
            )));
        }

        let turns_taken = self.players[player]
            .turns_taken
            .checked_add(1)
            .ok_or_else(|| GameError::Internal("回合计数溢出".into()))?;
        self.players[player].turns_taken = turns_taken;
        self.log.push(Event::TurnEnded { player });

        if !self.end_triggered {
            if let Some(reached) =
                (0..self.players.len()).find(|&id| self.points_for(id) >= WIN_POINTS)
            {
                self.end_triggered = true;
                self.log.push(Event::FinalRoundTriggered {
                    player: reached,
                    points: self.points_for(reached),
                });
            }
        }

        if self.end_triggered {
            let target_turns = self.players[0].turns_taken;
            if self
                .players
                .iter()
                .all(|player| player.turns_taken == target_turns)
            {
                self.finish_game();
                return Ok(());
            }
        }

        self.current = (self.current + 1) % self.players.len();
        self.turn_number = self
            .turn_number
            .checked_add(1)
            .ok_or_else(|| GameError::Internal("回合编号溢出".into()))?;
        self.evolved_this_turn = false;
        self.phase = Phase::MainAction;
        self.log.push(Event::TurnStarted {
            player: self.current,
            turn: self.turn_number,
        });
        Ok(())
    }

    fn finish_game(&mut self) {
        let points: Vec<u16> = (0..self.players.len())
            .map(|id| self.points_for(id))
            .collect();
        let max_points = points.iter().copied().max().unwrap_or(0);
        let mut winners: Vec<PlayerId> = (0..self.players.len())
            .filter(|&id| points[id] == max_points)
            .collect();

        if winners.len() > 1 {
            let max_evolutions = winners
                .iter()
                .map(|&id| self.players[id].evolved_away.len())
                .max()
                .unwrap_or(0);
            winners.retain(|&id| self.players[id].evolved_away.len() == max_evolutions);
        }
        if winners.len() > 1 {
            let max_cards = winners
                .iter()
                .map(|&id| self.players[id].captured.len())
                .max()
                .unwrap_or(0);
            winners.retain(|&id| self.players[id].captured.len() == max_cards);
        }

        self.winners = Some(winners.clone());
        self.phase = Phase::GameOver;
        self.log.push(Event::GameEnded { winners, points });
    }

    /// 列出当前玩家当前阶段的合法动作。
    ///
    /// 弃球阶段不枚举组合（数量太多），调用方需要自行构造 [`Action::Discard`]，
    /// 引擎会校验数量。进化阶段始终包含 [`Action::EndTurn`]。
    pub fn legal_actions(&self) -> Vec<Action> {
        if self.is_over() {
            return Vec::new();
        }
        match self.phase {
            Phase::MainAction => self.legal_main_actions(),
            Phase::Discard => Vec::new(),
            Phase::Evolution => self.legal_evolution_actions(),
            Phase::GameOver => Vec::new(),
        }
    }

    fn legal_main_actions(&self) -> Vec<Action> {
        let player = self.current;
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
                for (index, slot) in self.board.slots(category).iter().enumerate() {
                    if slot.is_some() {
                        actions.push(Action::Reserve {
                            source: CardSource::board(category, index),
                        });
                    }
                }
                if !self.decks.is_empty(category) {
                    actions.push(Action::Reserve {
                        source: CardSource::deck_top(category),
                    });
                }
            }
        }

        for category in Category::ALL {
            for (index, slot) in self.board.slots(category).iter().enumerate() {
                if let Some(card_id) = slot {
                    if self.can_afford(player, *card_id) {
                        actions.push(Action::Capture {
                            source: CardSource::board(category, index),
                        });
                    }
                }
            }
        }
        for (index, card_id) in self.players[player].reserved.iter().enumerate() {
            if self.can_afford(player, *card_id) {
                actions.push(Action::Capture {
                    source: CardSource::reserved(index),
                });
            }
        }

        actions
    }

    fn legal_evolution_actions(&self) -> Vec<Action> {
        let player = self.current;
        let mut actions = Vec::new();

        if !self.evolved_this_turn {
            let bonuses = self.bonuses_for(player);
            for (captured_index, source_id) in self.players[player].captured.iter().enumerate() {
                let Some(source) = self.catalog.get(*source_id) else {
                    continue;
                };
                let Some(next_category) = source.category.next_level() else {
                    continue;
                };
                let Some(expected_name) = source.evolves_to_zh.as_deref() else {
                    continue;
                };
                let Some(requirement) = source.evolution_requirement.as_ref() else {
                    continue;
                };
                if bonuses.get(requirement.color) < requirement.count {
                    continue;
                }

                for (index, slot) in self.board.slots(next_category).iter().enumerate() {
                    if let Some(target_id) = slot {
                        let matches = self
                            .catalog
                            .get(*target_id)
                            .map(|target| {
                                target.category == next_category
                                    && target.name_zh.as_str() == expected_name
                            })
                            .unwrap_or(false);
                        if matches {
                            actions.push(Action::Evolve {
                                captured_index,
                                target: CardSource::board(next_category, index),
                            });
                        }
                    }
                }
                for (index, target_id) in self.players[player].reserved.iter().enumerate() {
                    let matches = self
                        .catalog
                        .get(*target_id)
                        .map(|target| {
                            target.category == next_category
                                && target.name_zh.as_str() == expected_name
                        })
                        .unwrap_or(false);
                    if matches {
                        actions.push(Action::Evolve {
                            captured_index,
                            target: CardSource::reserved(index),
                        });
                    }
                }
            }
        }

        actions.push(Action::EndTurn);
        actions
    }

    fn can_afford(&self, player: PlayerId, card_id: CardId) -> bool {
        self.catalog
            .get(card_id)
            .map(|card| self.compute_payment(player, card).is_ok())
            .unwrap_or(false)
    }
}

impl Event {
    /// 面向 CLI / 日志的中文描述。
    pub fn describe(&self, game: &Game) -> String {
        match self {
            Event::GameStarted { players, seed } => {
                format!("对局开始：{players} 名玩家，随机种子 {seed}")
            }
            Event::TurnStarted { player, turn } => {
                format!("第 {turn} 回合，轮到 {}", game.player_name(*player))
            }
            Event::BallsTaken { player, tokens } => {
                format!("{} 拿取 {}", game.player_name(*player), tokens)
            }
            Event::CardReserved {
                player,
                from_deck,
                card,
                got_master,
            } => {
                let what = if *from_deck {
                    "牌堆顶（隐藏）".to_string()
                } else {
                    (*card)
                        .map(|id| card_name(game, id))
                        .unwrap_or_else(|| "场上牌".to_string())
                };
                let master = if *got_master {
                    "，获得 1 个大师球"
                } else {
                    ""
                };
                format!("{} 保留 {}{}", game.player_name(*player), what, master)
            }
            Event::CardCaptured {
                player,
                card,
                payment,
            } => format!(
                "{} 捕捉 {}，支付 {}",
                game.player_name(*player),
                card_name(game, *card),
                payment
            ),
            Event::BallsDiscarded { player, tokens } => {
                format!("{} 弃球 {}", game.player_name(*player), tokens)
            }
            Event::PokemonEvolved { player, from, to } => format!(
                "{} 将 {} 进化为 {}",
                game.player_name(*player),
                card_name(game, *from),
                card_name(game, *to)
            ),
            Event::TurnEnded { player } => {
                format!("{} 结束回合", game.player_name(*player))
            }
            Event::FinalRoundTriggered { player, points } => format!(
                "{} 达到 {points} 分，触发终局；本轮补完后结算",
                game.player_name(*player)
            ),
            Event::GameEnded { winners, points } => {
                let names: Vec<String> = winners
                    .iter()
                    .map(|id| {
                        format!(
                            "{}({}分)",
                            game.player_name(*id),
                            points.get(*id).copied().unwrap_or(0)
                        )
                    })
                    .collect();
                format!("对局结束，胜者：{}", names.join("、"))
            }
        }
    }
}

fn card_name(game: &Game, id: CardId) -> String {
    game.card(id)
        .map(|card| card.name_zh.clone())
        .unwrap_or_else(|| format!("#{id}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new_game(players: usize, seed: u64) -> Game {
        Game::new(
            (1..=players).map(|index| format!("P{index}")).collect(),
            seed,
        )
        .unwrap()
    }

    #[test]
    fn setup_token_counts_and_deck_sizes() {
        for (players, per_color) in [(2, 4), (3, 5), (4, 7)] {
            let game = new_game(players, 42);
            assert_eq!(
                game.supply,
                TokenCounts::new(per_color, per_color, per_color, per_color, per_color, 5)
            );
            for category in Category::LEVELS {
                assert_eq!(game.board.slots(category).len(), BOARD_SLOTS);
            }
            assert_eq!(game.decks.len(Category::Level1), 31);
            assert_eq!(game.decks.len(Category::Level2), 26);
            assert_eq!(game.decks.len(Category::Level3), 11);
            assert_eq!(game.decks.len(Category::Rare), 4);
            assert_eq!(game.decks.len(Category::Legendary), 4);
            assert_eq!(game.phase, Phase::MainAction);
            assert_eq!(game.current, 0);
        }
    }

    #[test]
    fn take_three_when_fewer_than_three_colors_remain() {
        let mut game = new_game(2, 1);
        game.supply = TokenCounts::new(0, 2, 1, 0, 0, 5);

        assert!(game
            .apply(
                0,
                Action::TakeThree {
                    colors: vec![Color::Blue],
                },
            )
            .is_err());

        game.apply(
            0,
            Action::TakeThree {
                colors: vec![Color::Blue, Color::Yellow],
            },
        )
        .unwrap();

        assert_eq!(game.players[0].tokens.get(Color::Blue), 1);
        assert_eq!(game.players[0].tokens.get(Color::Yellow), 1);
        assert_eq!(game.supply.get(Color::Blue), 1);
        assert_eq!(game.supply.get(Color::Yellow), 0);
        assert_eq!(game.phase, Phase::Evolution);
    }

    #[test]
    fn take_three_rejects_duplicate_colors() {
        let mut game = new_game(2, 1);
        assert!(game
            .apply(
                0,
                Action::TakeThree {
                    colors: vec![Color::Red, Color::Red, Color::Blue],
                },
            )
            .is_err());
    }

    #[test]
    fn reserve_from_board_refills_slot_and_grants_master() {
        let mut game = new_game(2, 1);
        let card_id = game.board.get(Category::Level1, 0).unwrap();
        let deck_top = game.decks.top(Category::Level1).unwrap();

        game.apply(
            0,
            Action::Reserve {
                source: CardSource::board(Category::Level1, 0),
            },
        )
        .unwrap();

        assert_eq!(game.players[0].reserved, vec![card_id]);
        assert_eq!(game.players[0].tokens.master, 1);
        assert_eq!(game.supply.master, 4);
        assert_eq!(game.board.get(Category::Level1, 0), Some(deck_top));
        assert_eq!(game.decks.len(Category::Level1), 30);
    }

    #[test]
    fn reserve_respects_limit_and_rarity() {
        let mut game = new_game(2, 1);
        game.players[0].reserved = vec![1100, 1101, 1102];
        assert!(game
            .apply(
                0,
                Action::Reserve {
                    source: CardSource::board(Category::Level1, 0),
                },
            )
            .is_err());

        game.players[0].reserved.clear();
        assert!(game
            .apply(
                0,
                Action::Reserve {
                    source: CardSource::board(Category::Rare, 0),
                },
            )
            .is_err());
        assert!(game
            .apply(
                0,
                Action::Reserve {
                    source: CardSource::deck_top(Category::Rare),
                },
            )
            .is_err());
    }

    #[test]
    fn capture_applies_bonus_and_returns_tokens_to_supply() {
        let mut game = new_game(2, 1);
        let card = game.catalog.get(1101).unwrap().clone();
        assert_eq!(card.name_zh, "喇叭芽");
        assert_eq!(card.cost.get(Color::Red), 2);
        assert_eq!(card.cost.get(Color::Pink), 2);

        game.players[0].captured = vec![1100];
        game.players[0].tokens = TokenCounts::new(1, 0, 0, 2, 0, 0);
        game.board.set(Category::Level1, 0, Some(1101));
        let supply_before = game.supply;

        game.apply(
            0,
            Action::Capture {
                source: CardSource::board(Category::Level1, 0),
            },
        )
        .unwrap();

        assert_eq!(game.players[0].tokens, TokenCounts::ZERO);
        assert_eq!(game.supply.red, supply_before.red + 1);
        assert_eq!(game.supply.pink, supply_before.pink + 2);
        assert!(game.players[0].captured.contains(&1101));
        assert_ne!(game.board.get(Category::Level1, 0), Some(1101));
        assert_eq!(game.phase, Phase::Evolution);
    }

    #[test]
    fn capture_uses_master_balls_for_missing_colors() {
        let mut game = new_game(2, 1);
        game.players[0].tokens = TokenCounts::new(2, 0, 0, 0, 0, 2);
        game.board.set(Category::Level1, 0, Some(1101));

        game.apply(
            0,
            Action::Capture {
                source: CardSource::board(Category::Level1, 0),
            },
        )
        .unwrap();

        assert_eq!(game.players[0].tokens, TokenCounts::ZERO);
        assert_eq!(game.supply.red, 6);
        assert_eq!(game.supply.pink, 4);
        assert_eq!(game.supply.master, 7);
    }

    #[test]
    fn failed_capture_does_not_mutate_state() {
        let mut game = new_game(2, 1);
        game.board.set(Category::Level1, 0, Some(1101));
        let supply_before = game.supply;

        assert!(game
            .apply(
                0,
                Action::Capture {
                    source: CardSource::board(Category::Level1, 0),
                },
            )
            .is_err());

        assert_eq!(game.board.get(Category::Level1, 0), Some(1101));
        assert!(game.players[0].captured.is_empty());
        assert_eq!(game.supply, supply_before);
        assert_eq!(game.phase, Phase::MainAction);
    }

    #[test]
    fn discard_trims_tokens_to_limit() {
        let mut game = new_game(2, 1);
        game.players[0].tokens = TokenCounts::new(10, 0, 0, 0, 0, 1);
        game.phase = Phase::Discard;

        assert!(game
            .apply(
                0,
                Action::Discard {
                    tokens: TokenCounts::ZERO,
                },
            )
            .is_err());

        game.apply(
            0,
            Action::Discard {
                tokens: TokenCounts::new(2, 0, 0, 0, 0, 0),
            },
        )
        .unwrap();

        assert_eq!(game.players[0].tokens.total(), 9);
        assert_eq!(game.supply.red, 6);
        assert_eq!(game.phase, Phase::Evolution);
    }

    fn evolution_fixture() -> (Game, CardId, CardId, Vec<CardId>) {
        let game = new_game(2, 1);
        let source_id = 1100;
        let target_id = game
            .catalog
            .by_category(Category::Level2)
            .find(|card| card.name_zh == "口呆花")
            .unwrap()
            .id;
        let pink_bonus: Vec<CardId> = game
            .catalog
            .by_category(Category::Level1)
            .filter(|card| card.bonus_color == Color::Pink)
            .take(2)
            .map(|card| card.id)
            .collect();
        (game, source_id, target_id, pink_bonus)
    }

    #[test]
    fn evolution_requires_bonus_and_replaces_card() {
        let (mut game, source_id, target_id, pink_bonus) = evolution_fixture();
        game.players[0].captured = vec![source_id];
        game.board.set(Category::Level2, 0, Some(target_id));
        game.phase = Phase::Evolution;

        assert!(game
            .apply(
                0,
                Action::Evolve {
                    captured_index: 0,
                    target: CardSource::board(Category::Level2, 0),
                },
            )
            .is_err());
        assert_eq!(game.players[0].captured, vec![source_id]);

        game.players[0].captured.extend(pink_bonus);
        game.apply(
            0,
            Action::Evolve {
                captured_index: 0,
                target: CardSource::board(Category::Level2, 0),
            },
        )
        .unwrap();

        assert!(!game.players[0].captured.contains(&source_id));
        assert!(game.players[0].captured.contains(&target_id));
        assert_eq!(game.players[0].evolved_away, vec![source_id]);
        assert!(game.evolved_this_turn);
        assert_ne!(game.board.get(Category::Level2, 0), Some(target_id));

        assert!(game
            .apply(
                0,
                Action::Evolve {
                    captured_index: 0,
                    target: CardSource::board(Category::Level2, 0),
                },
            )
            .is_err());
    }

    #[test]
    fn evolution_can_take_target_from_reserved() {
        let (mut game, source_id, target_id, pink_bonus) = evolution_fixture();
        game.players[0].captured = vec![source_id];
        game.players[0].captured.extend(pink_bonus);
        game.players[0].reserved = vec![target_id];
        game.phase = Phase::Evolution;

        game.apply(
            0,
            Action::Evolve {
                captured_index: 0,
                target: CardSource::reserved(0),
            },
        )
        .unwrap();

        assert!(game.players[0].reserved.is_empty());
        assert!(game.players[0].captured.contains(&target_id));
    }

    #[test]
    fn evolution_rejects_wrong_species() {
        let (mut game, source_id, _, pink_bonus) = evolution_fixture();
        let other_id = game
            .catalog
            .by_category(Category::Level2)
            .find(|card| card.name_zh != "口呆花")
            .unwrap()
            .id;
        game.players[0].captured = vec![source_id];
        game.players[0].captured.extend(pink_bonus);
        game.board.set(Category::Level2, 0, Some(other_id));
        game.phase = Phase::Evolution;

        assert!(game
            .apply(
                0,
                Action::Evolve {
                    captured_index: 0,
                    target: CardSource::board(Category::Level2, 0),
                },
            )
            .is_err());
    }

    fn give_winning_cards(game: &mut Game, player: PlayerId) {
        let mut chosen = Vec::new();
        let mut points = 0u16;
        for id in game
            .catalog
            .by_category(Category::Level3)
            .map(|card| card.id)
        {
            let card_points = game.catalog.get(id).unwrap().points as u16;
            if card_points == 0 {
                continue;
            }
            chosen.push(id);
            points += card_points;
            if points >= WIN_POINTS {
                break;
            }
        }
        assert!(points >= WIN_POINTS);
        game.players[player].captured = chosen;
    }

    #[test]
    fn final_round_completes_equal_turns() {
        let mut game = new_game(3, 1);
        give_winning_cards(&mut game, 0);
        game.phase = Phase::Evolution;
        game.apply(0, Action::EndTurn).unwrap();

        assert!(game.end_triggered);
        assert!(!game.is_over());
        assert_eq!(game.current, 1);

        game.phase = Phase::Evolution;
        game.apply(1, Action::EndTurn).unwrap();
        assert!(!game.is_over());

        game.phase = Phase::Evolution;
        game.apply(2, Action::EndTurn).unwrap();
        assert!(game.is_over());
        assert_eq!(game.winners.as_deref(), Some(&[0usize][..]));
    }

    #[test]
    fn tiebreakers_follow_evolutions_then_card_count() {
        let mut game = new_game(2, 1);
        game.players[0].captured = vec![3520, 3520, 3520, 3520];
        game.players[1].captured = vec![3520, 3520, 3520, 3520];
        game.players[1].evolved_away = vec![1100];
        game.finish_game();
        assert_eq!(game.winners.as_deref(), Some(&[1usize][..]));

        let mut game = new_game(2, 1);
        let four_point: Vec<CardId> = game
            .catalog
            .by_category(Category::Level3)
            .filter(|card| card.points == 4)
            .take(5)
            .map(|card| card.id)
            .collect();
        assert_eq!(four_point.len(), 5);
        game.players[0].captured = vec![3520, 3520, 3520, 3520];
        game.players[1].captured = four_point;
        game.finish_game();
        assert_eq!(game.winners.as_deref(), Some(&[1usize][..]));

        let mut game = new_game(2, 1);
        game.players[0].captured = vec![3520, 3520, 3520, 3520];
        game.players[1].captured = vec![3520, 3520, 3520, 3520];
        game.finish_game();
        assert_eq!(game.winners.as_deref(), Some(&[0usize, 1usize][..]));
    }

    #[test]
    fn capture_rare_card_grants_double_bonus_and_pays_master() {
        let mut game = new_game(2, 1);
        let card = game.catalog.get(4100).unwrap().clone();
        assert_eq!(card.category, Category::Rare);
        assert_eq!(card.bonus_color, Color::Red);
        assert_eq!(card.bonus_count, 2);
        assert_eq!(card.cost.master, 1);

        game.players[0].tokens = TokenCounts::new(0, 2, 0, 0, 3, 1);
        game.board.set(Category::Rare, 0, Some(4100));

        game.apply(
            0,
            Action::Capture {
                source: CardSource::board(Category::Rare, 0),
            },
        )
        .unwrap();

        assert_eq!(game.players[0].tokens, TokenCounts::ZERO);
        assert_eq!(game.bonuses_for(0).get(Color::Red), 2);
        assert_eq!(game.supply.master, 6);
        assert!(game.players[0].captured.contains(&4100));
    }

    #[test]
    fn game_roundtrips_through_json() {
        let game = new_game(3, 99);
        let json = serde_json::to_string(&game).unwrap();
        let restored: Game = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.supply(), game.supply());
        assert_eq!(restored.current_player(), game.current_player());
        assert_eq!(restored.players().len(), 3);
        for category in Category::ALL {
            assert_eq!(
                restored.board().slots(category),
                game.board().slots(category)
            );
            assert_eq!(restored.decks().len(category), game.decks().len(category));
        }
    }
}
