//! 回放时间线：每次动作后保存 `(Action, GameView)` 快照，可直接序列化为 JSON。
//!
//! 快照的 `view.log` 只保留本步新增事件，避免整局日志在每一帧重复导致文件膨胀；
//! 完整事件序列由各帧的 `events` 文本按时间拼接得到。其余字段是完整的 `GameView`。

use serde::{Deserialize, Serialize};

use crate::game::Game;
use crate::types::{Action, PlayerId};
use crate::view::GameView;

pub const REPLAY_VERSION: u32 = 1;

/// 整个回放文件。`frames[0]` 是开局快照，后续每帧对应一个动作。
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Replay {
    pub version: u32,
    pub seed: u64,
    pub players: Vec<String>,
    /// 与 `players` 一一对应：`human` / `random` / `heuristic` / `heuristic-v1`。
    pub controllers: Vec<String>,
    pub frames: Vec<ReplayFrame>,
    pub result: Option<ReplayResult>,
    /// 规则层僵局：供应区耗尽且当前玩家没有任何合法主行动。此时没有胜者。
    #[serde(default)]
    pub stalemate: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReplayFrame {
    /// 执行动作的玩家；开局帧为 `None`。
    pub actor: Option<PlayerId>,
    pub action: Option<Action>,
    /// 动作的中文文本，方便不依赖 Action 结构的前端直接展示。
    pub action_text: Option<String>,
    /// 本步新增事件的文本描述。
    pub events: Vec<String>,
    /// 动作之后的只读视图（`log` 仅含本步事件）。
    pub view: GameView,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReplayResult {
    pub winners: Vec<PlayerId>,
    pub points: Vec<u16>,
    pub turns: u32,
}

impl Replay {
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    pub fn from_json(input: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(input)
    }
}

/// 在游戏进行过程中逐步攒出 [`Replay`]。
///
/// 调用顺序必须是：`new` → 每执行一个动作 `record` → 对局结束后 `finish`。
pub struct ReplayRecorder {
    seed: u64,
    players: Vec<String>,
    controllers: Vec<String>,
    frames: Vec<ReplayFrame>,
    logged_events: usize,
    stalemate: bool,
}

impl ReplayRecorder {
    pub fn new(game: &Game, controllers: Vec<String>) -> Self {
        let mut recorder = Self {
            seed: game.seed(),
            players: game
                .players()
                .iter()
                .map(|player| player.name().to_string())
                .collect(),
            controllers,
            frames: Vec::new(),
            logged_events: 0,
            stalemate: false,
        };
        recorder.push_frame(game, None, None);
        recorder
    }

    pub fn record(&mut self, game: &Game, actor: PlayerId, action: &Action) {
        self.push_frame(game, Some(actor), Some(action.clone()));
    }

    pub fn finish(self, game: &Game) -> Replay {
        self.finish_with(game, false)
    }

    /// 以「规则僵局」收尾：没有胜者，但回放仍完整保留到最后一个合法动作。
    pub fn finish_stalemate(self, game: &Game) -> Replay {
        self.finish_with(game, true)
    }

    fn finish_with(mut self, game: &Game, stalemate: bool) -> Replay {
        self.stalemate |= stalemate;
        let result = if self.stalemate {
            None
        } else {
            game.winners().map(|winners| ReplayResult {
                winners: winners.to_vec(),
                points: (0..game.players().len())
                    .map(|player| game.points_for(player))
                    .collect(),
                turns: game.turn_number(),
            })
        };
        Replay {
            version: REPLAY_VERSION,
            seed: self.seed,
            players: self.players,
            controllers: self.controllers,
            frames: self.frames,
            result,
            stalemate: self.stalemate,
        }
    }

    fn push_frame(&mut self, game: &Game, actor: Option<PlayerId>, action: Option<Action>) {
        let viewer = actor.unwrap_or_else(|| game.current_player());
        let mut view = game.view_for(viewer);
        let new_log = &game.log()[self.logged_events..];
        let events: Vec<String> = new_log.iter().map(|event| event.describe(game)).collect();
        view.log = new_log.to_vec();
        self.logged_events = game.log().len();

        self.frames.push(ReplayFrame {
            actor,
            action_text: action.as_ref().map(ToString::to_string),
            action,
            events,
            view,
        });
    }
}
