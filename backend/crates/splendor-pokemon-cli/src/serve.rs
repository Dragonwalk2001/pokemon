//! 本地 JSON HTTP 服务：供 Godot 交互式 PvE 界面调用。
//!
//! 只监听 `127.0.0.1`，每次请求一个响应后关闭连接，不依赖额外 crate。
//! 所有规则判定仍走 `Game::apply`；bot 只拿到自己的 `GameView`。
//! 人类固定是玩家 0，服务在人类动作后连续推进 bot 回合，直到再次轮到人类、
//! 对局结束或出现规则僵局。

use std::io::{self, Read, Write};
use std::net::{Ipv4Addr, TcpListener, TcpStream};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use splendor_pokemon_core::{
    make_bot, Action, Bot, BotKind, Game, GameView, Phase, PlayerId, ReplayRecorder,
};

const DEFAULT_PORT: u16 = 43110;
const HUMAN: PlayerId = 0;
const MAX_REQUEST_BYTES: usize = 1 << 20;
const MAX_BOT_ACTIONS_PER_CALL: usize = 5_000;

pub(crate) fn run(args: &[String]) -> Result<(), String> {
    let Some(config) = Config::from_args(args)? else {
        return Ok(());
    };
    serve(config)
}

fn serve(config: Config) -> Result<(), String> {
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, config.port))
        .map_err(|error| format!("无法监听 127.0.0.1:{}: {error}", config.port))?;
    listener
        .set_nonblocking(true)
        .map_err(|error| format!("无法设置非阻塞监听: {error}"))?;
    let address = listener
        .local_addr()
        .map_err(|error| format!("无法读取监听地址: {error}"))?;

    let mut session = Session::new(config.seed, config.bots, config.bot, config.name)?;
    println!("璀璨宝石：宝可梦 — 本地对战引擎");
    println!("监听地址：http://{address}");
    println!("Godot 的「和 AI 对战」会自动连接该地址；按 Ctrl+C 结束。");
    if config.idle_timeout > 0 {
        println!("空闲 {} 秒后自动退出。", config.idle_timeout);
    }
    io::stdout().flush().ok();

    let mut last_activity = Instant::now();
    loop {
        if config.idle_timeout > 0
            && last_activity.elapsed() >= Duration::from_secs(config.idle_timeout)
        {
            println!("空闲超时，服务退出。");
            return Ok(());
        }
        match listener.accept() {
            Ok((mut stream, _peer)) => {
                last_activity = Instant::now();
                if let Err(error) = handle_connection(&mut session, &mut stream) {
                    eprintln!("请求处理失败: {error}");
                }
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(40));
            }
            Err(error) => return Err(format!("接受连接失败: {error}")),
        }
    }
}

struct Session {
    game: Game,
    bots: Vec<Option<Box<dyn Bot>>>,
    recorder: ReplayRecorder,
    controllers: Vec<String>,
    seed: u64,
    bots_count: usize,
    bot_kind: BotKind,
    stalemate: bool,
}

impl Session {
    fn new(seed: u64, bots_count: usize, bot_kind: BotKind, name: String) -> Result<Self, String> {
        if !(2..=3).contains(&bots_count) {
            return Err(format!("bots 必须为 2 或 3，当前为 {bots_count}"));
        }
        let players = bots_count + 1;
        let names: Vec<String> = std::iter::once(name)
            .chain((1..=bots_count).map(|index| format!("Bot{index}")))
            .collect();
        let game = Game::new(names, seed).map_err(|error| format!("无法创建对局: {error}"))?;
        let bots: Vec<Option<Box<dyn Bot>>> = (0..players)
            .map(|player| {
                if player == HUMAN {
                    None
                } else {
                    Some(make_bot(bot_kind, player, seed))
                }
            })
            .collect();
        let controllers: Vec<String> = std::iter::once("human".to_string())
            .chain((1..players).map(|_| bot_kind.as_str().to_string()))
            .collect();
        let recorder = ReplayRecorder::new(&game, controllers.clone());
        Ok(Self {
            game,
            bots,
            recorder,
            controllers,
            seed,
            bots_count,
            bot_kind,
            stalemate: false,
        })
    }

    fn apply_human_action(&mut self, action: Action) -> Result<Vec<Step>, String> {
        if self.game.is_over() {
            return Err("对局已结束".into());
        }
        if self.stalemate {
            return Err("对局已僵局".into());
        }
        if self.game.current_player() != HUMAN {
            return Err("当前不是你的回合".into());
        }

        let mut steps = vec![self.apply_and_record(HUMAN, action)?];
        self.run_bots(&mut steps)?;
        self.check_stalemate();
        Ok(steps)
    }

