//! 错误类型。错误消息直接面向 CLI / HTTP 客户端，使用中文描述。

use thiserror::Error;

use crate::types::Phase;

#[derive(Debug, Error)]
pub enum CatalogError {
    #[error("读取 cards.csv 失败: {0}")]
    Csv(#[from] csv::Error),

    #[error("cards.csv 缺少列 {0}")]
    MissingColumn(String),

    #[error("cards.csv 第 {line} 行的字段 {field} 值 {value:?} 无效")]
    InvalidField {
        line: usize,
        field: &'static str,
        value: String,
    },

    #[error("重复的卡牌 ID: {0}")]
    DuplicateId(u32),

    #[error("cards.csv 没有卡牌数据")]
    Empty,
}

#[derive(Debug, Error)]
pub enum GameError {
    #[error("玩家人数必须为 2–4，实际为 {0}")]
    InvalidPlayerCount(usize),

    #[error("卡牌数据错误: {0}")]
    Catalog(#[from] CatalogError),

    #[error("对局已结束")]
    GameOver,

    #[error("当前轮到玩家 {current}，不是玩家 {player}")]
    NotYourTurn {
        player: crate::types::PlayerId,
        current: crate::types::PlayerId,
    },

    #[error("阶段「{phase}」不允许执行「{action}」")]
    WrongPhase { phase: Phase, action: &'static str },

    #[error("非法动作: {0}")]
    InvalidAction(String),

    #[error("内部状态错误: {0}")]
    Internal(String),
}
