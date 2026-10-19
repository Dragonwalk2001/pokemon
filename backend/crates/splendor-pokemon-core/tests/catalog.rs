use std::collections::HashSet;

use splendor_pokemon_core::{Catalog, Category};

#[test]
fn builtin_catalog_matches_rulebook() {
    let catalog = Catalog::builtin().expect("builtin cards.csv should parse");
    assert_eq!(catalog.len(), 90);

    for (category, expected) in [
        (Category::Level1, 35),
        (Category::Level2, 30),
        (Category::Level3, 15),
        (Category::Rare, 5),
        (Category::Legendary, 5),
    ] {
        assert_eq!(
            catalog.by_category(category).count(),
            expected,
            "category {category:?}"
        );
    }

    let ids: HashSet<u32> = catalog.cards().iter().map(|card| card.id).collect();
    assert_eq!(ids.len(), 90);
}

#[test]
fn rare_and_legendary_have_double_bonus_and_no_evolution() {
    let catalog = Catalog::builtin().unwrap();
    for card in catalog
        .cards()
        .iter()
        .filter(|card| matches!(card.category, Category::Rare | Category::Legendary))
    {
        assert_eq!(card.bonus_count, 2, "{}", card.name_zh);
        assert!(card.evolves_to_zh.is_none(), "{}", card.name_zh);
        assert!(card.evolution_requirement.is_none(), "{}", card.name_zh);
        assert_eq!(card.cost.master, 1, "{}", card.name_zh);
    }
}

#[test]
fn level_cards_carry_evolution_data() {
    let catalog = Catalog::builtin().unwrap();

    for card in catalog
        .by_category(Category::Level1)
        .chain(catalog.by_category(Category::Level2))
    {
        assert!(card.evolves_to_zh.is_some(), "{}", card.name_zh);
        assert!(card.evolution_requirement.is_some(), "{}", card.name_zh);
    }

    for card in catalog.by_category(Category::Level3) {
        assert!(card.evolves_to_zh.is_none(), "{}", card.name_zh);
    }
}

#[test]
fn every_level_card_carries_full_evolution_chain() {
    let catalog = Catalog::builtin().unwrap();

    for card in catalog.cards() {
        if card.category.is_level() {
            let names: Vec<&str> = card
                .evolution_line
                .iter()
                .map(|step| step.name_zh.as_str())
                .collect();
            assert!(
                card.evolution_line.len() >= 2,
                "{} 缺少进化链",
                card.name_zh
            );
            assert!(
                names.contains(&card.name_zh.as_str()),
                "{} 不在自己的进化链 {:?} 里",
                card.name_zh,
                names
            );
            assert_eq!(
                card.evolution_line[0].category,
                Category::Level1,
                "{} 链首应为 1 阶",
                card.name_zh
            );
            assert_eq!(
                card.evolution_line.last().unwrap().category,
                Category::Level3,
                "{} 链尾应为 3 阶",
                card.name_zh
            );
            assert!(
                card.evolution_line.last().unwrap().requirement.is_none(),
                "{} 链尾不应再有进化门槛",
                card.name_zh
            );
        } else {
            assert!(
                card.evolution_line.is_empty(),
                "{} 不应有进化链",
                card.name_zh
            );
        }
    }

    let dratini = catalog
        .cards()
        .iter()
        .find(|card| card.name_zh == "迷你龙")
        .expect("迷你龙 in catalog");
    let names: Vec<&str> = dratini
        .evolution_line
        .iter()
        .map(|step| step.name_zh.as_str())
        .collect();
    assert_eq!(names, ["迷你龙", "哈克龙", "快龙"]);
    let requirement = dratini.evolution_line[0]
        .requirement
        .as_ref()
        .expect("迷你龙 has evolution requirement");
    assert_eq!(requirement.count, 3);
    assert_eq!(requirement.color, splendor_pokemon_core::Color::Blue);
}

#[test]
fn every_evolution_target_exists_in_next_level() {
    let catalog = Catalog::builtin().unwrap();
    for card in catalog.cards() {
        let Some(target_name) = &card.evolves_to_zh else {
            continue;
        };
        let next = card.category.next_level().expect("level card has next");
        assert!(
            catalog
                .by_category(next)
                .any(|target| &target.name_zh == target_name),
            "{} -> {} not found in {next:?}",
            card.name_zh,
            target_name
        );
    }
}
