extends Control
## 和 AI 对战：Godot 交互界面，规则与 bot 全部由本地 Rust 引擎执行。
##
## 交互模型：
## - 点场上牌 = 选中，卡片下方就地弹出「捕捉 / 保留」；
## - 点精灵球 = 放入行动篮（左键 +1、右键 −1），底栏统一确认；
## - 弃球与进化是同一套模态；Esc 逐级取消，再按打开菜单。
##
## 数据通道是 `splendor-pokemon serve` 的 127.0.0.1 JSON HTTP 接口。
## 视图一律来自 `view_for(0)`，界面不复制规则、不接触完整 `Game`。
## 环境变量 SPLENDOR_BATTLE_SMOKE=1 时执行无头冒烟，退出码 0 表示通过。

const Styles := preload("res://scripts/ui/styles.gd")
const Widgets := preload("res://scripts/ui/widgets.gd")
const TokenStrip := preload("res://scripts/game/token_strip.gd")

const MENU_SCENE := "res://scenes/menu.tscn"
const PORT := 43110
const DEFAULT_BASE_URL := "http://127.0.0.1:43110"
const CONNECT_TIMEOUT := 300.0
const BOT_LABELS := {
	"minimax": "极小化极大",
	"heuristic": "启发式",
	"heuristic-v1": "启发式 v1",
	"random": "随机",
}
const PHASES := {
	"main_action": "主要行动",
	"discard": "弃球",
	"evolution": "进化 / 结束回合",
	"game_over": "终局",
}
const COLOR_KEYS := ["red", "blue", "yellow", "pink", "black"]
const BOARD_CATEGORIES := ["level1", "level2", "level3", "rare", "legendary"]
const RESERVE_LIMIT := 3

@onready var phase_badge: Label = $Margin/Root/TopBar/PhaseBadge
@onready var meta_label: Label = $Margin/Root/TopBar/Meta
@onready var pause_button: Button = $Margin/Root/TopBar/PauseButton
@onready var board_view: Control = $Margin/Root/Body/BoardColumn/BoardPanel/VBox/BoardView
@onready var events_label: RichTextLabel = $Margin/Root/Body/BoardColumn/LogPanel/VBox/Events
@onready var opponents_box: VBoxContainer = $Margin/Root/Body/Side/OpponentsPanel/VBox/OpponentsBox
@onready var supply_view: Control = $Margin/Root/Body/Side/SupplyPanel/VBox/SupplyView
@onready var supply_hint: Label = $Margin/Root/Body/Side/SupplyPanel/VBox/SupplyHint
@onready var reserved_row: HFlowContainer = $Margin/Root/Body/Side/ReservedPanel/VBox/ReservedRow
@onready var me_head: HBoxContainer = $Margin/Root/Body/Side/MePanel/VBox/MeHead
@onready var me_tokens: Label = $Margin/Root/Body/Side/MePanel/VBox/MeTokens
@onready var me_cards: HFlowContainer = $Margin/Root/Body/Side/MePanel/VBox/MeCards
@onready var prompt_label: Label = $Margin/Root/ActionDock/VBox/PromptRow/PromptLabel
@onready var basket_label: Label = $Margin/Root/ActionDock/VBox/PromptRow/BasketLabel
@onready var end_turn_button: Button = $Margin/Root/ActionDock/VBox/ButtonRow/EndTurnButton
@onready var aux_button: Button = $Margin/Root/ActionDock/VBox/ButtonRow/AuxButton
@onready var cancel_button: Button = $Margin/Root/ActionDock/VBox/ButtonRow/CancelButton
@onready var confirm_button: Button = $Margin/Root/ActionDock/VBox/ButtonRow/ConfirmButton
@onready var context_actions: PanelContainer = $ContextActions
@onready var capture_button: Button = $ContextActions/Row/CaptureButton
@onready var reserve_button: Button = $ContextActions/Row/ReserveButton
@onready var context_close: Button = $ContextActions/Row/CloseButton
@onready var overlay: Control = $Overlay
@onready var modal: PanelContainer = $Overlay/Center/Modal
@onready var modal_title: Label = $Overlay/Center/Modal/VBox/Title
@onready var modal_body: VBoxContainer = $Overlay/Center/Modal/VBox/Body
@onready var modal_options: VBoxContainer = $Overlay/Center/Modal/VBox/Options
@onready var modal_cancel: Button = $Overlay/Center/Modal/VBox/Footer/CancelButton
@onready var modal_confirm: Button = $Overlay/Center/Modal/VBox/Footer/ConfirmButton
@onready var toast_panel: PanelContainer = $ToastPanel
@onready var toast_label: Label = $ToastPanel/Label
@onready var api_request: HTTPRequest = $ApiRequest
@onready var step_timer: Timer = $StepTimer
@onready var connect_timer: Timer = $ConnectTimer
@onready var toast_timer: Timer = $ToastTimer

var _view: Dictionary = {}
var _state: Dictionary = {}
var _log_lines: Array = []
var _selected := Vector2i(-1, -1)
var _supply_pick: Dictionary = {}
var _discard_pick: Dictionary = {}
var _evolution_action: Dictionary = {}
var _modal := ""
var _modal_return := ""
var _modal_confirm_cb := Callable()
var _last_action_line := ""

var _queue: Array = []
var _busy := false
var _active_callback := Callable()
var _base_url := DEFAULT_BASE_URL
var _connected := false
var _launch_attempted := false
var _server_pid := -1
var _connect_deadline := 0.0

var _animating := false
var _step_queue: Array = []
var _final_state: Dictionary = {}
var _replay_saved := false
var _has_env_url := false


func _ready() -> void:
	var env_url := OS.get_environment("SPLENDOR_SERVER_URL")
	if not env_url.is_empty():
		_base_url = env_url.trim_suffix("/")
		_has_env_url = true
	get_window().min_size = Vector2i(1180, 760)
	_apply_styles()
	_wire_signals()
	board_view.set_interactive(false)
	step_timer.wait_time = AppState.ai_step_delay()
	if OS.get_environment("SPLENDOR_BATTLE_SMOKE") == "1":
		_run_smoke()
		return
	_connect()


func _apply_styles() -> void:
	for button in [
		pause_button,
		end_turn_button,
		aux_button,
		cancel_button,
		capture_button,
		reserve_button,
		context_close,
		modal_cancel,
	]:
		Styles.style_button(button)
	Styles.style_button(confirm_button, true)
	Styles.style_button(modal_confirm, true)

	for path in [
		"Margin/Root/Body/BoardColumn/BoardPanel",
		"Margin/Root/Body/BoardColumn/LogPanel",
		"Margin/Root/Body/Side/OpponentsPanel",
		"Margin/Root/Body/Side/SupplyPanel",
		"Margin/Root/Body/Side/ReservedPanel",
		"Margin/Root/Body/Side/MePanel",
		"Margin/Root/ActionDock",
	]:
		var panel := get_node(path) as PanelContainer
		panel.add_theme_stylebox_override("panel", Styles.panel_box())
	context_actions.add_theme_stylebox_override(
		"panel", Styles.panel_box(Styles.COLOR_PANEL_2, Styles.COLOR_ACCENT, 10)
	)
	modal.add_theme_stylebox_override("panel", Styles.panel_box())
	toast_panel.add_theme_stylebox_override(
		"panel", Styles.panel_box(Color("2a2415"), Styles.COLOR_GOLD, 10)
	)

	events_label.add_theme_color_override("default_color", Styles.COLOR_MUTED)
	events_label.add_theme_font_size_override("normal_font_size", 12)
	events_label.add_theme_font_size_override("bold_font_size", 12)
	supply_hint.add_theme_color_override("font_color", Styles.COLOR_MUTED)
	me_tokens.add_theme_color_override("font_color", Styles.COLOR_MUTED)
	prompt_label.add_theme_color_override("font_color", Styles.COLOR_TEXT)
	basket_label.add_theme_color_override("font_color", Styles.COLOR_GOLD)
	phase_badge.add_theme_color_override("font_color", Styles.COLOR_ACCENT)


func _wire_signals() -> void:
	pause_button.pressed.connect(_open_pause_modal)
	end_turn_button.pressed.connect(_on_end_turn)
	aux_button.pressed.connect(_on_aux_pressed)
	cancel_button.pressed.connect(_on_dock_cancel)
	confirm_button.pressed.connect(_on_dock_confirm)
	board_view.slot_clicked.connect(_on_slot_clicked)
	supply_view.pile_clicked.connect(_on_supply_clicked)
	supply_view.pile_right_clicked.connect(_on_supply_right_clicked)
	capture_button.pressed.connect(_on_capture_pressed)
	reserve_button.pressed.connect(_on_reserve_pressed)
	context_close.pressed.connect(_clear_selection)
	modal_cancel.pressed.connect(_on_modal_cancel)
	modal_confirm.pressed.connect(_on_modal_confirm)
	api_request.request_completed.connect(_on_api_completed)
	step_timer.timeout.connect(_on_step_tick)
	connect_timer.timeout.connect(_on_connect_tick)
	toast_timer.timeout.connect(_on_toast_timeout)
	resized.connect(_place_context_actions)


