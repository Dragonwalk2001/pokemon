//! 领域类型：颜色、资源、卡牌、动作与事件。
//!
//! 所有类型都可序列化，便于后续 HTTP / WebSocket 层直接复用。

use std::fmt;

use serde::{Deserialize, Serialize};

/// 普通精灵球的五种颜色。大师球不是颜色，单独在 [`TokenCounts::master`] 中计数。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Color {
    Red,
    Blue,
    Yellow,
    Pink,
    Black,
}

impl Color {
    pub const ALL: [Color; 5] = [
        Color::Red,
        Color::Blue,
        Color::Yellow,
        Color::Pink,
        Color::Black,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Color::Red => "red",
            Color::Blue => "blue",
            Color::Yellow => "yellow",
            Color::Pink => "pink",
            Color::Black => "black",
        }
    }

    pub const fn label_zh(self) -> &'static str {
        match self {
            Color::Red => "红",
            Color::Blue => "蓝",
            Color::Yellow => "黄",
            Color::Pink => "粉",
            Color::Black => "黑",
        }
    }

    pub fn parse(input: &str) -> Option<Self> {
        match input.trim().to_ascii_lowercase().as_str() {
            "r" | "red" | "红" | "红色" => Some(Color::Red),
            "b" | "blue" | "蓝" | "蓝色" => Some(Color::Blue),
            "y" | "yellow" | "黄" | "黄色" => Some(Color::Yellow),
            "p" | "pink" | "粉" | "粉色" => Some(Color::Pink),
            "k" | "black" | "黑" | "黑色" => Some(Color::Black),
            _ => None,
        }
    }

    pub(crate) const fn index(self) -> usize {
        match self {
            Color::Red => 0,
            Color::Blue => 1,
            Color::Yellow => 2,
            Color::Pink => 3,
            Color::Black => 4,
        }
    }
}

impl fmt::Display for Color {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label_zh())
    }
}

/// 一组精灵球数量。同时用于玩家持有量、供应区、卡牌费用与支付额。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenCounts {
    pub red: u8,
    pub blue: u8,
    pub yellow: u8,
    pub pink: u8,
    pub black: u8,
    pub master: u8,
}

impl TokenCounts {
    pub const ZERO: Self = Self {
        red: 0,
        blue: 0,
        yellow: 0,
        pink: 0,
        black: 0,
        master: 0,
    };

    pub const fn new(red: u8, blue: u8, yellow: u8, pink: u8, black: u8, master: u8) -> Self {
        Self {
            red,
            blue,
            yellow,
            pink,
            black,
            master,
        }
    }

    pub const fn get(self, color: Color) -> u8 {
        match color {
            Color::Red => self.red,
            Color::Blue => self.blue,
            Color::Yellow => self.yellow,
            Color::Pink => self.pink,
            Color::Black => self.black,
        }
    }

    pub fn get_mut(&mut self, color: Color) -> &mut u8 {
        match color {
            Color::Red => &mut self.red,
            Color::Blue => &mut self.blue,
            Color::Yellow => &mut self.yellow,
            Color::Pink => &mut self.pink,
            Color::Black => &mut self.black,
        }
    }

    pub fn set(&mut self, color: Color, value: u8) {
        *self.get_mut(color) = value;
    }

    pub const fn total(self) -> u16 {
        self.red as u16
            + self.blue as u16
            + self.yellow as u16
            + self.pink as u16
            + self.black as u16
            + self.master as u16
    }

    pub const fn is_zero(self) -> bool {
        self.total() == 0
    }

    pub fn colors(self) -> [(Color, u8); 5] {
        [
            (Color::Red, self.red),
            (Color::Blue, self.blue),
            (Color::Yellow, self.yellow),
            (Color::Pink, self.pink),
            (Color::Black, self.black),
        ]
    }

    /// 逐项相加；溢出时返回 `None`。
    pub fn checked_add(&mut self, other: Self) -> Option<()> {
        self.red = self.red.checked_add(other.red)?;
        self.blue = self.blue.checked_add(other.blue)?;
        self.yellow = self.yellow.checked_add(other.yellow)?;
        self.pink = self.pink.checked_add(other.pink)?;
        self.black = self.black.checked_add(other.black)?;
        self.master = self.master.checked_add(other.master)?;
        Some(())
    }

