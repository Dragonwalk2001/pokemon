//! 面向客户端的只读视图。对手的保留牌只暴露数量，自己的保留牌才展示牌面。

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
    /// 对手只能看到保留牌数量。
    pub reserved_count: usize,
    /// 只有查看自己的视图时才为 `Some`。
    pub reserved: Option<Vec<Card>>,
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
                let reserved = if id == viewer {
                    Some(
                        player
                            .reserved()
                            .iter()
                            .filter_map(|card_id| self.card(*card_id).cloned())
                            .collect(),
                    )
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