func _unhandled_input(event: InputEvent) -> void:
	if not (event is InputEventKey) or not event.pressed or event.echo:
		return
	if event.keycode == KEY_ESCAPE:
		_handle_escape()
		get_viewport().set_input_as_handled()
		return
	if overlay.visible or not _can_interact():
		return
	if str(_view.get("phase", "")) != "main_action":
		return
	match event.keycode:
		KEY_1:
			_on_supply_clicked(COLOR_KEYS[0])
		KEY_2:
			_on_supply_clicked(COLOR_KEYS[1])
		KEY_3:
			_on_supply_clicked(COLOR_KEYS[2])
		KEY_4:
			_on_supply_clicked(COLOR_KEYS[3])
		KEY_5:
			_on_supply_clicked(COLOR_KEYS[4])
		_:
			return
	get_viewport().set_input_as_handled()
# ---------------------------------------------------------------------------
# 连接本地引擎
# ---------------------------------------------------------------------------


func _connect() -> void:
	_connected = false
	_launch_attempted = false
	_connect_deadline = Time.get_ticks_msec() / 1000.0 + CONNECT_TIMEOUT
	meta_label.text = "正在连接本地引擎…"
	meta_label.tooltip_text = _base_url
	_set_phase_badge("连接中", Styles.COLOR_ACCENT)
	_health_probe(true)


func _on_connect_tick() -> void:
	if _connected:
		connect_timer.stop()
		return
	_health_probe(true)


func _health_probe(may_launch: bool) -> void:
	# 未启动的端口在部分 Windows 环境会一直等到 HTTPRequest.timeout，
	# 探测必须用短超时，否则自动启动要白等一分钟。
	api_request.timeout = 2.0
	_request(HTTPClient.METHOD_GET, "/api/health", null, _on_health.bind(may_launch))


func _on_health(ok: bool, _code: int, _data: Dictionary, may_launch: bool) -> void:
	api_request.timeout = 60.0
	if ok:
		if _connected:
			return
		_connected = true
		connect_timer.stop()
		meta_label.text = "本地引擎已连接"
		if AppState.battle_resume:
			AppState.battle_resume = false
			meta_label.text = "正在恢复对局…"
			_request(HTTPClient.METHOD_GET, "/api/game/state", null, _on_resume_state)
		else:
			_start_new_game()
		return

	if Time.get_ticks_msec() / 1000.0 > _connect_deadline:
		connect_timer.stop()
		meta_label.text = "本地引擎未连接：请先构建后端，或在 backend/ 运行 cargo run -p splendor-pokemon-cli -- serve"
		_set_phase_badge("未连接", Styles.COLOR_RED)
		return

	if may_launch and not _launch_attempted:
		_launch_attempted = true
		if not _launch_server():
			connect_timer.stop()
			return
	connect_timer.start()


func _on_resume_state(ok: bool, _code: int, data: Dictionary) -> void:
	var state: Variant = data.get("state")
	if ok and state is Dictionary:
		_apply_state(state, true)
		return
	_start_new_game()


func _launch_server() -> bool:
	if _has_env_url:
		meta_label.text = "SPLENDOR_SERVER_URL 指向的引擎不可用；请启动该服务或清除环境变量"
		return false
	var executable := _find_server_executable()
	if not executable.is_empty():
		_server_pid = OS.create_process(
			executable,
			["serve", "--port", str(PORT), "--idle-timeout", "1800"]
		)
		if _server_pid > 0:
			AppState.server_pid = _server_pid
			meta_label.text = "正在启动本地引擎…"
			return true

	var cargo := _find_cargo()
	if cargo.is_empty():
		meta_label.text = "找不到 splendor-pokemon 或 cargo；请先在 backend/ 下 cargo build"
		return false
	var rust_root := _repo_dir().path_join(".rust")
	if DirAccess.dir_exists_absolute(rust_root.path_join("cargo")):
		OS.set_environment("CARGO_HOME", rust_root.path_join("cargo"))
		OS.set_environment("RUSTUP_HOME", rust_root.path_join("rustup"))
	var manifest := _repo_dir().path_join("backend").path_join("Cargo.toml")
	_server_pid = OS.create_process(
		cargo,
		[
			"run",
			"--manifest-path",
			manifest,
			"-p",
			"splendor-pokemon-cli",
			"--",
			"serve",
			"--port",
			str(PORT),
			"--idle-timeout",
			"1800",
		]
	)
	if _server_pid <= 0:
		meta_label.text = "启动本地引擎失败；请在 backend/ 手动运行 cargo run -p splendor-pokemon-cli -- serve"
		return false
	AppState.server_pid = _server_pid
	meta_label.text = "正在启动本地引擎（首次需要编译）…"
	return true


func _find_server_executable() -> String:
	var explicit := OS.get_environment("SPLENDOR_CLI")
	if not explicit.is_empty() and FileAccess.file_exists(explicit):
		return explicit
	var suffix := ".exe" if OS.get_name() == "Windows" else ""
	for profile in ["debug", "release"]:
		var path := _repo_dir().path_join(
			"backend/target/%s/splendor-pokemon%s" % [profile, suffix]
		)
		if FileAccess.file_exists(path):
			return path
	return ""


func _find_cargo() -> String:
	var suffix := ".exe" if OS.get_name() == "Windows" else ""
	var separator := ";" if OS.get_name() == "Windows" else ":"
	for directory in OS.get_environment("PATH").split(separator):
		if directory.is_empty():
			continue
		var path := directory.path_join("cargo" + suffix)
		if FileAccess.file_exists(path):
			return path
	var local := _repo_dir().path_join(".rust/cargo/bin/cargo" + suffix)
	return local if FileAccess.file_exists(local) else ""


func _repo_dir() -> String:
	return ProjectSettings.globalize_path("res://").trim_suffix("/").get_base_dir()
# ---------------------------------------------------------------------------
# JSON 请求队列
# ---------------------------------------------------------------------------


func _request(method: int, path: String, body: Variant, callback: Callable) -> void:
	_queue.append({"method": method, "path": path, "body": body, "callback": callback})
	_pump()


func _pump() -> void:
	if _busy or _queue.is_empty():
		return
	var item: Dictionary = _queue.pop_front()
	_busy = true
	_active_callback = item["callback"]
	var payload := ""
	if item["body"] != null:
		payload = JSON.stringify(item["body"])
	var error := api_request.request(
		_base_url + str(item["path"]),
		["Content-Type: application/json"],
		int(item["method"]),
		payload
	)
	if error != OK:
		_busy = false
		var callback := _active_callback
		_active_callback = Callable()
		if callback.is_valid():
			callback.call(false, 0, {"error": "无法连接本地引擎（%d）" % error})
		call_deferred("_pump")


func _on_api_completed(
	result: int, response_code: int, _headers: PackedStringArray, body: PackedByteArray
) -> void:
	_busy = false
	var data: Dictionary = {}
	if result == HTTPRequest.RESULT_SUCCESS:
		var parsed: Variant = _normalize_numbers(JSON.parse_string(body.get_string_from_utf8()))
		if parsed is Dictionary:
			data = parsed
	var ok := (
		result == HTTPRequest.RESULT_SUCCESS
		and response_code >= 200
		and response_code < 300
		and bool(data.get("ok", true))
	)
	if result != HTTPRequest.RESULT_SUCCESS and response_code == 0:
		_connected = false
	var callback := _active_callback
	_active_callback = Callable()
	if callback.is_valid():
		callback.call(ok, response_code, data)
	call_deferred("_pump")


static func _normalize_numbers(value: Variant) -> Variant:
	if typeof(value) == TYPE_FLOAT:
		var number: float = value
		if number == floor(number) and absf(number) < 9.0e15:
			return int(number)
		return number
	if value is Array:
		var array: Array = []
		array.resize(value.size())
		for i in value.size():
			array[i] = _normalize_numbers(value[i])
		return array
	if value is Dictionary:
		var dict: Dictionary = {}
		for key in value:
			dict[key] = _normalize_numbers(value[key])
		return dict
	return value


# ---------------------------------------------------------------------------
# 开局、动作与逐步播放
# ---------------------------------------------------------------------------


