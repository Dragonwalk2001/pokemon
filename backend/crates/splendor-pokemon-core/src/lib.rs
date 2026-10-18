//! 《璀璨宝石：宝可梦》规则引擎。
//!
//! - [`Catalog`]：解析 `data/cards.csv` 得到 90 张卡牌定义。
//! - [`Game`]：2–4 人权威对局状态机，覆盖拿球、保留、捕捉、弃球、进化与终局。
//! - [`GameView`]：按查看者过滤私有信息的只读快照，供 CLI / 后续 HTTP 层使用。
//!
//! 数据在编译期通过 `include_str!` 嵌入，运行时不依赖工作目录。

pub mod catalog;
pub mod error;
pub mod game;
pub mod types;
pub mod view;

pub use catalog::Catalog;
pub use error::{CatalogError, GameError};
pub use game::{
    Board, Decks, Game, BOARD_SLOTS, MAX_PLAYERS, MAX_RESERVED, MIN_PLAYERS, TOKEN_LIMIT,
    WIN_POINTS,
};
pub use types::{
    Action, Card, CardId, CardSource, Category, Color, Event, EvolutionRequirement, Phase, Player,
    PlayerId, TokenCounts,
};
pub use view::{BoardView, GameView, PlayerView};
