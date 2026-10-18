//! 卡牌目录：解析 `data/cards.csv`，提供按 ID / 类别的查询。

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::error::CatalogError;
use crate::types::{Card, Category, Color, EvolutionRequirement, TokenCounts};

/// 编译期嵌入规则数据，运行时不依赖工作目录。
pub const BUILTIN_CARDS_CSV: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../data/cards.csv"
));

/// 全部卡牌定义。构造后只读。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Catalog {
    cards: Vec<Card>,
    by_id: HashMap<u32, usize>,
}

impl Catalog {
    pub fn builtin() -> Result<Self, CatalogError> {
        Self::from_csv(BUILTIN_CARDS_CSV)
    }

    pub fn from_csv(input: &str) -> Result<Self, CatalogError> {
        let input = input.strip_prefix('\u{feff}').unwrap_or(input);
        let mut reader = csv::ReaderBuilder::new()
            .has_headers(true)
            .trim(csv::Trim::All)
            .from_reader(input.as_bytes());
        let headers = reader.headers().map_err(CatalogError::Csv)?.clone();

        let column = |name: &str| -> Result<usize, CatalogError> {
            headers
                .iter()
                .position(|header| header == name)
                .ok_or_else(|| CatalogError::MissingColumn(name.to_string()))
        };

        let id_col = column("card_id")?;
        let category_col = column("category")?;
        let name_col = column("name_zh")?;
        let points_col = column("points")?;
        let bonus_color_col = column("bonus_color")?;
        let bonus_count_col = column("bonus_count")?;
        let cost_red_col = column("cost_red")?;
        let cost_blue_col = column("cost_blue")?;
        let cost_yellow_col = column("cost_yellow")?;
        let cost_pink_col = column("cost_pink")?;
        let cost_black_col = column("cost_black")?;
        let cost_master_col = column("cost_master")?;
        let evolves_to_col = column("evolves_to_zh")?;
        let evolution_count_col = column("evolution_requirement_count")?;
        let evolution_color_col = column("evolution_requirement_color")?;

        let mut cards = Vec::new();
        let mut by_id = HashMap::new();

        for (offset, record) in reader.records().enumerate() {
            let line = offset + 2;
            let record = record.map_err(CatalogError::Csv)?;
            let get = |index: usize| record.get(index).unwrap_or("");

            let card_id = parse_u32(get(id_col), line, "card_id")?;
            let category =
                Category::parse(get(category_col)).ok_or_else(|| CatalogError::InvalidField {
                    line,
                    field: "category",
                    value: get(category_col).to_string(),
                })?;
            let name_zh = get(name_col).trim();
            if name_zh.is_empty() {
                return Err(CatalogError::InvalidField {
                    line,
                    field: "name_zh",
                    value: String::new(),
                });
            }
            let bonus_color =
                Color::parse(get(bonus_color_col)).ok_or_else(|| CatalogError::InvalidField {
                    line,
                    field: "bonus_color",
                    value: get(bonus_color_col).to_string(),
                })?;
            let evolves_to_raw = get(evolves_to_col);
            let evolves_to_zh = if evolves_to_raw.is_empty() {
                None
            } else {
                Some(evolves_to_raw.to_string())
            };
            let req_count_raw = get(evolution_count_col);
            let req_color_raw = get(evolution_color_col);
            let evolution_requirement = match (req_count_raw.is_empty(), req_color_raw.is_empty()) {
                (true, true) => None,
                (false, false) => Some(EvolutionRequirement {
                    color: Color::parse(req_color_raw).ok_or_else(|| {
                        CatalogError::InvalidField {
                            line,
                            field: "evolution_requirement_color",
                            value: req_color_raw.to_string(),
                        }
                    })?,
                    count: parse_u8(req_count_raw, line, "evolution_requirement_count")?,
                }),
                _ => {
                    return Err(CatalogError::InvalidField {
                        line,
                        field: "evolution_requirement",
                        value: format!("{req_count_raw}/{req_color_raw}"),
                    })
                }
            };

            let card = Card {
                id: card_id,
                category,
                name_zh: name_zh.to_string(),
                points: parse_u8(get(points_col), line, "points")?,
                bonus_color,
                bonus_count: parse_u8(get(bonus_count_col), line, "bonus_count")?,
                cost: TokenCounts::new(
                    parse_u8(get(cost_red_col), line, "cost_red")?,
                    parse_u8(get(cost_blue_col), line, "cost_blue")?,
                    parse_u8(get(cost_yellow_col), line, "cost_yellow")?,
                    parse_u8(get(cost_pink_col), line, "cost_pink")?,
                    parse_u8(get(cost_black_col), line, "cost_black")?,
                    parse_u8(get(cost_master_col), line, "cost_master")?,
                ),
                evolves_to_zh,
                evolution_requirement,
            };

            if by_id.insert(card_id, cards.len()).is_some() {
                return Err(CatalogError::DuplicateId(card_id));
            }
            cards.push(card);
        }

        if cards.is_empty() {
            return Err(CatalogError::Empty);
        }

        Ok(Self { cards, by_id })
    }

    pub fn cards(&self) -> &[Card] {
        &self.cards
    }

    pub fn get(&self, id: u32) -> Option<&Card> {
        self.by_id.get(&id).map(|&index| &self.cards[index])
    }

    pub fn by_category(&self, category: Category) -> impl Iterator<Item = &Card> {
        self.cards
            .iter()
            .filter(move |card| card.category == category)
    }

    pub fn len(&self) -> usize {
        self.cards.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cards.is_empty()
    }
}

fn parse_u8(value: &str, line: usize, field: &'static str) -> Result<u8, CatalogError> {
    value.parse::<u8>().map_err(|_| CatalogError::InvalidField {
        line,
        field,
        value: value.to_string(),
    })
}

fn parse_u32(value: &str, line: usize, field: &'static str) -> Result<u32, CatalogError> {
    value
        .parse::<u32>()
        .map_err(|_| CatalogError::InvalidField {
            line,
            field,
            value: value.to_string(),
        })
}