func _start_new_game() -> void:
	if not _connected:
		_connect()
		return
	if _animating or _busy:
		return
	_queue.clear()
	_selected = Vector2i(-1, -1)
	_supply_pick.clear()
	_discard_pick.clear()
	_evolution_action.clear()
	context_actions.visible = false
	_close_modal()
	_state = {}
	_view = {}
	_replay_saved = false
	AppState.battle_in_progress = true
	meta_label.text = "正在开局…"
	var cfg: Dictionary = AppState.battle_config
	_request(
		HTTPClient.METHOD_POST,
		"/api/game/new",
		{
			"seed": int(cfg.get("seed", 42)),
			"bots": int(cfg.get("bots", 2)),
			"bot": str(cfg.get("bot", "minimax")),
			"name": "你",
		},
		_on_new_game
	)
	_refresh_all()


func _on_new_game(ok: bool, _code: int, data: Dictionary) -> void:
	if not ok:
		_show_error(data)
		return
	var state: Variant = data.get("state")
	if not (state is Dictionary):
		_show_error({"error": "开局响应缺少 state"})
		return
	_apply_state(state, true)


func _send_action(action: Dictionary) -> void:
	if action.is_empty() or _animating or _busy:
		return
	if not _can_interact():
		_toast("现在还不能行动")
		return
	var action_type := str(action.get("type", ""))
	if action_type != "discard" and not _is_legal(action):
		_toast("当前不能执行这个动作")
		return
	meta_label.text = "正在执行动作…"
	_request(HTTPClient.METHOD_POST, "/api/game/action", {"action": action}, _on_action)
	_refresh_all()


func _on_action(ok: bool, _code: int, data: Dictionary) -> void:
	if not ok:
		_refresh_all()
		_show_error(data)
		return
	_selected = Vector2i(-1, -1)
	_supply_pick.clear()
	_discard_pick.clear()
	_evolution_action.clear()
	context_actions.visible = false
	_close_modal()
	board_view.set_selected(_selected)
	var steps: Variant = data.get("steps")
	var state: Variant = data.get("state")
	_final_state = state if state is Dictionary else {}
	_step_queue = (steps as Array).duplicate() if steps is Array else []
	if _step_queue.is_empty():
		if not _final_state.is_empty():
			_apply_state(_final_state, true)
		return
	_animating = true
	meta_label.text = "对手行动中…"
	_advance_step()


func _advance_step() -> void:
	if _step_queue.is_empty():
		_animating = false
		if not _final_state.is_empty():
			_apply_state(_final_state, true)
		else:
			_refresh_all()
		return
	var step: Dictionary = _step_queue.pop_front()
	var view: Variant = step.get("view")
	if view is Dictionary:
		_apply_view(view)
	_append_step(step)
	var delay := AppState.ai_step_delay()
	if int(step.get("actor", 0)) == 0:
		delay *= 0.8
	step_timer.start(delay)


func _on_step_tick() -> void:
	_advance_step()


func _append_step(step: Dictionary) -> void:
	var events: Variant = step.get("events")
	if events is Array:
		for line in events:
			_log_lines.append(str(line))
	_trim_log()
	var actor := int(step.get("actor", 0))
	var text := str(step.get("action_text", ""))
	_last_action_line = "%s：%s" % [_player_name(_view, actor), text]
# ---------------------------------------------------------------------------
# 状态与视图
# ---------------------------------------------------------------------------


func _apply_state(state: Dictionary, replace_log: bool) -> void:
	_state = state
	var view: Variant = state.get("view")
	_view = view if view is Dictionary else {}
	if replace_log:
		_log_lines.clear()
		var log: Variant = state.get("log")
		if log is Array:
			for line in log:
				_log_lines.append(str(line))
		_trim_log()
	_animating = false
	_apply_view(_view)
	if str(_view.get("phase", "")) == "discard" and _can_interact() and _modal == "":
		_open_discard_modal()
	var completed := (
		bool(_state.get("completed", false))
		or str(_view.get("phase", "")) == "game_over"
	)
	AppState.battle_in_progress = not completed
	if completed:
		if not _replay_saved:
			_replay_saved = true
			_save_replay(true)
		if _modal != "result":
			_open_result_modal()


func _trim_log() -> void:
	var limit := 240
	if _log_lines.size() > limit:
		_log_lines = _log_lines.slice(_log_lines.size() - limit)
	events_label.text = "\n".join(_log_lines)


func _apply_view(view: Dictionary) -> void:
	_view = view
	if _selected.x >= 0 and _selected_card().is_empty():
		_selected = Vector2i(-1, -1)
	board_view.set_view(view.get("board", {}))
	board_view.set_actionable(_actionable_map())
	board_view.set_selected(_selected)
	board_view.set_interactive(_board_interactive())
	_render_opponents()
	_render_me()
	_render_reserved()
	_render_supply()
	_render_meta()
	_render_dock()
	_update_context_actions()


func _refresh_all() -> void:
	_apply_view(_view)


func _render_meta() -> void:
	if not _connected:
		return
	if _busy:
		meta_label.text = "正在等待本地引擎…"
		meta_label.tooltip_text = _base_url
		return
	var seed := int(_state.get("seed", 0))
	var bots := int(_state.get("bots", 0))
	var bot_id := str(_state.get("bot", "minimax"))
	var turn := int(_view.get("turn_number", 0))
	meta_label.text = "种子 %d · 你 vs %d 个%s · 第 %d 回合" % [
		seed,
		bots,
		BOT_LABELS.get(bot_id, bot_id),
		turn,
	]
	meta_label.tooltip_text = _base_url
	if not bool(_can_interact()):
		meta_label.text += " · 等待中"


## legal_actions → {"row:slot": {"capture": true, "reserve": true}}。
func _actionable_map() -> Dictionary:
	var result := {}
	var actions: Variant = _view.get("legal_actions")
	if not (actions is Array):
		return result
	for candidate in actions:
		if not (candidate is Dictionary):
			continue
		var action: Dictionary = candidate
		var action_type := str(action.get("type", ""))
		if action_type != "capture" and action_type != "reserve":
			continue
		var source: Variant = action.get("source")
		if not (source is Dictionary):
			continue
		if str((source as Dictionary).get("type", "")) != "board":
			continue
		var row := BOARD_CATEGORIES.find(str((source as Dictionary).get("category", "")))
		if row < 0:
			continue
		var key := "%d:%d" % [row, int((source as Dictionary).get("index", 0))]
		var entry: Dictionary = result.get(key, {})
		entry[action_type] = true
		result[key] = entry
	return result


func _board_interactive() -> bool:
	return _can_interact() and str(_view.get("phase", "")) == "main_action"


func _render_dock() -> void:
	var phase := str(_view.get("phase", ""))
	var my_turn := _can_interact()
	var picking := not _supply_pick.is_empty()
	var card := _selected_card()
	_update_phase_badge(phase)

	end_turn_button.visible = my_turn and phase == "evolution"
	end_turn_button.disabled = not my_turn or not _is_legal({"type": "end_turn"})
	aux_button.visible = my_turn and (phase == "discard" or phase == "evolution")
	aux_button.text = "弃球…" if phase == "discard" else "进化…"

	if picking:
		var take := _take_action()
		confirm_button.visible = true
		confirm_button.disabled = not my_turn or take.is_empty() or not _is_legal(take)
		confirm_button.text = (
			"拿取 " + _pick_text(_supply_pick) if not take.is_empty() else "还需选择"
		)
	else:
		confirm_button.visible = false

	cancel_button.visible = picking or _selected.x >= 0
	cancel_button.disabled = not my_turn or not (picking or _selected.x >= 0)
	cancel_button.text = "清空" if picking else "取消选择"

	if not _connected:
		prompt_label.text = "正在连接本地引擎…"
	elif _busy:
		prompt_label.text = "正在等待本地引擎…"
	elif _animating:
		prompt_label.text = "等待对手行动…"
	elif bool(_state.get("completed", false)):
		prompt_label.text = "对局已结束。可以保存回放或再来一局。"
	elif phase == "discard":
		prompt_label.text = "需要弃 %d 个精灵球（球数上限 10）。" % _discard_required()
	elif phase == "evolution":
		prompt_label.text = "可以进化一只宝可梦，或直接结束回合。"
	elif phase == "main_action":
		if not card.is_empty():
			prompt_label.text = "已选「%s」：用卡片下方的按钮捕捉或保留。" % str(
				card.get("name_zh", "?")
			)
		elif picking:
			prompt_label.text = "行动篮已选 %d 个球：确认拿取，或继续选择。" % _pick_total(
				_supply_pick
			)
		else:
			prompt_label.text = "选择一张场上牌，或点击精灵球拿取。"
	else:
		prompt_label.text = str(PHASES.get(phase, phase))

	if picking:
		basket_label.text = "行动篮 " + _pick_text(_supply_pick)
	elif not card.is_empty():
		basket_label.text = "%s · %d 分" % [
			str(card.get("name_zh", "?")),
			int(card.get("points", 0)),
		]
	elif _animating:
		basket_label.text = _last_action_line
	else:
		basket_label.text = ""