    fn check_stalemate(&mut self) {
        if !self.game.is_over()
            && !self.stalemate
            && self.game.phase() == Phase::MainAction
            && self.game.legal_actions().is_empty()
        {
            self.stalemate = true;
        }
    }

    fn apply_and_record(&mut self, actor: PlayerId, action: Action) -> Result<Step, String> {
        let before = self.game.log().len();
        let action_text = action.to_string();
        self.game
            .apply(actor, action.clone())
            .map_err(|error| format!("动作不合法: {error}"))?;
        self.recorder.record(&self.game, actor, &action);

        let new_log = &self.game.log()[before..];
        let events: Vec<String> = new_log
            .iter()
            .map(|event| event.describe(&self.game))
            .collect();
        let mut view = self.game.view_for(HUMAN);
        view.log = new_log.to_vec();
        Ok(Step {
            actor,
            action,
            action_text,
            events,
            view,
        })
    }

    fn run_bots(&mut self, steps: &mut Vec<Step>) -> Result<(), String> {
        let mut actions = 0usize;
        loop {
            if self.game.is_over() || self.game.current_player() == HUMAN || self.stalemate {
                return Ok(());
            }
            if actions >= MAX_BOT_ACTIONS_PER_CALL {
                return Err(format!(
                    "bot 连续行动超过 {MAX_BOT_ACTIONS_PER_CALL} 步，拒绝继续"
                ));
            }
            if self.game.phase() == Phase::MainAction && self.game.legal_actions().is_empty() {
                self.stalemate = true;
                return Ok(());
            }

            let current = self.game.current_player();
            let action = {
                let view = self.game.view_for(current);
                let bot = self
                    .bots
                    .get_mut(current)
                    .and_then(Option::as_mut)
                    .ok_or_else(|| format!("玩家 {current} 不是 bot"))?;
                bot.choose_action(&view)
            };
            let Some(action) = action else {
                self.stalemate = true;
                return Ok(());
            };
            steps.push(self.apply_and_record(current, action)?);
            actions += 1;
        }
    }

    fn human_turn(&self) -> bool {
        !self.game.is_over() && !self.stalemate && self.game.current_player() == HUMAN
    }

    fn result_value(&self) -> Value {
        if self.stalemate {
            return Value::Null;
        }
        let Some(winners) = self.game.winners() else {
            return Value::Null;
        };
        let points: Vec<u16> = (0..self.game.players().len())
            .map(|player| self.game.points_for(player))
            .collect();
        json!({
            "winners": winners,
            "points": points,
            "turns": self.game.turn_number(),
        })
    }

    fn state_value(&self) -> Value {
        let mut view = self.game.view_for(HUMAN);
        view.log.clear();
        let legal_action_texts: Vec<String> = if self.human_turn() {
            self.game
                .legal_actions()
                .iter()
                .map(ToString::to_string)
                .collect()
        } else {
            Vec::new()
        };
        let log: Vec<String> = self
            .game
            .log()
            .iter()
            .map(|event| event.describe(&self.game))
            .collect();
        json!({
            "view": view,
            "log": log,
            "legal_action_texts": legal_action_texts,
            "human_turn": self.human_turn(),
            "completed": self.game.is_over() || self.stalemate,
            "stalemate": self.stalemate,
            "seed": self.seed,
            "bots": self.bots_count,
            "bot": self.bot_kind.as_str(),
            "controllers": &self.controllers,
            "result": self.result_value(),
        })
    }

    fn save_replay(&self) -> Result<Value, String> {
        let replay = self.recorder.to_replay(&self.game, self.stalemate);
        let stem = format!("pve-{}-{}p", self.seed, self.bots_count + 1);
        let json_path = crate::bot_cmd::default_tmp_file(&format!("{stem}.json"));
        let html_path = crate::bot_cmd::default_tmp_file(&format!("{stem}.html"));
        crate::bot_cmd::write_replay(&replay, &json_path, &html_path)?;
        Ok(json!({
            "json": json_path.display().to_string(),
            "html": html_path.display().to_string(),
            "frames": replay.frames.len(),
            "completed": replay.result.is_some() || replay.stalemate,
        }))
    }
}

#[derive(Serialize)]
struct Step {
    actor: PlayerId,
    action: Action,
    action_text: String,
    events: Vec<String>,
    view: GameView,
}

#[derive(Deserialize)]
struct NewGameRequest {
    seed: Option<u64>,
    bots: Option<usize>,
    bot: Option<String>,
    name: Option<String>,
}