    /// 逐项相减；任一颜色不足时返回 `None`。
    pub fn checked_sub(&mut self, other: Self) -> Option<()> {
        self.red = self.red.checked_sub(other.red)?;
        self.blue = self.blue.checked_sub(other.blue)?;
        self.yellow = self.yellow.checked_sub(other.yellow)?;
        self.pink = self.pink.checked_sub(other.pink)?;
        self.black = self.black.checked_sub(other.black)?;
        self.master = self.master.checked_sub(other.master)?;
        Some(())
    }
}

impl fmt::Display for TokenCounts {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut parts = Vec::new();
        for (color, count) in self.colors() {
            if count > 0 {
                parts.push(format!("{}{}", color.label_zh(), count));
            }
        }
        if self.master > 0 {
            parts.push(format!("大师{}", self.master));
        }
        if parts.is_empty() {
            f.write_str("无")
        } else {
            f.write_str(&parts.join(" "))
        }
    }
}

/// 卡牌类别。1–3 阶为普通牌；稀有与传说牌不能进化、不能保留。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Level1,
    Level2,
    Level3,
    Rare,
    Legendary,
}

impl Category {
    pub const LEVELS: [Category; 3] = [Category::Level1, Category::Level2, Category::Level3];

    pub const ALL: [Category; 5] = [
        Category::Level1,
        Category::Level2,
        Category::Level3,
        Category::Rare,
        Category::Legendary,
    ];

    pub const fn label_zh(self) -> &'static str {
        match self {
            Category::Level1 => "1阶",
            Category::Level2 => "2阶",
            Category::Level3 => "3阶",
            Category::Rare => "稀有",
            Category::Legendary => "传说",
        }
    }

    pub fn parse(input: &str) -> Option<Self> {
        match input.trim().to_ascii_lowercase().as_str() {
            "1" | "l1" | "level1" | "level_1" | "一阶" | "1阶" => Some(Category::Level1),
            "2" | "l2" | "level2" | "level_2" | "二阶" | "2阶" => Some(Category::Level2),
            "3" | "l3" | "level3" | "level_3" | "三阶" | "3阶" => Some(Category::Level3),
            "rare" | "稀有" => Some(Category::Rare),
            "legendary" | "传说" => Some(Category::Legendary),
            _ => None,
        }
    }

    pub const fn is_level(self) -> bool {
        matches!(self, Category::Level1 | Category::Level2 | Category::Level3)
    }

    pub const fn next_level(self) -> Option<Category> {
        match self {
            Category::Level1 => Some(Category::Level2),
            Category::Level2 => Some(Category::Level3),
            Category::Level3 | Category::Rare | Category::Legendary => None,
        }
    }
}

impl fmt::Display for Category {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.label_zh())
    }
}

/// 进化门槛：指定颜色的永久减免达到 `count` 个。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct EvolutionRequirement {
    pub color: Color,
    pub count: u8,
}

impl fmt::Display for EvolutionRequirement {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}{}", self.count, self.color.label_zh())
    }
}

/// 卡牌定义，来自 `data/cards.csv`。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Card {
    pub id: u32,
    pub category: Category,
    pub name_zh: String,
    pub points: u8,
    pub bonus_color: Color,
    pub bonus_count: u8,
    pub cost: TokenCounts,
    pub evolves_to_zh: Option<String>,
    pub evolution_requirement: Option<EvolutionRequirement>,
}

impl Card {
    pub fn can_be_reserved(&self) -> bool {
        self.category.is_level()
    }
}

pub type PlayerId = usize;
pub type CardId = u32;

/// 一名玩家。字段私有，保证规则引擎之外不能随意破坏状态。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Player {
    pub(crate) name: String,
    pub(crate) tokens: TokenCounts,
    pub(crate) captured: Vec<CardId>,
    pub(crate) reserved: Vec<CardId>,
    pub(crate) evolved_away: Vec<CardId>,
    pub(crate) turns_taken: u32,
}