func _update_phase_badge(phase: String) -> void:
	if not _connected:
		_set_phase_badge("连接中", Styles.COLOR_ACCENT)
		return
	if bool(_state.get("completed", false)) or phase == "game_over":
		_set_phase_badge("对局结束", Styles.COLOR_GOLD)
		return
	if _animating:
		_set_phase_badge("对手行动中…", Styles.COLOR_YELLOW)
		return
	if not _can_interact():
		_set_phase_badge("等待对手", Styles.COLOR_MUTED)
		return
	_set_phase_badge(
		"你的回合 · %s" % str(PHASES.get(phase, phase)), Styles.COLOR_ACCENT
	)


func _set_phase_badge(text: String, color: Color) -> void:
	phase_badge.text = text
	phase_badge.add_theme_color_override("font_color", color)
# ---------------------------------------------------------------------------
# 渲染：对手、自己、保留牌、供应区
# ---------------------------------------------------------------------------


func _render_opponents() -> void:
	Widgets.clear(opponents_box)
	var players: Variant = _view.get("players")
	if not (players is Array):
		return
	var current := int(_view.get("current_player", -1))
	var game_over := str(_view.get("phase", "")) == "game_over"
	for index in range(1, (players as Array).size()):
		var player_v: Variant = (players as Array)[index]
		if player_v is Dictionary:
			opponents_box.add_child(_opponent_panel(player_v, current, game_over))


func _opponent_panel(player: Dictionary, current_id: int, game_over: bool) -> PanelContainer:
	var player_id := int(player.get("id", 0))
	var active := player_id == current_id and not game_over
	var panel := Widgets.panel(Styles.panel_box(
		Styles.COLOR_PANEL_2,
		Styles.COLOR_ACCENT if active else Styles.COLOR_LINE,
		10,
		2 if active else 1
	))
	panel.tooltip_text = _player_tooltip(player)
	var vbox := VBoxContainer.new()
	vbox.add_theme_constant_override("separation", 3)
	panel.add_child(vbox)

	var head := HBoxContainer.new()
	head.add_theme_constant_override("separation", 7)
	vbox.add_child(head)
	head.add_child(Widgets.label(str(player.get("name", "?")), 15, Styles.COLOR_TEXT))
	if active:
		head.add_child(Widgets.badge("行动中"))
	head.add_child(Widgets.spacer())
	var score := Widgets.label(str(int(player.get("points", 0))), 20, Styles.COLOR_GOLD)
	score.vertical_alignment = VERTICAL_ALIGNMENT_CENTER
	head.add_child(score)
	head.add_child(Widgets.label("分", 11, Styles.COLOR_MUTED))

	var tokens: Dictionary = player.get("tokens", {})
	vbox.add_child(Widgets.label(_token_summary(tokens), 11, Styles.COLOR_MUTED))

	var stats := "减免 %s ｜ 捕捉 %d · 保留 %d/%d · 进化 %d" % [
		_bonus_summary(player.get("bonuses", {})),
		int(player.get("card_count", 0)),
		int(player.get("reserved_count", 0)),
		RESERVE_LIMIT,
		int(player.get("evolution_count", 0)),
	]
	vbox.add_child(Widgets.label(stats, 11, Styles.COLOR_MUTED))

	var captured: Array = []
	var captured_v: Variant = player.get("captured")
	if captured_v is Array:
		captured = captured_v
	if not captured.is_empty():
		vbox.add_child(Widgets.label(_captured_summary(captured), 11, Styles.COLOR_TEXT))
	return panel


func _render_me() -> void:
	Widgets.clear(me_head)
	Widgets.clear(me_cards)
	var players: Variant = _view.get("players")
	if not (players is Array) or (players as Array).is_empty():
		me_tokens.text = "精灵球：—"
		return
	var me: Dictionary = (players as Array)[0]
	me_head.add_child(Widgets.label(str(me.get("name", "你")), 16, Styles.COLOR_TEXT))
	me_head.add_child(Widgets.badge("我"))
	me_head.add_child(Widgets.spacer())
	me_head.add_child(Widgets.label(str(int(me.get("points", 0))), 22, Styles.COLOR_GOLD))
	me_head.add_child(Widgets.label("分", 11, Styles.COLOR_MUTED))

	var tokens: Dictionary = me.get("tokens", {})
	var bonuses: Dictionary = me.get("bonuses", {})
	me_tokens.text = "精灵球：%s ｜ 减免：%s" % [
		_token_summary(tokens),
		_bonus_summary(bonuses),
	]

	var captured: Array = []
	var captured_v: Variant = me.get("captured")
	if captured_v is Array:
		captured = captured_v
	captured.sort_custom(
		func(a, b): return int(a.get("points", 0)) > int(b.get("points", 0))
	)
	for index in mini(4, captured.size()):
		if captured[index] is Dictionary:
			me_cards.add_child(Widgets.card_chip(captured[index]))
	if captured.size() > 4:
		me_cards.add_child(Widgets.chip("… +%d" % (captured.size() - 4)))
	if captured.is_empty():
		me_cards.add_child(Widgets.chip("尚无宝可梦"))
func _render_reserved() -> void:
	Widgets.clear(reserved_row)
	var players: Variant = _view.get("players")
	if not (players is Array) or (players as Array).is_empty():
		return
	var me: Dictionary = (players as Array)[0]
	var reserved: Array = []
	var reserved_v: Variant = me.get("reserved")
	if reserved_v is Array:
		reserved = reserved_v
	if reserved.is_empty():
		reserved_row.add_child(Widgets.chip("没有保留牌"))
		return
	for index in reserved.size():
		var card_v: Variant = reserved[index]
		if not (card_v is Dictionary):
			continue
		var card: Dictionary = card_v
		var legal := _is_legal(_capture_reserved(index)) and _can_interact()
		var button := Button.new()
		button.text = "捕捉 · %s %d分" % [
			str(card.get("name_zh", "?")),
			int(card.get("points", 0)),
		]
		button.tooltip_text = _card_tooltip(card)
		button.disabled = not legal
		var color: Color = Styles.TOKEN_COLORS.get(
			str(card.get("bonus_color", "red")), Styles.COLOR_ACCENT
		)
		button.add_theme_stylebox_override(
			"normal",
			Styles.button_box(Styles.COLOR_PANEL_2.lerp(color, 0.18), color.darkened(0.25))
		)
		button.add_theme_stylebox_override(
			"hover",
			Styles.button_box(Styles.COLOR_PANEL_2.lerp(color, 0.32), color)
		)
		button.pressed.connect(_on_reserved_pressed.bind(index))
		reserved_row.add_child(button)


func _render_supply() -> void:
	var supply: Dictionary = _view.get("supply", {})
	supply_view.set_state(supply, _supply_pick, _board_interactive(), _supply_blocks())
	supply_hint.text = _supply_hint_text()


func _supply_blocks() -> Array:
	var blocked: Array = ["master"]
	var two_color := ""
	var total := _pick_total(_supply_pick)
	for key in COLOR_KEYS:
		if int(_supply_pick.get(key, 0)) >= 2:
			two_color = key
	var supply: Dictionary = _view.get("supply", {})
	for key in COLOR_KEYS:
		var available := int(supply.get(key, 0))
		if available <= 0:
			blocked.append(key)
			continue
		var picked := int(_supply_pick.get(key, 0))
		if two_color != "" and key != two_color:
			blocked.append(key)
		elif total >= 3:
			blocked.append(key)
		elif picked >= 2:
			blocked.append(key)
		elif picked == 1 and available < 4:
			blocked.append(key)
	return blocked


func _supply_hint_text() -> String:
	if not _board_interactive():
		return "现在不是拿球阶段。"
	var total := _pick_total(_supply_pick)
	if total == 0:
		return "最多拿 3 个：三种颜色各 1，或同色 2 个（该色需至少 4 个）。"
	var two_color := ""
	for key in COLOR_KEYS:
		if int(_supply_pick.get(key, 0)) >= 2:
			two_color = str(Styles.COLOR_LABELS.get(key, key))
	if not two_color.is_empty():
		return "已选 2 个%s：不能再拿其他颜色，确认或右键减少。" % two_color
	var action := _take_action()
	if not action.is_empty() and not _is_legal(action):
		return "当前选择不合法：通常需要 3 种不同颜色各 1，或同色 2 个。"
	return "已选 %d 个：可以再选不同颜色，或用底栏确认。" % total