#[derive(Deserialize)]
struct ActionRequest {
    action: Action,
}

fn handle_connection(session: &mut Session, stream: &mut TcpStream) -> Result<(), String> {
    stream.set_nonblocking(false).ok();
    stream.set_read_timeout(Some(Duration::from_secs(10))).ok();
    let Some(request) = read_request(stream)? else {
        return Ok(());
    };
    let (status, body) = match route(session, &request) {
        Ok(response) => response,
        Err(error) => (400, json!({ "ok": false, "error": error })),
    };
    write_response(stream, status, &body)
}

fn route(session: &mut Session, request: &Request) -> Result<(u16, Value), String> {
    let path = request.path.split('?').next().unwrap_or("");
    match (request.method.as_str(), path) {
        ("GET", "/api/health") => Ok((
            200,
            json!({
                "ok": true,
                "service": "splendor-pokemon",
                "version": env!("CARGO_PKG_VERSION"),
            }),
        )),
        ("GET", "/api/game/state") => {
            Ok((200, json!({ "ok": true, "state": session.state_value() })))
        }
        ("POST", "/api/game/new") => {
            let body = parse_body(&request.body)?;
            let options: NewGameRequest =
                serde_json::from_value(body).map_err(|error| format!("新对局参数无效: {error}"))?;
            let seed = options.seed.unwrap_or(42);
            let bots = options.bots.unwrap_or(2);
            let bot = match options.bot.as_deref() {
                Some(value) => BotKind::parse(value).ok_or_else(|| {
                    format!("未知 bot {value:?}，可用 minimax / heuristic / heuristic-v1 / random")
                })?,
                None => BotKind::Minimax,
            };
            let name = options.name.unwrap_or_else(|| "你".to_string());
            *session = Session::new(seed, bots, bot, name)?;
            Ok((200, json!({ "ok": true, "state": session.state_value() })))
        }
        ("POST", "/api/game/action") => {
            let body = parse_body(&request.body)?;
            let action: ActionRequest =
                serde_json::from_value(body).map_err(|error| format!("动作格式无效: {error}"))?;
            let steps = session.apply_human_action(action.action)?;
            Ok((
                200,
                json!({
                    "ok": true,
                    "steps": steps,
                    "state": session.state_value(),
                }),
            ))
        }
        ("POST", "/api/game/replay/save") => {
            let replay = session.save_replay()?;
            Ok((200, json!({ "ok": true, "replay": replay })))
        }
        ("OPTIONS", _) => Ok((204, Value::Null)),
        _ => Ok((
            404,
            json!({
                "ok": false,
                "error": format!("未知接口 {} {}", request.method, path),
            }),
        )),
    }
}

fn parse_body(body: &str) -> Result<Value, String> {
    if body.trim().is_empty() {
        return Ok(json!({}));
    }
    serde_json::from_str(body).map_err(|error| format!("JSON 解析失败: {error}"))
}

struct Request {
    method: String,
    path: String,
    body: String,
}

fn read_request(stream: &mut TcpStream) -> Result<Option<Request>, String> {
    let mut data = Vec::new();
    let mut buffer = [0u8; 8192];
    let mut header_end: Option<usize> = None;

    loop {
        let read = stream
            .read(&mut buffer)
            .map_err(|error| format!("读取请求失败: {error}"))?;
        if read == 0 {
            break;
        }
        data.extend_from_slice(&buffer[..read]);
        if data.len() > MAX_REQUEST_BYTES {
            return Err(format!("请求超过 {MAX_REQUEST_BYTES} 字节"));
        }
        if header_end.is_none() {
            header_end = find_bytes(&data, b"\r\n\r\n");
        }
        if let Some(position) = header_end {
            let head = String::from_utf8_lossy(&data[..position]);
            let content_length = content_length(&head);
            if data.len() >= position + 4 + content_length {
                break;
            }
        }
    }

    let Some(position) = header_end else {
        return if data.is_empty() {
            Ok(None)
        } else {
            Err("请求头不完整".into())
        };
    };
    let head = String::from_utf8_lossy(&data[..position]).to_string();
    let request_line = head
        .lines()
        .next()
        .ok_or_else(|| "缺少请求行".to_string())?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next().ok_or_else(|| "缺少方法".to_string())?;
    let path = parts.next().ok_or_else(|| "缺少路径".to_string())?;
    let body_start = position + 4;
    let body_end = body_start + content_length(&head);
    let body = String::from_utf8_lossy(&data[body_start.min(data.len())..body_end.min(data.len())])
        .to_string();
    Ok(Some(Request {
        method: method.to_string(),
        path: path.to_string(),
        body,
    }))
}

