//! 面向客户端的只读视图。
//!
//! 保留牌的可见性取决于来源：从场上正面牌保留的牌，牌面在保留前已经公开，
//! 所有玩家都能看到；只有从牌堆顶保留的牌对对手保密。

use serde::{Deserialize, Serialize};

use crate::game::Game;
use crate::types::{Action, Card, Category, Event, Phase, PlayerId, TokenCounts};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct GameView {
    pub current_player: PlayerId,
    pub phase: Phase,
    pub turn_number: u32,
    pub supply: TokenCounts,
    pub board: BoardView,
    pub players: Vec<PlayerView>,
    pub end_triggered: bool,
    pub winners: Option<Vec<PlayerId>>,
    pub legal_actions: Vec<Action>,
    pub log: Vec<Event>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlayerView {
    pub id: PlayerId,
    pub name: String,
    pub tokens: TokenCounts,
    pub points: u16,
    pub bonuses: TokenCounts,
    pub captured: Vec<Card>,
    pub reserved_count: usize,
    /// 当前查看者可见的保留牌：自己的全部保留牌，以及对手从场上保留的牌。
    /// 对手没有可见保留牌时为 `None`。
    pub reserved: Option<Vec<Card>>,
    /// 当前查看者看不到的保留牌数量，即对手从牌堆顶保留的牌。
    #[serde(default)]
    pub hidden_reserved_count: usize,
    pub evolution_count: usize,
    pub card_count: usize,
    pub turns_taken: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BoardView {
    pub level1: Vec<Option<Card>>,
    pub level2: Vec<Option<Card>>,
    pub level3: Vec<Option<Card>>,
    pub rare: Vec<Option<Card>>,
    pub legendary: Vec<Option<Card>>,
}

impl BoardView {
    pub fn slots(&self, category: Category) -> &[Option<Card>] {
        match category {
            Category::Level1 => &self.level1,
            Category::Level2 => &self.level2,
            Category::Level3 => &self.level3,
            Category::Rare => &self.rare,
            Category::Legendary => &self.legendary,
        }
    }
}

impl Game {
    pub fn view_for(&self, viewer: PlayerId) -> GameView {
        let players = (0..self.players().len())
            .map(|id| {
                let player = &self.players()[id];
                let captured = player
                    .captured()
                    .iter()
                    .filter_map(|card_id| self.card(*card_id).cloned())
                    .collect();
                let visible_reserved: Vec<Card> = player
                    .reserved()
                    .iter()
                    .filter(|reserved| id == viewer || !reserved.hidden)
                    .filter_map(|reserved| self.card(reserved.card).cloned())
                    .collect();
                let hidden_reserved_count = if id == viewer {
                    0
                } else {
                    player
                        .reserved()
                        .iter()
                        .filter(|reserved| reserved.hidden)
                        .count()
                };
                let reserved = if id == viewer || !visible_reserved.is_empty() {
                    Some(visible_reserved)
                } else {
                    None
                };
                PlayerView {
                    id,
                    name: player.name().to_string(),
                    tokens: player.tokens(),
                    points: self.points_for(id),
                    bonuses: self.bonuses_for(id),
                    captured,
                    reserved_count: player.reserved().len(),
                    reserved,
                    hidden_reserved_count,
                    evolution_count: player.evolved_away().len(),
                    card_count: player.captured().len(),
                    turns_taken: player.turns_taken(),
                }
            })
            .collect();

        GameView {
            current_player: self.current_player(),
            phase: self.phase(),
            turn_number: self.turn_number(),
            supply: self.supply(),
            board: BoardView {
                level1: self.cards_on(Category::Level1),
                level2: self.cards_on(Category::Level2),
                level3: self.cards_on(Category::Level3),
                rare: self.cards_on(Category::Rare),
                legendary: self.cards_on(Category::Legendary),
            },
            players,
            end_triggered: self.end_triggered(),
            winners: self.winners().map(|winners| winners.to_vec()),
            legal_actions: if viewer == self.current_player() {
                self.legal_actions()
            } else {
                Vec::new()
            },
            log: self.log().to_vec(),
        }
    }

    fn cards_on(&self, category: Category) -> Vec<Option<Card>> {
        self.board()
            .slots(category)
            .iter()
            .map(|slot| slot.and_then(|card_id| self.card(card_id).cloned()))
            .collect()
    }
}