func _token_summary(tokens: Dictionary) -> String:
	var parts: Array[String] = []
	for key in Styles.COLOR_KEYS:
		var count := int(tokens.get(key, 0))
		if count > 0:
			parts.append("%s%d" % [Styles.COLOR_LABELS[key], count])
	if parts.is_empty():
		return "无"
	return " ".join(parts)


func _bonus_summary(bonuses: Dictionary) -> String:
	var parts: Array[String] = []
	for key in COLOR_KEYS:
		var count := int(bonuses.get(key, 0))
		if count > 0:
			parts.append("%s%d" % [Styles.COLOR_LABELS[key], count])
	return " ".join(parts) if not parts.is_empty() else "无"


func _captured_summary(captured: Array) -> String:
	var parts: Array[String] = []
	var sorted := captured.duplicate()
	sorted.sort_custom(
		func(a, b): return int(a.get("points", 0)) > int(b.get("points", 0))
	)
	for index in mini(2, sorted.size()):
		if sorted[index] is Dictionary:
			parts.append("%s %d分" % [
				str(sorted[index].get("name_zh", "?")),
				int(sorted[index].get("points", 0)),
			])
	if parts.is_empty():
		return "尚无宝可梦"
	return "最强：" + " · ".join(parts)
# ---------------------------------------------------------------------------
# 选择与主要行动
# ---------------------------------------------------------------------------


func _on_slot_clicked(row: int, slot: int, card: Dictionary) -> void:
	if not _board_interactive():
		return
	if card.is_empty():
		_clear_selection()
		return
	var key := Vector2i(row, slot)
	if _selected == key:
		_clear_selection()
		return
	_selected = key
	_supply_pick.clear()
	board_view.set_selected(_selected)
	_render_supply()
	_render_dock()
	_update_context_actions()


func _clear_selection() -> void:
	_selected = Vector2i(-1, -1)
	board_view.set_selected(_selected)
	context_actions.visible = false
	_render_dock()


func _update_context_actions() -> void:
	var card := _selected_card()
	if card.is_empty() or not _board_interactive():
		context_actions.visible = false
		return
	var capture := _capture_board(_selected.x, _selected.y)
	var reserve := _reserve_board(_selected.x, _selected.y)
	var capture_ok := _is_legal(capture)
	var reserve_ok := _is_legal(reserve)

	capture_button.disabled = not capture_ok
	capture_button.text = "捕捉" if capture_ok else "捕捉 · %s" % _capture_block_reason(card)
	capture_button.tooltip_text = _card_tooltip(card)
	reserve_button.disabled = not reserve_ok
	reserve_button.text = "保留 · 拿 1 大师球" if reserve_ok else "保留 · 不可用"
	reserve_button.tooltip_text = (
		"保留这张牌并获得 1 个大师球（上限 %d 张）" % RESERVE_LIMIT
		if reserve_ok
		else "保留牌已满或这张牌不能保留"
	)
	context_actions.visible = true
	context_actions.reset_size()
	call_deferred("_place_context_actions")


func _capture_block_reason(card: Dictionary) -> String:
	var missing: Array[String] = []
	var cost: Dictionary = {}
	var cost_raw: Variant = card.get("cost", {})
	if cost_raw is Dictionary:
		cost = cost_raw
	var me := _me_player()
	var tokens: Dictionary = me.get("tokens", {})
	var bonuses: Dictionary = me.get("bonuses", {})
	for key in COLOR_KEYS:
		var need := (
			int(cost.get(key, 0))
			- int(bonuses.get(key, 0))
			- int(tokens.get(key, 0))
		)
		if need > 0:
			missing.append("%s缺%d" % [Styles.COLOR_LABELS.get(key, key), need])
	if missing.is_empty():
		return "不可用"
	var text := " ".join(missing)
	if int(tokens.get("master", 0)) > 0:
		text += "（大师球可补）"
	return text


func _place_context_actions() -> void:
	if not context_actions.visible:
		return
	var rect: Rect2 = board_view.selection_rect()
	if rect.size.x <= 0.0:
		return
	var bar := context_actions.size
	if bar.x <= 0.0:
		bar = Vector2(320, 48)
	var origin: Vector2 = (
		get_global_transform().affine_inverse()
		* (board_view.global_position + rect.position)
	)
	var pos := Vector2(
		origin.x + rect.size.x * 0.5 - bar.x * 0.5,
		origin.y + rect.size.y + 8.0
	)
	if pos.y + bar.y > size.y - 8.0:
		pos.y = origin.y - bar.y - 8.0
	pos.x = clampf(pos.x, 8.0, size.x - bar.x - 8.0)
	pos.y = clampf(pos.y, 8.0, size.y - bar.y - 8.0)
	context_actions.position = pos


func _on_capture_pressed() -> void:
	if _selected.x < 0:
		return
	_send_action(_capture_board(_selected.x, _selected.y))


func _on_reserve_pressed() -> void:
	if _selected.x < 0:
		return
	_send_action(_reserve_board(_selected.x, _selected.y))


func _on_reserved_pressed(index: int) -> void:
	if not _can_interact():
		_toast("现在还不能行动")
		return
	_send_action(_capture_reserved(index))


func _on_supply_clicked(key: String) -> void:
	if not _board_interactive():
		return
	if key == "master":
		_toast("大师球不能主动拿取，只能通过保留获得。")
		return
	var supply: Dictionary = _view.get("supply", {})
	var available := int(supply.get(key, 0))
	if available <= 0:
		return
	var picked := int(_supply_pick.get(key, 0))
	if picked >= 2:
		_toast("同色最多拿 2 个。")
		return
	for other in COLOR_KEYS:
		if other != key and int(_supply_pick.get(other, 0)) >= 2:
			_toast("已经选了 2 个同色，不能再拿其他颜色。")
			return
	if _pick_total(_supply_pick) >= 3:
		_toast("一次最多拿 3 个精灵球。")
		return
	if picked == 1 and available < 4:
		_toast("同色拿 2 个需要供应区该色至少 4 个。")
		return
	_supply_pick[key] = picked + 1
	_clear_selection()
	_render_supply()
	_render_dock()


func _on_supply_right_clicked(key: String) -> void:
	if not _board_interactive():
		return
	var picked := int(_supply_pick.get(key, 0))
	if picked <= 0:
		return
	if picked <= 1:
		_supply_pick.erase(key)
	else:
		_supply_pick[key] = picked - 1
	_render_supply()
	_render_dock()


func _take_action() -> Dictionary:
	var colors: Array[String] = []
	var total := 0
	var two_color := ""
	for key in COLOR_KEYS:
		var count := int(_supply_pick.get(key, 0))
		if count == 0:
			continue
		total += count
		if count == 1:
			colors.append(key)
		elif count == 2:
			if not two_color.is_empty():
				return {}
			two_color = key
		else:
			return {}
	if not two_color.is_empty():
		if total == 2 and colors.is_empty():
			return {"type": "take_two", "color": two_color}
		return {}
	if total >= 1 and total <= 3 and colors.size() == total:
		return {"type": "take_three", "colors": colors}
	return {}
func _on_dock_confirm() -> void:
	var phase := str(_view.get("phase", ""))
	if phase == "discard":
		_open_discard_modal()
		return
	if _supply_pick.is_empty():
		return
	var action := _take_action()
	if action.is_empty():
		_toast("请选择 3 种不同颜色各 1 个，或 1 种颜色 2 个。")
		return
	_send_action(action)


func _on_dock_cancel() -> void:
	_supply_pick.clear()
	_clear_selection()
	_render_supply()
	_render_dock()


func _on_end_turn() -> void:
	_send_action({"type": "end_turn"})


func _on_aux_pressed() -> void:
	match str(_view.get("phase", "")):
		"discard":
			_open_discard_modal()
		"evolution":
			_open_evolution_modal()


func _pick_total(pick: Dictionary) -> int:
	var total := 0
	for key in COLOR_KEYS:
		total += int(pick.get(key, 0))
	total += int(pick.get("master", 0))
	return total


func _pick_text(pick: Dictionary) -> String:
	var parts: Array[String] = []
	for key in Styles.COLOR_KEYS:
		var count := int(pick.get(key, 0))
		if count > 0:
			parts.append("%s×%d" % [Styles.COLOR_LABELS.get(key, key), count])
	return " ".join(parts) if not parts.is_empty() else "—"