fn content_length(head: &str) -> usize {
    for line in head.lines().skip(1) {
        if let Some((name, value)) = line.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                if let Ok(length) = value.trim().parse::<usize>() {
                    return length;
                }
            }
        }
    }
    0
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return None;
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

fn write_response(stream: &mut TcpStream, status: u16, body: &Value) -> Result<(), String> {
    let reason = match status {
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        404 => "Not Found",
        405 => "Method Not Allowed",
        500 => "Internal Server Error",
        _ => "OK",
    };
    let payload = if status == 204 || body.is_null() {
        String::new()
    } else {
        serde_json::to_string(body).map_err(|error| format!("响应序列化失败: {error}"))?
    };
    let head = format!(
        "HTTP/1.1 {status} {reason}\r\n\
         Content-Type: application/json; charset=utf-8\r\n\
         Content-Length: {}\r\n\
         Access-Control-Allow-Origin: *\r\n\
         Access-Control-Allow-Methods: GET, POST, OPTIONS\r\n\
         Access-Control-Allow-Headers: Content-Type\r\n\
         Connection: close\r\n\
         \r\n",
        payload.len()
    );
    stream
        .write_all(head.as_bytes())
        .and_then(|()| stream.write_all(payload.as_bytes()))
        .and_then(|()| stream.flush())
        .map_err(|error| format!("写响应失败: {error}"))
}

struct Config {
    port: u16,
    idle_timeout: u64,
    seed: u64,
    bots: usize,
    bot: BotKind,
    name: String,
}