impl Player {
    pub(crate) fn new(name: String) -> Self {
        Self {
            name,
            tokens: TokenCounts::ZERO,
            captured: Vec::new(),
            reserved: Vec::new(),
            evolved_away: Vec::new(),
            turns_taken: 0,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn tokens(&self) -> TokenCounts {
        self.tokens
    }

    pub fn captured(&self) -> &[CardId] {
        &self.captured
    }

    pub fn reserved(&self) -> &[CardId] {
        &self.reserved
    }

    pub fn evolved_away(&self) -> &[CardId] {
        &self.evolved_away
    }

    pub fn turns_taken(&self) -> u32 {
        self.turns_taken
    }
}

/// 动作作用的卡牌来源。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CardSource {
    /// 场上正面牌：`category` + 该行的槽位下标。
    Board { category: Category, index: usize },
    /// 自己手牌中的保留牌。
    Reserved { index: usize },
    /// 牌堆顶（仅用于保留，牌面隐藏）。
    DeckTop { category: Category },
}

impl CardSource {
    pub fn board(category: Category, index: usize) -> Self {
        Self::Board { category, index }
    }

    pub fn reserved(index: usize) -> Self {
        Self::Reserved { index }
    }

    pub fn deck_top(category: Category) -> Self {
        Self::DeckTop { category }
    }
}

impl fmt::Display for CardSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CardSource::Board { category, index } => {
                write!(f, "场上{}#{}", category.label_zh(), index)
            }
            CardSource::Reserved { index } => write!(f, "保留#{}", index),
            CardSource::DeckTop { category } => {
                write!(f, "{}牌堆顶", category.label_zh())
            }
        }
    }
}

/// 一个完整动作。主行动、弃球、进化与结束回合都在这里表达。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Action {
    TakeThree {
        colors: Vec<Color>,
    },
    TakeTwo {
        color: Color,
    },
    Reserve {
        source: CardSource,
    },
    Capture {
        source: CardSource,
    },
    Discard {
        tokens: TokenCounts,
    },
    Evolve {
        captured_index: usize,
        target: CardSource,
    },
    EndTurn,
}

impl Action {
    pub fn name(&self) -> &'static str {
        match self {
            Action::TakeThree { .. } => "拿取三种球",
            Action::TakeTwo { .. } => "拿取两个同色球",
            Action::Reserve { .. } => "保留卡牌",
            Action::Capture { .. } => "捕捉宝可梦",
            Action::Discard { .. } => "弃球",
            Action::Evolve { .. } => "进化",
            Action::EndTurn => "结束回合",
        }
    }
}

impl fmt::Display for Action {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Action::TakeThree { colors } => {
                let names: Vec<&str> = colors.iter().map(|c| c.label_zh()).collect();
                write!(f, "拿3色[{}]", names.join(" "))
            }
            Action::TakeTwo { color } => write!(f, "拿2{}", color.label_zh()),
            Action::Reserve { source } => write!(f, "保留 {source}"),
            Action::Capture { source } => write!(f, "捕捉 {source}"),
            Action::Discard { tokens } => write!(f, "弃球 {tokens}"),
            Action::Evolve {
                captured_index,
                target,
            } => write!(f, "进化 捕捉#{captured_index} → {target}"),
            Action::EndTurn => f.write_str("结束回合"),
        }
    }
}

/// 回合内阶段。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    /// 必须执行一个主要行动。
    MainAction,
    /// 回合末球数超过 10，必须先弃球。
    Discard,
    /// 可以进化一次，或直接结束回合。
    Evolution,
    /// 对局结束。
    GameOver,
}

impl fmt::Display for Phase {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Phase::MainAction => "主要行动",
            Phase::Discard => "弃球",
            Phase::Evolution => "进化/结束回合",
            Phase::GameOver => "终局",
        })
    }
}

/// 对局事件。保留牌从牌堆顶拿取时不暴露卡牌 ID。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Event {
    GameStarted {
        players: usize,
        seed: u64,
    },
    TurnStarted {
        player: PlayerId,
        turn: u32,
    },
    BallsTaken {
        player: PlayerId,
        tokens: TokenCounts,
    },
    CardReserved {
        player: PlayerId,
        from_deck: bool,
        card: Option<CardId>,
        got_master: bool,
    },
    CardCaptured {
        player: PlayerId,
        card: CardId,
        payment: TokenCounts,
    },
    BallsDiscarded {
        player: PlayerId,
        tokens: TokenCounts,
    },
    PokemonEvolved {
        player: PlayerId,
        from: CardId,
        to: CardId,
    },
    TurnEnded {
        player: PlayerId,
    },
    FinalRoundTriggered {
        player: PlayerId,
        points: u16,
    },
    GameEnded {
        winners: Vec<PlayerId>,
        points: Vec<u16>,
    },
}
