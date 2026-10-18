use splendor_pokemon_core::{Action, CardSource, Category, Color, Game, Phase};

fn new_game(players: usize, seed: u64) -> Game {
    let names = (1..=players).map(|index| format!("P{index}")).collect();
    Game::new(names, seed).expect("game should initialise")
}

#[test]
fn setup_matches_player_count() {
    for (players, per_color) in [(2, 4), (3, 5), (4, 7)] {
        let game = new_game(players, 42);
        let supply = game.supply();
        for color in Color::ALL {
            assert_eq!(supply.get(color), per_color, "{players} players");
        }
        assert_eq!(supply.master, 5);

        for category in Category::LEVELS {
            assert_eq!(game.board().slots(category).len(), 4);
            assert_eq!(
                game.board()
                    .slots(category)
                    .iter()
                    .filter(|slot| slot.is_some())
                    .count(),
                4
            );
        }
        for category in [Category::Rare, Category::Legendary] {
            assert_eq!(
                game.board()
                    .slots(category)
                    .iter()
                    .filter(|slot| slot.is_some())
                    .count(),
                1
            );
        }

        assert_eq!(game.decks().len(Category::Level1), 31);
        assert_eq!(game.decks().len(Category::Level2), 26);
        assert_eq!(game.decks().len(Category::Level3), 11);
        assert_eq!(game.decks().len(Category::Rare), 4);
        assert_eq!(game.decks().len(Category::Legendary), 4);

        assert_eq!(game.phase(), Phase::MainAction);
        assert_eq!(game.current_player(), 0);
    }
}

#[test]
fn same_seed_produces_same_board() {
    let first = new_game(2, 123);
    let second = new_game(2, 123);
    for category in Category::ALL {
        assert_eq!(
            first.board().slots(category),
            second.board().slots(category)
        );
    }
}

#[test]
fn take_three_then_end_turn() {
    let mut game = new_game(3, 7);
    game.apply(
        0,
        Action::TakeThree {
            colors: vec![Color::Red, Color::Blue, Color::Yellow],
        },
    )
    .unwrap();

    assert_eq!(game.players()[0].tokens().red, 1);
    assert_eq!(game.players()[0].tokens().blue, 1);
    assert_eq!(game.players()[0].tokens().yellow, 1);
    assert_eq!(game.supply().red, 4);
    assert_eq!(game.phase(), Phase::Evolution);

    game.apply(0, Action::EndTurn).unwrap();
    assert_eq!(game.current_player(), 1);
    assert_eq!(game.phase(), Phase::MainAction);
}

#[test]
fn only_current_player_can_act() {
    let mut game = new_game(2, 7);
    let result = game.apply(1, Action::TakeTwo { color: Color::Red });
    assert!(result.is_err());
    assert_eq!(game.supply().red, 4);
}

#[test]
fn take_two_requires_four_in_supply() {
    let mut game = new_game(2, 7);
    game.apply(0, Action::TakeTwo { color: Color::Red })
        .unwrap();
    game.apply(0, Action::EndTurn).unwrap();

    let result = game.apply(1, Action::TakeTwo { color: Color::Red });
    assert!(result.is_err());
}

#[test]
fn opponent_reserved_cards_are_hidden() {
    let mut game = new_game(2, 7);
    game.apply(
        0,
        Action::Reserve {
            source: CardSource::deck_top(Category::Level1),
        },
    )
    .unwrap();
    game.apply(0, Action::EndTurn).unwrap();

    let opponent_view = game.view_for(1);
    assert!(opponent_view.players[0].reserved.is_none());
    assert_eq!(opponent_view.players[0].reserved_count, 1);

    let own_view = game.view_for(0);
    assert_eq!(
        own_view.players[0]
            .reserved
            .as_ref()
            .expect("own reserved cards are visible")
            .len(),
        1
    );
}

#[test]
fn legal_actions_include_take_three_after_setup() {
    let game = new_game(2, 7);
    let actions = game.legal_actions();
    assert!(actions
        .iter()
        .any(|action| matches!(action, Action::TakeThree { .. })));
    assert!(actions
        .iter()
        .any(|action| matches!(action, Action::TakeTwo { .. })));
}
