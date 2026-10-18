//! bot / 回放集成测试：可复现性、批量稳定性与 PvE 完成度。

use splendor_pokemon_core::bot::play_bot_game;
use splendor_pokemon_core::{BotKind, Replay};

fn names(count: usize) -> Vec<String> {
    (1..=count).map(|index| format!("P{index}")).collect()
}

fn kinds(count: usize, seed: u64) -> Vec<BotKind> {
    (0..count)
        .map(|player| {
            if (seed as usize + player) % 3 == 0 {
                BotKind::Random
            } else {
                BotKind::Heuristic
            }
        })
        .collect()
}

#[test]
fn same_seed_produces_identical_replay() {
    let styles = [BotKind::Heuristic, BotKind::Random, BotKind::Heuristic];
    let first = play_bot_game(names(3), 20241018, &styles).expect("first game");
    let second = play_bot_game(names(3), 20241018, &styles).expect("second game");

    assert_eq!(
        first.to_json().expect("serialise first"),
        second.to_json().expect("serialise second"),
        "同一 seed 的回放必须逐字节一致"
    );
    assert!(first.frames.len() > 1);
}

#[test]
fn different_seed_produces_different_replay() {
    let styles = [BotKind::Heuristic, BotKind::Heuristic];
    let first = play_bot_game(names(2), 1, &styles).expect("first game");
    let second = play_bot_game(names(2), 2, &styles).expect("second game");
    assert_ne!(
        first.to_json().expect("serialise first"),
        second.to_json().expect("serialise second")
    );
}

#[test]
fn hundred_mixed_games_terminate_without_panic() {
    let mut total_frames = 0usize;
    for seed in 0..100u64 {
        let players = 2 + (seed as usize % 3);
        let styles = kinds(players, seed);
        let replay = play_bot_game(names(players), seed, &styles)
            .unwrap_or_else(|error| panic!("seed {seed} / {players} players failed: {error}"));

        assert!(
            replay.result.is_some() || replay.stalemate,
            "seed {seed} neither finished nor stalemated"
        );
        if let Some(result) = replay.result.as_ref() {
            assert_eq!(result.points.len(), players);
            assert!(!result.winners.is_empty());
            assert_eq!(
                replay.frames.last().unwrap().view.phase,
                splendor_pokemon_core::Phase::GameOver
            );
        }
        assert_eq!(replay.frames[0].action, None);
        assert!(replay.frames.len() > 2);
        assert!(replay.frames[1..]
            .iter()
            .all(|frame| frame.action.is_some()));
        total_frames += replay.frames.len();
    }
    assert!(total_frames > 0);
}

#[test]
fn hundred_random_games_terminate_without_panic() {
    for seed in 0..100u64 {
        let players = 2 + (seed as usize % 3);
        let styles = vec![BotKind::Random; players];
        let replay = play_bot_game(names(players), seed, &styles)
            .unwrap_or_else(|error| panic!("random seed {seed} failed: {error}"));
        assert!(
            replay.result.is_some() || replay.stalemate,
            "random seed {seed} neither finished nor stalemated"
        );
    }
}

#[test]
fn hundred_heuristic_games_complete() {
    for seed in 0..100u64 {
        let players = 2 + (seed as usize % 3);
        let styles = vec![BotKind::Heuristic; players];
        let replay = play_bot_game(names(players), seed, &styles)
            .unwrap_or_else(|error| panic!("heuristic seed {seed} failed: {error}"));
        assert!(
            replay.result.is_some(),
            "heuristic seed {seed} ended in stalemate"
        );
    }
}

#[test]
fn heuristic_v1_completes_protocol_seeds() {
    // 回测基线在官方 20 个种子上必须能正常终局。
    for seed in 1..=20u64 {
        let replay = play_bot_game(
            names(2),
            seed,
            &[BotKind::HeuristicV1, BotKind::HeuristicV1],
        )
        .unwrap_or_else(|error| panic!("baseline seed {seed} failed: {error}"));
        assert!(replay.result.is_some(), "baseline seed {seed} must finish");
    }
}

#[test]
fn bot_kind_parses_versions() {
    assert_eq!(BotKind::parse("heuristic"), Some(BotKind::Heuristic));
    assert_eq!(BotKind::parse("heuristic-v1"), Some(BotKind::HeuristicV1));
    assert_eq!(BotKind::parse("v1"), Some(BotKind::HeuristicV1));
    assert_eq!(BotKind::parse("random"), Some(BotKind::Random));
    assert_eq!(BotKind::parse("unknown"), None);
}

#[test]
fn pve_style_game_completes() {
    // 玩家 0 用启发式代替人类席位，后两名是 bot；与 CLI `pve --auto` 走同一条 runner。
    let replay = play_bot_game(
        vec!["你".into(), "Bot1".into(), "Bot2".into()],
        7,
        &[BotKind::Heuristic, BotKind::Random, BotKind::Heuristic],
    )
    .expect("pve game should finish");

    let result = replay.result.expect("pve game must have a result");
    assert_eq!(result.points.len(), 3);
    assert!(result.winners.iter().all(|&winner| winner < 3));
}

#[test]
fn replay_json_round_trips() {
    let replay = play_bot_game(names(2), 99, &[BotKind::Heuristic, BotKind::Random])
        .expect("game should finish");
    let json = replay.to_json().expect("serialise");
    let loaded = Replay::from_json(&json).expect("deserialise");

    assert_eq!(loaded.version, replay.version);
    assert_eq!(loaded.seed, replay.seed);
    assert_eq!(loaded.players, replay.players);
    assert_eq!(loaded.controllers, replay.controllers);
    assert_eq!(loaded.frames.len(), replay.frames.len());
    assert_eq!(
        loaded.result.as_ref().map(|result| result.winners.clone()),
        replay.result.as_ref().map(|result| result.winners.clone())
    );
    // 每帧只保留本步事件，回放文件不随动作数平方增长。
    let last = loaded.frames.last().expect("last frame");
    assert!(last.view.log.len() <= last.events.len());
}