func _discard_required() -> int:
	var me := _me_player()
	var tokens: Dictionary = me.get("tokens", {})
	return maxi(0, _token_total(tokens) - 10)


func _me_player() -> Dictionary:
	var players: Variant = _view.get("players")
	if players is Array and not (players as Array).is_empty():
		var me: Variant = (players as Array)[0]
		if me is Dictionary:
			return me
	return {}


func _selected_card() -> Dictionary:
	if _selected.x < 0 or _selected.x >= BOARD_CATEGORIES.size():
		return {}
	var board: Variant = _view.get("board")
	if not (board is Dictionary):
		return {}
	var slots: Variant = (board as Dictionary).get(BOARD_CATEGORIES[_selected.x])
	if not (slots is Array) or _selected.y < 0 or _selected.y >= (slots as Array).size():
		return {}
	var card: Variant = (slots as Array)[_selected.y]
	return card if card is Dictionary else {}


func _capture_board(row: int, slot: int) -> Dictionary:
	return {
		"type": "capture",
		"source": {
			"type": "board",
			"category": BOARD_CATEGORIES[row],
			"index": slot,
		},
	}


func _reserve_board(row: int, slot: int) -> Dictionary:
	return {
		"type": "reserve",
		"source": {
			"type": "board",
			"category": BOARD_CATEGORIES[row],
			"index": slot,
		},
	}


func _capture_reserved(index: int) -> Dictionary:
	return {"type": "capture", "source": {"type": "reserved", "index": index}}
# ---------------------------------------------------------------------------
# 模态：弃球 / 进化 / 菜单 / 结算
# ---------------------------------------------------------------------------


func _begin_modal(kind: String, title: String) -> void:
	_modal = kind
	overlay.visible = true
	modal_title.text = title
	Widgets.clear(modal_body)
	Widgets.clear(modal_options)
	modal_cancel.visible = true
	modal_confirm.visible = true
	modal_confirm.disabled = false
	modal_cancel.text = "取消"
	modal_confirm.text = "确定"
	_modal_confirm_cb = Callable()
	_modal_return = ""


func _close_modal() -> void:
	overlay.visible = false
	_modal = ""
	_modal_return = ""
	_modal_confirm_cb = Callable()
	Widgets.clear(modal_body)
	Widgets.clear(modal_options)


func _modal_option(text: String, callback: Callable, primary: bool = false) -> Button:
	var button := Button.new()
	button.text = text
	button.custom_minimum_size = Vector2(0, 38)
	Styles.style_button(button, primary)
	button.pressed.connect(callback)
	modal_options.add_child(button)
	return button


# ---- 弃球 ----------------------------------------------------------------


func _open_discard_modal() -> void:
	if not _can_interact():
		return
	_begin_modal("discard", "弃球")
	_build_discard_modal()


func _build_discard_modal() -> void:
	Widgets.clear(modal_body)
	Widgets.clear(modal_options)
	var me := _me_player()
	var tokens: Dictionary = me.get("tokens", {})
	var held := _token_total(tokens)
	var required := maxi(0, held - 10)
	var picked := _token_total(_discard_pick)

	modal_body.add_child(Widgets.label(
		"你持有 %d 个球，上限 10，还需弃 %d 个。" % [held, required],
		14,
		Styles.COLOR_TEXT
	))
	modal_body.add_child(Widgets.label(
		"持有：左键 +1 / 右键 −1", 12, Styles.COLOR_MUTED
	))
	var held_strip: Control = TokenStrip.new()
	held_strip.custom_minimum_size = Vector2(0, 58)
	held_strip.set_state(tokens, _discard_pick, true, _discard_blocked(tokens))
	held_strip.pile_clicked.connect(_on_discard_add)
	held_strip.pile_right_clicked.connect(_on_discard_remove)
	modal_body.add_child(held_strip)

	modal_body.add_child(Widgets.label("弃球区：点击取回", 12, Styles.COLOR_MUTED))
	var out_strip: Control = TokenStrip.new()
	out_strip.custom_minimum_size = Vector2(0, 58)
	out_strip.set_state(_discard_pick, {}, picked > 0, [])
	out_strip.pile_clicked.connect(_on_discard_remove)
	modal_body.add_child(out_strip)

	modal_cancel.text = "关闭"
	modal_confirm.text = "确认弃球（%d / %d）" % [picked, required]
	modal_confirm.disabled = picked != required or required <= 0


func _discard_blocked(tokens: Dictionary) -> Array:
	var blocked: Array = []
	for key in Styles.COLOR_KEYS:
		if int(_discard_pick.get(key, 0)) >= int(tokens.get(key, 0)):
			blocked.append(key)
	return blocked


func _on_discard_add(key: String) -> void:
	var me := _me_player()
	var tokens: Dictionary = me.get("tokens", {})
	var owned := int(tokens.get(key, 0))
	var picked := int(_discard_pick.get(key, 0))
	if picked >= owned:
		return
	_discard_pick[key] = picked + 1
	_build_discard_modal()


func _on_discard_remove(key: String) -> void:
	var picked := int(_discard_pick.get(key, 0))
	if picked <= 0:
		return
	if picked <= 1:
		_discard_pick.erase(key)
	else:
		_discard_pick[key] = picked - 1
	_build_discard_modal()


func _confirm_discard() -> void:
	var required := _discard_required()
	if _token_total(_discard_pick) != required:
		_toast("弃球数量必须正好让球数降到 10。")
		return
	var payload := {}
	for key in Styles.COLOR_KEYS:
		payload[key] = int(_discard_pick.get(key, 0))
	_close_modal()
	_send_action({"type": "discard", "tokens": payload})
# ---- 进化 ----------------------------------------------------------------


func _open_evolution_modal() -> void:
	if not _can_interact():
		return
	_begin_modal("evolution", "进化")
	_evolution_action = {}
	modal_cancel.text = "关闭"
	modal_confirm.text = "进化"
	modal_confirm.disabled = true
	modal_body.add_child(Widgets.label(
		"选择一种进化；不进化就关闭，并用底栏「结束回合」。",
		12,
		Styles.COLOR_MUTED
	))

	var actions: Variant = _view.get("legal_actions")
	var texts: Variant = _state.get("legal_action_texts")
	var group := ButtonGroup.new()
	var count := 0
	if actions is Array:
		for index in (actions as Array).size():
			var action_v: Variant = (actions as Array)[index]
			if not (action_v is Dictionary):
				continue
			var action: Dictionary = action_v
			if str(action.get("type", "")) == "end_turn":
				continue
			var label := JSON.stringify(action)
			if texts is Array and index < (texts as Array).size():
				label = str((texts as Array)[index])
			var button := Button.new()
			button.text = label
			button.toggle_mode = true
			button.button_group = group
			button.custom_minimum_size = Vector2(0, 36)
			Styles.style_button(button)
			button.pressed.connect(_select_evolution.bind(action))
			modal_options.add_child(button)
			count += 1
	if count == 0:
		modal_body.add_child(Widgets.label(
			"当前没有可执行的进化。", 13, Styles.COLOR_MUTED
		))


func _select_evolution(action: Dictionary) -> void:
	_evolution_action = action
	modal_confirm.disabled = false


func _confirm_evolution() -> void:
	if _evolution_action.is_empty():
		return
	var action := _evolution_action
	_close_modal()
	_send_action(action)


# ---- 暂停 / 结算 / 二次确认 ----------------------------------------------


func _open_pause_modal() -> void:
	_begin_modal("pause", "暂停")
	modal_cancel.visible = false
	modal_confirm.visible = false
	modal_body.add_child(Widgets.label(
		"对局仍保存在本地引擎里，可以随时从标题菜单继续。",
		12,
		Styles.COLOR_MUTED
	))
	_modal_option("继续游戏", _close_modal, true)
	_modal_option("重新开始", _request_restart)
	_modal_option("返回标题", _request_menu)


func _request_restart() -> void:
	_open_confirm_modal(
		"重新开始？",
		"当前对局会被放弃，并从同一组设置重新开局。",
		_restart_confirmed,
		"pause"
	)


func _request_menu() -> void:
	_open_confirm_modal(
		"返回标题？",
		"对局会保留在本地引擎中，可以从标题菜单「继续对局」回来。",
		_return_to_menu,
		"pause"
	)


func _restart_confirmed() -> void:
	_close_modal()
	_start_new_game()


func _return_to_menu() -> void:
	get_tree().change_scene_to_file(MENU_SCENE)