impl Config {
    fn from_args(args: &[String]) -> Result<Option<Self>, String> {
        let mut port = DEFAULT_PORT;
        let mut idle_timeout = 0u64;
        let mut seed = 42u64;
        let mut bots = 2usize;
        let mut bot = BotKind::Minimax;
        let mut name = "你".to_string();
        let mut index = 0;

        while index < args.len() {
            match args[index].as_str() {
                "--help" | "-h" => {
                    crate::ui::print_usage();
                    return Ok(None);
                }
                "--port" => {
                    index += 1;
                    port = args
                        .get(index)
                        .ok_or("--port 缺少数值")?
                        .parse()
                        .map_err(|_| "--port 必须是 0–65535".to_string())?;
                }
                "--idle-timeout" => {
                    index += 1;
                    idle_timeout = args
                        .get(index)
                        .ok_or("--idle-timeout 缺少数值")?
                        .parse()
                        .map_err(|_| "--idle-timeout 必须是整数秒".to_string())?;
                }
                "--seed" => {
                    index += 1;
                    seed = args
                        .get(index)
                        .ok_or("--seed 缺少数值")?
                        .parse()
                        .map_err(|_| "--seed 必须是整数".to_string())?;
                }
                "--bots" => {
                    index += 1;
                    bots = args
                        .get(index)
                        .ok_or("--bots 缺少数值")?
                        .parse()
                        .map_err(|_| "--bots 必须是 2 或 3".to_string())?;
                }
                "--bot" => {
                    index += 1;
                    let value = args.get(index).ok_or("--bot 缺少数值")?;
                    bot = BotKind::parse(value).ok_or_else(|| {
                        format!(
                            "未知 bot {value:?}，可用 minimax / heuristic / heuristic-v1 / random"
                        )
                    })?;
                }
                "--name" => {
                    index += 1;
                    name = args.get(index).ok_or("--name 缺少名字")?.clone();
                }
                other => return Err(format!("未知参数 {other}")),
            }
            index += 1;
        }

        if !(2..=3).contains(&bots) {
            return Err(format!("bots 必须为 2 或 3，当前为 {bots}"));
        }
        Ok(Some(Self {
            port,
            idle_timeout,
            seed,
            bots,
            bot,
            name,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session() -> Session {
        Session::new(7, 2, BotKind::Heuristic, "你".to_string()).expect("session")
    }

    fn first_take_three(session: &Session) -> Action {
        session
            .game
            .legal_actions()
            .into_iter()
            .find(|action| matches!(action, Action::TakeThree { .. }))
            .expect("take three")
    }

    #[test]
    fn new_session_waits_for_human() {
        let session = session();
        assert_eq!(session.game.current_player(), HUMAN);
        assert_eq!(session.game.phase(), Phase::MainAction);
        assert!(session.human_turn());
        assert!(!session.stalemate);
        assert_eq!(session.controllers, vec!["human", "heuristic", "heuristic"]);
    }

    #[test]
    fn human_action_runs_bots_until_next_human_turn() {
        let mut session = session();
        let action = first_take_three(&session);
        let steps = session.apply_human_action(action).expect("apply");
        assert_eq!(steps[0].actor, HUMAN);
        assert!(!steps[0].events.is_empty());
        assert_eq!(session.game.current_player(), HUMAN);
        assert_eq!(session.game.phase(), Phase::Evolution);

        let steps = session
            .apply_human_action(Action::EndTurn)
            .expect("end turn");
        assert!(steps.len() >= 2, "结束回合后 2 个 bot 应各行动一次");
        assert_eq!(session.game.current_player(), HUMAN);
        assert!(!session.stalemate);
        assert!(session.human_turn());
    }

    #[test]
    fn state_exposes_view_and_action_texts() {
        let session = session();
        let state = session.state_value();
        assert_eq!(state["human_turn"], true);
        assert_eq!(state["view"]["players"].as_array().unwrap().len(), 3);
        assert!(state["view"]["log"].as_array().unwrap().is_empty());
        assert!(!state["log"].as_array().unwrap().is_empty());
        assert_eq!(
            state["legal_action_texts"].as_array().unwrap().len(),
            state["view"]["legal_actions"].as_array().unwrap().len()
        );
    }

    #[test]
    fn http_route_runs_new_game_and_action() {
        let mut session = session();
        let new_body =
            json!({ "seed": 11, "bots": 2, "bot": "random", "name": "训练家" }).to_string();
        let (status, value) = route(
            &mut session,
            &Request {
                method: "POST".into(),
                path: "/api/game/new".into(),
                body: new_body,
            },
        )
        .expect("new game route");
        assert_eq!(status, 200);
        assert_eq!(value["ok"], true);
        assert_eq!(value["state"]["seed"], 11);
        assert_eq!(value["state"]["view"]["players"][0]["name"], "训练家");

        let action = first_take_three(&session);
        let action_body = json!({ "action": action }).to_string();
        let (status, value) = route(
            &mut session,
            &Request {
                method: "POST".into(),
                path: "/api/game/action".into(),
                body: action_body,
            },
        )
        .expect("action route");
        assert_eq!(status, 200);
        assert_eq!(value["steps"].as_array().unwrap().len(), 1);
        assert_eq!(value["state"]["human_turn"], true);

        let (status, value) = route(
            &mut session,
            &Request {
                method: "POST".into(),
                path: "/api/game/action".into(),
                body: json!({ "action": { "type": "end_turn" } }).to_string(),
            },
        )
        .expect("end turn route");
        assert_eq!(status, 200);
        assert!(value["steps"].as_array().unwrap().len() >= 2);
        assert_eq!(value["state"]["human_turn"], true);
    }

    #[test]
    fn session_plays_a_full_game_without_protocol_errors() {
        let mut session =
            Session::new(23, 2, BotKind::Heuristic, "你".to_string()).expect("session");
        let mut human = make_bot(BotKind::Heuristic, HUMAN, 23);
        for _ in 0..5_000 {
            if session.game.is_over() || session.stalemate {
                break;
            }
            let view = session.game.view_for(HUMAN);
            let Some(action) = human.choose_action(&view) else {
                break;
            };
            session.apply_human_action(action).expect("human action");
        }
        assert!(
            session.game.is_over() || session.stalemate,
            "对局应在 5000 个动作内结束（回合 {}）",
            session.game.turn_number()
        );
        let replay = session.recorder.to_replay(&session.game, session.stalemate);
        assert!(replay.frames.len() > 20);
        assert!(replay.result.is_some() || replay.stalemate);
    }

    #[test]
    fn discard_payload_from_ui_parses() {
        let action: Action = serde_json::from_value(json!({
            "type": "discard",
            "tokens": {
                "red": 1,
                "blue": 0,
                "yellow": 0,
                "pink": 0,
                "black": 0,
                "master": 0
            }
        }))
        .expect("discard payload");
        match action {
            Action::Discard { tokens } => {
                assert_eq!(tokens.red, 1);
                assert_eq!(tokens.master, 0);
            }
            other => panic!("expected discard, got {other:?}"),
        }
    }

    #[test]
    fn health_route_is_available() {
        let mut session = session();
        let (status, value) = route(
            &mut session,
            &Request {
                method: "GET".into(),
                path: "/api/health".into(),
                body: String::new(),
            },
        )
        .expect("health");
        assert_eq!(status, 200);
        assert_eq!(value["ok"], true);
    }
}