func _open_result_modal() -> void:
	var stalemate := bool(_state.get("stalemate", false))
	_begin_modal("result", "对局僵局" if stalemate else "对局结束")
	modal_cancel.visible = false
	modal_confirm.visible = false
	if stalemate:
		modal_body.add_child(Widgets.label(
			"供应区耗尽，当前玩家没有合法主要行动，本局无胜者。",
			14,
			Styles.COLOR_YELLOW
		))
	else:
		var winners: Variant = _view.get("winners")
		var parts: Array[String] = []
		if winners is Array:
			for winner_v in winners:
				var player_id := int(winner_v)
				parts.append("%s（%d 分）" % [
					_player_name(_view, player_id),
					_player_points(_view, player_id),
				])
		modal_body.add_child(Widgets.label(
			"胜者：%s" % "、".join(parts) if not parts.is_empty() else "对局结束，无胜者。",
			16,
			Styles.COLOR_GOLD
		))
	modal_body.add_child(Widgets.label(
		"种子 %d · 共 %d 回合 · 回放已自动保存到 tmp/" % [
			int(_state.get("seed", 0)),
			int(_view.get("turn_number", 0)),
		],
		12,
		Styles.COLOR_MUTED
	))
	_modal_option("再来一局", _restart_confirmed, true)
	_modal_option("保存回放", _on_save_pressed)
	_modal_option("返回标题", _return_to_menu)


func _open_confirm_modal(
	title: String, message: String, callback: Callable, return_to: String
) -> void:
	_begin_modal("confirm", title)
	_modal_return = return_to
	_modal_confirm_cb = callback
	modal_body.add_child(Widgets.label(message, 13, Styles.COLOR_TEXT))
	modal_cancel.text = "取消"
	modal_confirm.text = "确认"
	modal_cancel.grab_focus()


func _on_modal_cancel() -> void:
	if _modal == "confirm" and _modal_return == "pause":
		_open_pause_modal()
		return
	_close_modal()


func _on_modal_confirm() -> void:
	match _modal:
		"discard":
			_confirm_discard()
		"evolution":
			_confirm_evolution()
		"confirm":
			var callback := _modal_confirm_cb
			_modal_confirm_cb = Callable()
			if callback.is_valid():
				callback.call()
			else:
				_close_modal()
		_:
			_close_modal()


func _handle_escape() -> void:
	if _modal == "confirm":
		_on_modal_cancel()
		return
	if overlay.visible:
		_close_modal()
		return
	if _selected.x >= 0 or not _supply_pick.is_empty():
		_supply_pick.clear()
		_clear_selection()
		_render_supply()
		_render_dock()
		return
	if _connected and not _busy:
		_open_pause_modal()
# ---------------------------------------------------------------------------
# 保存、提示与查询
# ---------------------------------------------------------------------------


func _on_save_pressed() -> void:
	_save_replay(false)


func _save_replay(auto: bool) -> void:
	_request(HTTPClient.METHOD_POST, "/api/game/replay/save", null, _on_saved.bind(auto))


func _on_saved(ok: bool, _code: int, data: Dictionary, auto: bool) -> void:
	if not ok:
		_toast("保存回放失败：%s" % str(data.get("error", "未知错误")))
		return
	var replay: Variant = data.get("replay")
	if not (replay is Dictionary):
		_toast("保存回放失败：响应缺少 replay")
		return
	var path := str((replay as Dictionary).get("json", ""))
	_toast(("回放已自动保存：%s" if auto else "回放已写入：%s") % path.get_file())


func _show_error(data: Dictionary) -> void:
	_toast("错误：%s" % str(data.get("error", "请求失败")))


func _toast(message: String) -> void:
	toast_label.text = message
	toast_panel.visible = true
	toast_timer.start()
	_log_lines.append("⚠ " + message)
	_trim_log()


func _on_toast_timeout() -> void:
	toast_panel.visible = false


func _can_interact() -> bool:
	return (
		_connected
		and not _animating
		and not _busy
		and bool(_state.get("human_turn", false))
		and not bool(_state.get("completed", false))
		and str(_view.get("phase", "")) != "game_over"
	)


func _is_legal(action: Dictionary) -> bool:
	if action.is_empty():
		return false
	var actions: Variant = _view.get("legal_actions")
	if not (actions is Array):
		return false
	for candidate in actions:
		if candidate is Dictionary and _same_action(candidate, action):
			return true
	return false


func _same_action(a: Dictionary, b: Dictionary) -> bool:
	return JSON.stringify(a) == JSON.stringify(b)


func _token_total(tokens: Dictionary) -> int:
	var total := 0
	for key in Styles.COLOR_KEYS:
		total += int(tokens.get(key, 0))
	return total


func _player_name(view: Dictionary, player_id: int) -> String:
	var players: Variant = view.get("players")
	if players is Array:
		for player_v in players:
			if player_v is Dictionary and int(player_v.get("id", -1)) == player_id:
				return str(player_v.get("name", "玩家 %d" % player_id))
	return "玩家 %d" % player_id


func _player_points(view: Dictionary, player_id: int) -> int:
	var players: Variant = view.get("players")
	if players is Array:
		for player_v in players:
			if player_v is Dictionary and int(player_v.get("id", -1)) == player_id:
				return int(player_v.get("points", 0))
	return 0


func _player_tooltip(player: Dictionary) -> String:
	var lines: Array[String] = []
	lines.append("%s · %d 分" % [
		str(player.get("name", "?")),
		int(player.get("points", 0)),
	])
	var tokens: Dictionary = player.get("tokens", {})
	var token_parts: Array[String] = []
	for key in Styles.COLOR_KEYS:
		var count := int(tokens.get(key, 0))
		if count > 0:
			token_parts.append("%s%d" % [Styles.COLOR_LABELS[key], count])
	if token_parts.is_empty():
		token_parts.append("无")
	lines.append("精灵球：" + " ".join(token_parts))

	var bonuses: Dictionary = player.get("bonuses", {})
	var bonus_parts: Array[String] = []
	for key in COLOR_KEYS:
		var count := int(bonuses.get(key, 0))
		if count > 0:
			bonus_parts.append("%s%d" % [Styles.COLOR_LABELS[key], count])
	lines.append("减免：" + (" ".join(bonus_parts) if not bonus_parts.is_empty() else "无"))

	var captured_parts: Array[String] = []
	var captured: Variant = player.get("captured")
	if captured is Array:
		for card_v in captured:
			if card_v is Dictionary:
				captured_parts.append("%s(%d)" % [
					str(card_v.get("name_zh", "?")),
					int(card_v.get("points", 0)),
				])
	lines.append("捕捉：%s" % (
		"、".join(captured_parts) if not captured_parts.is_empty() else "无"
	))

	var reserved_count := int(player.get("reserved_count", 0))
	var reserved_parts: Array[String] = []
	var reserved: Variant = player.get("reserved")
	if reserved is Array:
		for card_v in reserved:
			if card_v is Dictionary:
				reserved_parts.append(str(card_v.get("name_zh", "?")))
	var hidden := maxi(0, reserved_count - reserved_parts.size())
	lines.append("保留：%s" % (
		"、".join(reserved_parts) + ("、%d 张未公开" % hidden if hidden > 0 else "")
		if not reserved_parts.is_empty()
		else ("%d 张未公开" % hidden if hidden > 0 else "无")
	))
	return "\n".join(lines)


func _card_tooltip(card: Dictionary) -> String:
	var lines: Array[String] = []
	lines.append("%s · %d 分" % [
		str(card.get("name_zh", "?")),
		int(card.get("points", 0)),
	])
	var bonus_key := str(card.get("bonus_color", "red"))
	lines.append("减免 %s +%d" % [
		Styles.COLOR_LABELS.get(bonus_key, "?"),
		int(card.get("bonus_count", 0)),
	])
	var cost: Dictionary = {}
	var cost_raw: Variant = card.get("cost", {})
	if cost_raw is Dictionary:
		cost = cost_raw
	var parts: Array[String] = []
	for key in Styles.COLOR_KEYS:
		var count := int(cost.get(key, 0))
		if count > 0:
			parts.append("%s%d" % [Styles.COLOR_LABELS.get(key, "?"), count])
	lines.append("费用：" + (" ".join(parts) if not parts.is_empty() else "免费"))
	return "\n".join(lines)
# ---------------------------------------------------------------------------
# 无头冒烟
# ---------------------------------------------------------------------------


func _run_smoke() -> void:
	var env_url := OS.get_environment("SPLENDOR_SERVER_URL")
	if not env_url.is_empty():
		_base_url = env_url.trim_suffix("/")
	var health: Dictionary = await _smoke_request(HTTPClient.METHOD_GET, "/api/health", null)
	if not bool(health.get("ok", false)):
		_smoke_fail("本地引擎不可用：%s" % str(health))
		return
	_connected = true

	var created: Dictionary = await _smoke_request(
		HTTPClient.METHOD_POST,
		"/api/game/new",
		{"seed": 7, "bots": 2, "bot": "heuristic", "name": "你"}
	)
	if not bool(created.get("ok", false)):
		_smoke_fail("开局失败：%s" % str(created))
		return
	var state: Variant = (created["data"] as Dictionary).get("state")
	if not (state is Dictionary):
		_smoke_fail("开局响应缺少 state")
		return
	_apply_state(state, true)
	await get_tree().process_frame
	await get_tree().process_frame

	var failures: Array[String] = []
	if opponents_box.get_child_count() != 2:
		failures.append("对手面板应为 2 个，实际 %d" % opponents_box.get_child_count())
	if board_view.board().is_empty():
		failures.append("BoardView 未注入棋盘")
	else:
		var level1: Variant = board_view.board().get("level1")
		if not (level1 is Array) or (level1 as Array).size() != 4:
			failures.append("1 阶行应有 4 个槽位")
	if prompt_label.text.is_empty():
		failures.append("行动提示为空")
	if supply_view.size.y < 40.0:
		failures.append("供应区高度异常：%.0f" % supply_view.size.y)
	if end_turn_button.visible:
		failures.append("主要行动阶段不应显示结束回合")

	var sample := {"red": 2, "blue": 1, "yellow": 0, "pink": 0, "black": 0, "master": 3}
	if _token_total(sample) != 6:
		failures.append("球数统计应包含大师球且只计一次，实际 %d" % _token_total(sample))
	_supply_pick = {"red": 1, "blue": 1, "yellow": 1}
	if str(_take_action().get("type", "")) != "take_three":
		failures.append("三种不同颜色应拼成 take_three")
	_supply_pick = {"red": 2}
	if str(_take_action().get("type", "")) != "take_two":
		failures.append("同色 2 个应拼成 take_two")
	_supply_pick.clear()

	var take := _smoke_find_take()
	if take.is_empty():
		failures.append("没有找到合法拿球动作")
	else:
		_smoke_fill_pick(take)
		_render_supply()
		_render_dock()
		if confirm_button.disabled:
			failures.append("拿球已选满但确认按钮仍禁用")
		if not _same_action(_take_action(), take):
			failures.append("界面拼出的拿球动作与引擎动作不一致")

	var slot := _smoke_first_actionable_slot()
	if slot.x < 0:
		failures.append("没有找到可操作的场上牌")
	else:
		var slots: Variant = board_view.board().get(BOARD_CATEGORIES[slot.x])
		var card: Variant = (slots as Array)[slot.y]
		_on_slot_clicked(slot.x, slot.y, card)
		if not context_actions.visible:
			failures.append("选中卡片后动作条未显示")
		if capture_button.text.is_empty() or reserve_button.text.is_empty():
			failures.append("动作条按钮文案为空")
		_clear_selection()

	if not take.is_empty():
		var acted: Dictionary = await _smoke_request(
			HTTPClient.METHOD_POST, "/api/game/action", {"action": take}
		)
		if not bool(acted.get("ok", false)):
			failures.append("拿球动作失败：%s" % str(acted))
		else:
			_on_action(true, 200, acted["data"])
			_smoke_drain_steps()
			if str(_view.get("phase", "")) != "evolution":
				failures.append("拿球后应进入进化阶段，实际 %s" % str(_view.get("phase", "")))
			if not end_turn_button.visible:
				failures.append("进化阶段应显示结束回合")
			if not aux_button.visible:
				failures.append("进化阶段应显示进化按钮")
			_open_evolution_modal()
			await get_tree().process_frame
			if not overlay.visible:
				failures.append("进化模态未打开")
			_close_modal()

		var ended: Dictionary = await _smoke_request(
			HTTPClient.METHOD_POST, "/api/game/action", {"action": {"type": "end_turn"}}
		)
		if not bool(ended.get("ok", false)):
			failures.append("结束回合失败：%s" % str(ended))
		else:
			var steps: Variant = (ended["data"] as Dictionary).get("steps")
			if not (steps is Array) or (steps as Array).size() < 2:
				failures.append("结束回合后应包含至少 2 个 AI 动作")
			_on_action(true, 200, ended["data"])
			_smoke_drain_steps()
			if int(_view.get("current_player", -1)) != 0:
				failures.append("AI 回合后应轮回到人类")
			if str(_view.get("phase", "")) != "main_action":
				failures.append("AI 回合后应回到主要行动阶段")

	_begin_modal("discard", "弃球")
	_build_discard_modal()
	await get_tree().process_frame
	if modal_body.get_child_count() < 4:
		failures.append("弃球模态缺少球架")
	if not modal_confirm.disabled:
		failures.append("未选弃球时确认按钮应禁用")
	_close_modal()

	_open_pause_modal()
	await get_tree().process_frame
	if not overlay.visible:
		failures.append("暂停菜单未打开")
	if modal_options.get_child_count() != 3:
		failures.append("暂停菜单应有 3 个选项")
	_handle_escape()
	if overlay.visible:
		failures.append("Esc 未关闭暂停菜单")

	_open_result_modal()
	await get_tree().process_frame
	if modal_options.get_child_count() != 3:
		failures.append("结算面板应有 3 个选项")
	_close_modal()

	_open_confirm_modal("测试确认", "确认对话框", func(): pass, "")
	if modal_cancel.text != "取消" or modal_confirm.text != "确认":
		failures.append("确认对话框按钮文案异常")
	_on_modal_cancel()
	if overlay.visible:
		failures.append("确认对话框未关闭")

	var theme_font := get_theme_default_font()
	if theme_font == null or not theme_font.has_char("璀".unicode_at(0)):
		failures.append("默认字体缺少中文字形")

	if failures.is_empty():
		print("[battle-smoke] OK 对手=%d 棋盘=%s 供应=%s 记录=%d 字" % [
			opponents_box.get_child_count(),
			str(board_view.size),
			str(supply_view.size),
			events_label.text.length(),
		])
		get_tree().quit(0)
		return
	for message in failures:
		printerr("[battle-smoke] FAIL " + message)
	get_tree().quit(1)


func _smoke_find_take() -> Dictionary:
	var actions: Variant = _view.get("legal_actions")
	if not (actions is Array):
		return {}
	var fallback: Dictionary = {}
	for candidate in actions:
		if not (candidate is Dictionary):
			continue
		var action: Dictionary = candidate
		var action_type := str(action.get("type", ""))
		if action_type == "take_three":
			return action.duplicate()
		if action_type == "take_two" and fallback.is_empty():
			fallback = action.duplicate()
	return fallback


func _smoke_fill_pick(action: Dictionary) -> void:
	_supply_pick.clear()
	var action_type := str(action.get("type", ""))
	if action_type == "take_two":
		_supply_pick[str(action.get("color", ""))] = 2
	elif action_type == "take_three":
		var colors: Variant = action.get("colors", [])
		if colors is Array:
			for color in colors:
				var key := str(color)
				_supply_pick[key] = int(_supply_pick.get(key, 0)) + 1


func _smoke_first_actionable_slot() -> Vector2i:
	var map := _actionable_map()
	for key in map:
		var parts := str(key).split(":")
		if parts.size() == 2:
			return Vector2i(int(parts[0]), int(parts[1]))
	return Vector2i(-1, -1)


func _smoke_fail(message: String) -> void:
	printerr("[battle-smoke] FAIL " + message)
	get_tree().quit(1)


func _smoke_drain_steps() -> void:
	step_timer.stop()
	var guard := 0
	while _animating and guard < 200:
		_advance_step()
		step_timer.stop()
		guard += 1


func _smoke_request(method: int, path: String, body: Variant) -> Dictionary:
	var request := HTTPRequest.new()
	request.timeout = 60.0
	add_child(request)
	var payload := ""
	if body != null:
		payload = JSON.stringify(body)
	var error := request.request(
		_base_url + path,
		["Content-Type: application/json"],
		method,
		payload
	)
	if error != OK:
		request.queue_free()
		return {"ok": false, "code": 0, "data": {}, "error": "request %d" % error}
	var completed: Array = await request.request_completed
	request.queue_free()
	var result := int(completed[0])
	var code := int(completed[1])
	var raw: PackedByteArray = completed[3]
	var data: Dictionary = {}
	var parsed: Variant = _normalize_numbers(JSON.parse_string(raw.get_string_from_utf8()))
	if parsed is Dictionary:
		data = parsed
	var ok := (
		result == HTTPRequest.RESULT_SUCCESS
		and code >= 200
		and code < 300
		and bool(data.get("ok", true))
	)
	return {"ok": ok, "code": code, "data": data}