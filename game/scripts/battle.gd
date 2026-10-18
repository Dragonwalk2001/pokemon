extends Control
## 和 AI 对战：Godot 交互界面，规则与 bot 全部由本地 Rust 引擎执行。
##
## 数据通道是 `splendor-pokemon serve` 暴露的 127.0.0.1 JSON HTTP 接口。
## 界面只发送动作、渲染引擎返回的 `GameView`，不复制任何规则：
## - 人类固定玩家 0；每次动作后服务端连续推进 AI 回合，直到再次轮到人类。
## - 私密信息由 `view_for(0)` 决定，Godot 不接触完整 `Game`。
## - 对局结束自动把回放写入仓库 `tmp/pve-<seed>-<n>p.{json,html}`。
##
## 启动顺序：优先连接 `SPLENDOR_SERVER_URL`；否则尝试探测默认端口，失败时
## 启动 `backend/target/{debug,release}/splendor-pokemon[.exe]`，再退回 `cargo run`。
## 环境变量 `SPLENDOR_BATTLE_SMOKE=1` 时执行无头冒烟，退出码 0 表示通过。

const Styles := preload("res://scripts/ui/styles.gd")
const Widgets := preload("res://scripts/ui/widgets.gd")

const PORT := 43110
const DEFAULT_BASE_URL := "http://127.0.0.1:43110"
const CONNECT_TIMEOUT := 300.0
const BOT_OPTIONS := [
	{"id": "minimax", "label": "极小化极大"},
	{"id": "heuristic", "label": "启发式"},
	{"id": "random", "label": "随机"},
]
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
const CATEGORY_LABELS := {
	"level1": "1阶",
	"level2": "2阶",
	"level3": "3阶",
	"rare": "稀有",
	"legendary": "传说",
}

@onready var meta_label: Label = $Margin/Root/TopBar/Meta
@onready var bots_option: OptionButton = $Margin/Root/TopBar/BotsOption
@onready var bot_option: OptionButton = $Margin/Root/TopBar/BotOption
@onready var seed_spin: SpinBox = $Margin/Root/TopBar/SeedSpin
@onready var new_button: Button = $Margin/Root/TopBar/NewButton
@onready var replay_button: Button = $Margin/Root/TopBar/ReplayButton
@onready var board_panel: PanelContainer = $Margin/Root/Body/Left/BoardPanel
@onready var board_view: Control = $Margin/Root/Body/Left/BoardPanel/VBox/BoardView
@onready var players_panel: PanelContainer = $Margin/Root/Body/Left/PlayersPanel
@onready var players_row: HBoxContainer = $Margin/Root/Body/Left/PlayersPanel/VBox/PlayersRow
@onready var reserved_panel: PanelContainer = $Margin/Root/Body/Left/ReservedPanel
@onready var reserved_row: HFlowContainer = $Margin/Root/Body/Left/ReservedPanel/VBox/ReservedRow
@onready var supply_panel: PanelContainer = $Margin/Root/Body/Right/SupplyPanel
@onready var supply_list: VBoxContainer = $Margin/Root/Body/Right/SupplyPanel/VBox/SupplyList
@onready var selection_label: Label = $Margin/Root/Body/Right/SupplyPanel/VBox/SelectionLabel
@onready var clear_button: Button = $Margin/Root/Body/Right/SupplyPanel/VBox/SupplyActions/ClearButton
@onready var take_button: Button = $Margin/Root/Body/Right/SupplyPanel/VBox/SupplyActions/TakeButton
@onready var action_panel: PanelContainer = $Margin/Root/Body/Right/ActionPanel
@onready var phase_label: Label = $Margin/Root/Body/Right/ActionPanel/VBox/PhaseLabel
@onready var hint_label: Label = $Margin/Root/Body/Right/ActionPanel/VBox/HintLabel
@onready var action_list: VBoxContainer = $Margin/Root/Body/Right/ActionPanel/VBox/ActionList
@onready var events_label: RichTextLabel = $Margin/Root/Body/Right/ActionPanel/VBox/Events
@onready var banner_panel: PanelContainer = $Margin/Root/BannerPanel
@onready var banner_label: Label = $Margin/Root/BannerPanel/BannerRow/BannerLabel
@onready var save_button: Button = $Margin/Root/BannerPanel/BannerRow/SaveButton
@onready var again_button: Button = $Margin/Root/BannerPanel/BannerRow/AgainButton
@onready var api_request: HTTPRequest = $ApiRequest
@onready var step_timer: Timer = $StepTimer
@onready var connect_timer: Timer = $ConnectTimer
@onready var toast_timer: Timer = $ToastTimer

var _view: Dictionary = {}
var _state: Dictionary = {}
var _log_lines: Array = []
var _selection: Dictionary = {}
var _discard_selection: Dictionary = {}
var _selected := Vector2i(-1, -1)
var _supply_buttons: Dictionary = {}
var _master_label: Label

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
	get_window().min_size = Vector2i(1200, 760)
	_apply_styles()
	_build_options()
	_build_supply()
	_wire_signals()
	board_view.set_interactive(false)
	if OS.get_environment("SPLENDOR_BATTLE_SMOKE") == "1":
		_run_smoke()
		return
	_connect()


func _exit_tree() -> void:
	if _server_pid > 0:
		OS.kill(_server_pid)


func _apply_styles() -> void:
	for button in [new_button, replay_button, clear_button, save_button]:
		Styles.style_button(button)
		button.focus_mode = Control.FOCUS_NONE
	Styles.style_button(bots_option)
	Styles.style_button(bot_option)
	bots_option.focus_mode = Control.FOCUS_NONE
	bot_option.focus_mode = Control.FOCUS_NONE
	Styles.style_button(take_button, true)
	Styles.style_button(again_button, true)
	take_button.focus_mode = Control.FOCUS_NONE
	again_button.focus_mode = Control.FOCUS_NONE
	for panel in [board_panel, players_panel, reserved_panel, supply_panel, action_panel]:
		panel.add_theme_stylebox_override("panel", Styles.panel_box())
	events_label.add_theme_color_override("default_color", Styles.COLOR_MUTED)
	events_label.add_theme_font_size_override("normal_font_size", 12)
	events_label.add_theme_font_size_override("bold_font_size", 12)
	hint_label.add_theme_color_override("font_color", Styles.COLOR_MUTED)


func _build_options() -> void:
	bots_option.add_item("1 人 + 2 AI", 2)
	bots_option.add_item("1 人 + 3 AI", 3)
	bots_option.select(0)
	for option in BOT_OPTIONS:
		var index := bot_option.item_count
		bot_option.add_item(str(option["label"]), index)
		bot_option.set_item_metadata(index, str(option["id"]))
	bot_option.select(0)


func _build_supply() -> void:
	for key in COLOR_KEYS:
		var button := Button.new()
		button.alignment = HORIZONTAL_ALIGNMENT_LEFT
		button.custom_minimum_size = Vector2(0, 34)
		button.focus_mode = Control.FOCUS_NONE
		var color: Color = Styles.TOKEN_COLORS[key]
		button.add_theme_stylebox_override(
			"normal", Styles.button_box(Styles.COLOR_PANEL_2.lerp(color, 0.16), color.darkened(0.25))
		)
		button.add_theme_stylebox_override(
			"hover", Styles.button_box(Styles.COLOR_PANEL_2.lerp(color, 0.30), color)
		)
		button.add_theme_stylebox_override(
			"pressed", Styles.button_box(Styles.COLOR_PANEL.lerp(color, 0.20), color)
		)
		button.add_theme_stylebox_override(
			"disabled", Styles.button_box(Styles.COLOR_PANEL, Styles.COLOR_LINE)
		)
		button.add_theme_color_override("font_color", Styles.COLOR_TEXT)
		button.add_theme_color_override("font_hover_color", Color.WHITE)
		button.add_theme_color_override("font_disabled_color", Color(0.45, 0.50, 0.60))
		button.add_theme_font_size_override("font_size", 14)
		button.pressed.connect(_on_supply_color.bind(key))
		supply_list.add_child(button)
		_supply_buttons[key] = button
	_master_label = Widgets.label("大师球 0", 12, Styles.COLOR_MUTED)
	supply_list.add_child(_master_label)


func _wire_signals() -> void:
	new_button.pressed.connect(_start_new_game)
	again_button.pressed.connect(_start_new_game)
	replay_button.pressed.connect(_on_open_replay)
	save_button.pressed.connect(_on_save_pressed)
	clear_button.pressed.connect(_on_clear_supply)
	take_button.pressed.connect(_on_take_pressed)
	board_view.slot_clicked.connect(_on_slot_clicked)
	board_view.slot_right_clicked.connect(_on_slot_right_clicked)
	api_request.request_completed.connect(_on_api_completed)
	step_timer.timeout.connect(_on_step_tick)
	connect_timer.timeout.connect(_on_connect_tick)
	toast_timer.timeout.connect(_on_toast_timeout)


func _on_open_replay() -> void:
	get_tree().change_scene_to_file("res://scenes/main.tscn")



# ---------------------------------------------------------------------------
# 连接本地引擎
# ---------------------------------------------------------------------------


func _connect() -> void:
	_connected = false
	_launch_attempted = false
	_connect_deadline = Time.get_ticks_msec() / 1000.0 + CONNECT_TIMEOUT
	meta_label.text = "正在连接本地引擎…"
	meta_label.tooltip_text = _base_url
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
		meta_label.text = "本地引擎已连接，正在开局…"
		_start_new_game()
		return

	if Time.get_ticks_msec() / 1000.0 > _connect_deadline:
		connect_timer.stop()
		meta_label.text = "本地引擎未连接：请先构建后端，或在 backend/ 运行 cargo run -p splendor-pokemon-cli -- serve"
		return

	if may_launch and not _launch_attempted:
		_launch_attempted = true
		if not _launch_server():
			connect_timer.stop()
			return
	connect_timer.start()


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
	save_button.text = "保存回放"
	var bots := 2
	if bots_option.selected >= 0:
		bots = bots_option.get_item_id(bots_option.selected)
	var bot_id := "minimax"
	if bot_option.selected >= 0:
		bot_id = str(bot_option.get_item_metadata(bot_option.selected))
	var seed := int(seed_spin.value)
	_selection.clear()
	_discard_selection.clear()
	_selected = Vector2i(-1, -1)
	_replay_saved = false
	meta_label.text = "正在开局…"
	_request(
		HTTPClient.METHOD_POST,
		"/api/game/new",
		{"seed": seed, "bots": bots, "bot": bot_id, "name": "你"},
		_on_new_game
	)
	_refresh_controls()


func _on_new_game(ok: bool, _code: int, data: Dictionary) -> void:
	if not ok:
		_show_error(data)
		return
	var state: Variant = data.get("state")
	if not (state is Dictionary):
		_show_error({"error": "开局响应缺少 state"})
		return
	_state = state
	_step_queue.clear()
	_final_state = {}
	_animating = false
	_apply_state(_state, true)


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
	_refresh_controls()


func _on_action(ok: bool, _code: int, data: Dictionary) -> void:
	if not ok:
		_refresh_controls()
		_show_error(data)
		return
	_selection.clear()
	_discard_selection.clear()
	_selected = Vector2i(-1, -1)
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
	meta_label.text = "AI 行动中…"
	_advance_step()


func _advance_step() -> void:
	if _step_queue.is_empty():
		_animating = false
		if not _final_state.is_empty():
			_apply_state(_final_state, true)
		else:
			_refresh_controls()
		return
	var step: Dictionary = _step_queue.pop_front()
	var view: Variant = step.get("view")
	if view is Dictionary:
		_apply_view(view)
	_append_step(step)
	var wait := 0.30 if int(step.get("actor", 0)) == 0 else 0.45
	step_timer.start(wait)


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
	phase_label.text = "%s · %s" % [_player_name(_view, actor), text]


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
	if bool(_state.get("completed", false)) and not _replay_saved:
		_replay_saved = true
		_save_replay(true)


func _trim_log() -> void:
	var limit := 240
	if _log_lines.size() > limit:
		_log_lines = _log_lines.slice(_log_lines.size() - limit)
	events_label.text = "\n".join(_log_lines)



# ---------------------------------------------------------------------------
# 渲染
# ---------------------------------------------------------------------------


func _apply_view(view: Dictionary) -> void:
	_view = view
	board_view.set_view(view.get("board", {}))
	board_view.set_selected(_selected)
	board_view.set_interactive(_can_interact() and str(view.get("phase", "")) == "main_action")
	_render_players()
	_render_reserved()
	_refresh_supply()
	_render_actions()
	_render_banner()
	_render_meta()


func _render_players() -> void:
	Widgets.clear(players_row)
	var players: Variant = _view.get("players")
	if not (players is Array):
		return
	var current := int(_view.get("current_player", -1))
	var game_over := str(_view.get("phase", "")) == "game_over"
	for player_v in players:
		if player_v is Dictionary:
			players_row.add_child(_build_player_panel(player_v, current, game_over))


func _build_player_panel(player: Dictionary, current_id: int, game_over: bool) -> PanelContainer:
	var player_id := int(player.get("id", 0))
	var is_current := player_id == current_id and not game_over
	var panel := Widgets.panel(
		Styles.panel_box(
			Styles.COLOR_PANEL_2,
			Styles.COLOR_ACCENT if is_current else Styles.COLOR_LINE,
			10,
			2 if is_current else 1
		)
	)
	panel.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	panel.custom_minimum_size = Vector2(170, 0)

	var vbox := VBoxContainer.new()
	vbox.add_theme_constant_override("separation", 5)
	panel.add_child(vbox)

	var head := HBoxContainer.new()
	head.add_theme_constant_override("separation", 7)
	vbox.add_child(head)
	head.add_child(Widgets.label(str(player.get("name", "?")), 16, Styles.COLOR_TEXT))
	if is_current:
		head.add_child(Widgets.badge("行动中"))
	head.add_child(Widgets.spacer())
	var score := Widgets.label(str(int(player.get("points", 0))), 21, Styles.COLOR_GOLD)
	score.vertical_alignment = VERTICAL_ALIGNMENT_CENTER
	head.add_child(score)
	head.add_child(Widgets.label("分", 11, Styles.COLOR_MUTED))

	var token_row := HFlowContainer.new()
	token_row.add_theme_constant_override("h_separation", 4)
	token_row.add_theme_constant_override("v_separation", 3)
	vbox.add_child(token_row)
	var tokens: Dictionary = player.get("tokens", {})
	for key in Styles.COLOR_KEYS:
		var count := int(tokens.get(key, 0))
		token_row.add_child(
			Widgets.pill(
				"%s %d" % [Styles.COLOR_LABELS[key], count], Styles.TOKEN_COLORS[key], count == 0
			)
		)
	var master_count := int(tokens.get("master", 0))
	token_row.add_child(
		Widgets.pill("大师 %d" % master_count, Styles.COLOR_MASTER, master_count == 0)
	)

	var stats_row := HFlowContainer.new()
	stats_row.add_theme_constant_override("h_separation", 4)
	stats_row.add_theme_constant_override("v_separation", 3)
	vbox.add_child(stats_row)
	stats_row.add_child(Widgets.chip("减免"))
	var bonuses: Dictionary = player.get("bonuses", {})
	for key in COLOR_KEYS:
		var count := int(bonuses.get(key, 0))
		if count > 0:
			stats_row.add_child(Widgets.pill(str(count), Styles.TOKEN_COLORS[key]))
	stats_row.add_child(Widgets.chip("捕捉 %d" % int(player.get("card_count", 0))))
	stats_row.add_child(Widgets.chip("保留 %d / 3" % int(player.get("reserved_count", 0))))
	stats_row.add_child(Widgets.chip("进化 %d" % int(player.get("evolution_count", 0))))

	var cards_row := HFlowContainer.new()
	cards_row.add_theme_constant_override("h_separation", 4)
	cards_row.add_theme_constant_override("v_separation", 3)
	vbox.add_child(cards_row)
	var captured: Array = []
	var captured_v: Variant = player.get("captured")
	if captured_v is Array:
		captured = (captured_v as Array).duplicate()
		captured.sort_custom(
			func(a, b): return int(a.get("points", 0)) > int(b.get("points", 0))
		)
	for i in mini(3, captured.size()):
		if captured[i] is Dictionary:
			cards_row.add_child(Widgets.card_chip(captured[i]))
	if captured.size() > 3:
		cards_row.add_child(Widgets.chip("… +%d" % (captured.size() - 3)))
	if captured.is_empty():
		cards_row.add_child(Widgets.chip("尚无宝可梦"))

	panel.tooltip_text = _player_tooltip(player)
	return panel



func _render_reserved() -> void:
	Widgets.clear(reserved_row)
	var players: Variant = _view.get("players")
	if not (players is Array) or (players as Array).is_empty():
		return
	var me: Dictionary = players[0]
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
		var button := Button.new()
		button.text = "捕捉 · %s %d分" % [
			str(card.get("name_zh", "?")),
			int(card.get("points", 0)),
		]
		button.focus_mode = Control.FOCUS_NONE
		button.tooltip_text = _card_tooltip(card)
		var color: Color = Styles.TOKEN_COLORS.get(
			str(card.get("bonus_color", "red")), Styles.COLOR_ACCENT
		)
		button.add_theme_stylebox_override(
			"normal", Styles.button_box(Styles.COLOR_PANEL_2.lerp(color, 0.18), color.darkened(0.25))
		)
		button.add_theme_stylebox_override(
			"hover", Styles.button_box(Styles.COLOR_PANEL_2.lerp(color, 0.32), color)
		)
		button.pressed.connect(_on_reserved_pressed.bind(index))
		button.disabled = not _is_legal(_capture_reserved(index))
		reserved_row.add_child(button)


func _render_meta() -> void:
	var seed := int(_state.get("seed", 0))
	var bots := int(_state.get("bots", 0))
	var bot_id := str(_state.get("bot", "minimax"))
	var turn := int(_view.get("turn_number", 0))
	if _connected:
		meta_label.text = "本地引擎 · 种子 %d · 你 vs %d 个%s bot · 第 %d 回合" % [
			seed,
			bots,
			BOT_LABELS.get(bot_id, bot_id),
			turn,
		]
	meta_label.tooltip_text = _base_url


func _render_banner() -> void:
	var completed := bool(_state.get("completed", false))
	var stalemate := bool(_state.get("stalemate", false))
	if not completed:
		banner_panel.visible = false
		return
	if stalemate:
		banner_panel.add_theme_stylebox_override("panel", Styles.banner_box(Styles.COLOR_YELLOW))
		banner_label.add_theme_color_override("font_color", Color("ffe2a0"))
		banner_label.text = "对局僵局 · 供应区耗尽且当前玩家没有合法主行动（无胜者）"
		banner_panel.visible = true
		return
	var winners: Variant = _view.get("winners")
	if not (winners is Array) or (winners as Array).is_empty():
		banner_panel.visible = false
		return
	var parts: Array[String] = []
	for winner_v in winners:
		var player_id := int(winner_v)
		parts.append(
			"%s（%d 分）" % [_player_name(_view, player_id), _player_points(_view, player_id)]
		)
	banner_panel.add_theme_stylebox_override("panel", Styles.banner_box(Styles.COLOR_GOLD))
	banner_label.add_theme_color_override("font_color", Color("ffe2a0"))
	banner_label.text = "对局结束 · 胜者：%s" % "、".join(parts)
	banner_panel.visible = true



# ---------------------------------------------------------------------------
# 供应区与主要行动
# ---------------------------------------------------------------------------


func _refresh_supply() -> void:
	var supply: Dictionary = _view.get("supply", {})
	var phase := str(_view.get("phase", ""))
	var enabled := _can_interact() and phase == "main_action"
	for key in COLOR_KEYS:
		var button: Button = _supply_buttons[key]
		var count := int(supply.get(key, 0))
		var picked := int(_selection.get(key, 0))
		button.text = "%s  供应 %d" % [Styles.COLOR_LABELS[key], count]
		if picked > 0:
			button.text += "   · 选 %d" % picked
		button.disabled = not enabled or count == 0
	_master_label.text = "大师球 供应 %d（不能主动拿取）" % int(supply.get("master", 0))
	_refresh_selection_label()
	take_button.disabled = not enabled or _take_action().is_empty() or not _is_legal(_take_action())
	clear_button.disabled = _selection.is_empty()


func _refresh_selection_label() -> void:
	if _selection.is_empty():
		selection_label.text = "尚未选择精灵球"
		return
	var parts: Array[String] = []
	for key in COLOR_KEYS:
		var count := int(_selection.get(key, 0))
		if count > 0:
			parts.append("%s%d" % [Styles.COLOR_LABELS[key], count])
	selection_label.text = "已选：%s" % " ".join(parts)


func _on_supply_color(key: String) -> void:
	if not _can_interact() or str(_view.get("phase", "")) != "main_action":
		return
	var supply: Dictionary = _view.get("supply", {})
	var available := int(supply.get(key, 0))
	var limit := mini(2, available)
	var current := int(_selection.get(key, 0))
	var next := current + 1
	if next > limit:
		next = 0
	if next == 0:
		_selection.erase(key)
	else:
		_selection[key] = next
	_refresh_supply()


func _on_clear_supply() -> void:
	_selection.clear()
	_refresh_supply()


func _take_action() -> Dictionary:
	var colors: Array[String] = []
	var total := 0
	var two_color := ""
	for key in COLOR_KEYS:
		var count := int(_selection.get(key, 0))
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


func _on_take_pressed() -> void:
	var action := _take_action()
	if action.is_empty():
		_toast("请选择 3 种不同颜色各 1 个，或 1 种颜色 2 个")
		return
	_send_action(action)


func _on_slot_clicked(row: int, slot: int, _card: Dictionary) -> void:
	_selected = Vector2i(row, slot)
	board_view.set_selected(_selected)
	_render_actions()


func _on_slot_right_clicked(row: int, slot: int, _card: Dictionary) -> void:
	var action := {
		"type": "reserve",
		"source": {
			"type": "board",
			"category": BOARD_CATEGORIES[row],
			"index": slot,
		},
	}
	if not _is_legal(action):
		_toast("这张牌现在不能保留")
		return
	_send_action(action)


func _on_capture_selected() -> void:
	if _selected.x < 0:
		return
	_send_action(_capture_board(_selected.x, _selected.y))


func _on_reserve_selected() -> void:
	if _selected.x < 0:
		return
	_send_action(_reserve_board(_selected.x, _selected.y))


func _on_reserved_pressed(index: int) -> void:
	_send_action(_capture_reserved(index))


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
# 行动面板：选择、弃球、进化
# ---------------------------------------------------------------------------


func _render_actions() -> void:
	Widgets.clear(action_list)
	hint_label.add_theme_color_override("font_color", Styles.COLOR_MUTED)
	var phase := str(_view.get("phase", ""))
	var text := str(PHASES.get(phase, phase))
	if phase == "main_action":
		text = "第 %d 回合 · 主要行动" % int(_view.get("turn_number", 0))
	elif phase == "evolution":
		text = "第 %d 回合 · 进化 / 结束回合" % int(_view.get("turn_number", 0))
	phase_label.text = text if not text.is_empty() else "等待开局"

	if bool(_state.get("completed", false)) or phase == "game_over":
		hint_label.text = "对局已结束。可以保存回放，或点「再来一局」。"
		return
	if _animating or _busy:
		hint_label.text = "正在等待本地引擎…"
		return
	if not _can_interact():
		hint_label.text = "等待本地引擎返回当前回合…"
		return

	match phase:
		"main_action":
			_render_main_action()
		"discard":
			_render_discard()
		"evolution":
			_render_evolution()
		_:
			hint_label.text = "未知阶段：%s" % phase


func _render_main_action() -> void:
	hint_label.text = "点击场上牌选择捕捉或保留；右键可直接保留。拿球请在右侧供应区选择。"
	var card := _selected_card()
	if card.is_empty():
		return
	var capture := _capture_board(_selected.x, _selected.y)
	var reserve := _reserve_board(_selected.x, _selected.y)
	var head := Widgets.label("已选：%s（%d 分）" % [
		str(card.get("name_zh", "?")),
		int(card.get("points", 0)),
	], 14, Styles.COLOR_TEXT)
	action_list.add_child(head)
	var row := HBoxContainer.new()
	row.add_theme_constant_override("separation", 6)
	action_list.add_child(row)
	var capture_button := Button.new()
	capture_button.text = "捕捉"
	Styles.style_button(capture_button, true)
	capture_button.focus_mode = Control.FOCUS_NONE
	capture_button.disabled = not _is_legal(capture)
	capture_button.pressed.connect(_on_capture_selected)
	row.add_child(capture_button)
	var reserve_button := Button.new()
	reserve_button.text = "保留（拿 1 大师球）"
	Styles.style_button(reserve_button)
	reserve_button.focus_mode = Control.FOCUS_NONE
	reserve_button.disabled = not _is_legal(reserve)
	reserve_button.pressed.connect(_on_reserve_selected)
	row.add_child(reserve_button)
	if capture_button.disabled:
		hint_label.text = "这张牌暂时捕捉不了（精灵球不足），右键可尝试保留。"


func _render_discard() -> void:
	var players: Variant = _view.get("players")
	if not (players is Array) or (players as Array).is_empty():
		return
	var me: Dictionary = players[0]
	var tokens: Dictionary = me.get("tokens", {})
	var held := _token_total(tokens)
	var required := maxi(0, held - 10)
	var picked := _token_total(_discard_selection)
	hint_label.text = "球数 %d 超过上限 10：还需弃 %d 个。点击颜色循环 0 → 持有数 → 0。" % [
		held,
		required,
	]
	for key in Styles.COLOR_KEYS:
		var owned := int(tokens.get(key, 0))
		if owned <= 0:
			continue
		var chosen := int(_discard_selection.get(key, 0))
		var button := Button.new()
		button.text = "%s 持有 %d · 弃 %d" % [Styles.COLOR_LABELS[key], owned, chosen]
		button.focus_mode = Control.FOCUS_NONE
		var color: Color = Styles.TOKEN_COLORS[key]
		button.add_theme_stylebox_override(
			"normal", Styles.button_box(Styles.COLOR_PANEL_2.lerp(color, 0.18), color.darkened(0.25))
		)
		button.add_theme_stylebox_override(
			"hover", Styles.button_box(Styles.COLOR_PANEL_2.lerp(color, 0.32), color)
		)
		button.pressed.connect(_on_discard_color.bind(key, owned))
		action_list.add_child(button)
	var master_owned := int(tokens.get("master", 0))
	if master_owned > 0:
		var master_chosen := int(_discard_selection.get("master", 0))
		var master_button := Button.new()
		master_button.text = "大师 持有 %d · 弃 %d" % [master_owned, master_chosen]
		master_button.focus_mode = Control.FOCUS_NONE
		master_button.add_theme_stylebox_override(
			"normal",
			Styles.button_box(
				Styles.COLOR_PANEL_2.lerp(Styles.COLOR_MASTER, 0.18),
				Styles.COLOR_MASTER.darkened(0.25)
			)
		)
		master_button.pressed.connect(_on_discard_color.bind("master", master_owned))
		action_list.add_child(master_button)
	var confirm := Button.new()
	confirm.text = "确认弃球（%d / %d）" % [picked, required]
	Styles.style_button(confirm, true)
	confirm.focus_mode = Control.FOCUS_NONE
	confirm.disabled = picked != required or required <= 0
	confirm.pressed.connect(_on_confirm_discard)
	action_list.add_child(confirm)


func _on_discard_color(key: String, owned: int) -> void:
	var current := int(_discard_selection.get(key, 0))
	var next := current + 1
	if next > owned:
		next = 0
	if next == 0:
		_discard_selection.erase(key)
	else:
		_discard_selection[key] = next
	_render_actions()


func _on_confirm_discard() -> void:
	var players: Variant = _view.get("players")
	if not (players is Array) or (players as Array).is_empty():
		return
	var tokens: Dictionary = (players[0] as Dictionary).get("tokens", {})
	var held := _token_total(tokens)
	var required := maxi(0, held - 10)
	if _token_total(_discard_selection) != required:
		_toast("弃球数量必须正好让球数降到 10")
		return
	var payload := {}
	for key in Styles.COLOR_KEYS:
		payload[key] = int(_discard_selection.get(key, 0))
	payload["master"] = int(_discard_selection.get("master", 0))
	_send_action({"type": "discard", "tokens": payload})


func _render_evolution() -> void:
	hint_label.text = "可以进化一只宝可梦，或不进化直接结束回合。"
	var actions: Variant = _view.get("legal_actions")
	var texts: Variant = _state.get("legal_action_texts")
	var end_action: Dictionary = {"type": "end_turn"}
	var end_text := "结束回合"
	if actions is Array:
		for index in (actions as Array).size():
			var action_v: Variant = actions[index]
			if not (action_v is Dictionary):
				continue
			var action: Dictionary = action_v
			if str(action.get("type", "")) == "end_turn":
				end_action = action
				if texts is Array and index < (texts as Array).size():
					end_text = str(texts[index])
				continue
			var label := JSON.stringify(action)
			if texts is Array and index < (texts as Array).size():
				label = str(texts[index])
			var button := Button.new()
			button.text = label
			Styles.style_button(button)
			button.focus_mode = Control.FOCUS_NONE
			button.pressed.connect(_send_action.bind(action))
			action_list.add_child(button)
	var end_button := Button.new()
	end_button.text = end_text
	Styles.style_button(end_button, true)
	end_button.focus_mode = Control.FOCUS_NONE
	end_button.pressed.connect(_send_action.bind(end_action))
	action_list.add_child(end_button)



# ---------------------------------------------------------------------------
# 保存、提示与查询
# ---------------------------------------------------------------------------


func _on_save_pressed() -> void:
	_save_replay(false)


func _save_replay(auto: bool) -> void:
	_request(HTTPClient.METHOD_POST, "/api/game/replay/save", null, _on_saved.bind(auto))


func _on_saved(ok: bool, _code: int, data: Dictionary, _auto: bool) -> void:
	if not ok:
		_toast("保存回放失败：%s" % str(data.get("error", "未知错误")))
		return
	var replay: Variant = data.get("replay")
	if not (replay is Dictionary):
		_toast("保存回放失败：响应缺少 replay")
		return
	var path := str((replay as Dictionary).get("json", ""))
	save_button.text = "已保存回放"
	_toast("回放已写入 %s" % path.get_file())


func _show_error(data: Dictionary) -> void:
	_toast("错误：%s" % str(data.get("error", "请求失败")))


func _toast(message: String) -> void:
	_log_lines.append("⚠ " + message)
	_trim_log()
	hint_label.text = message
	hint_label.add_theme_color_override("font_color", Styles.COLOR_YELLOW)
	toast_timer.start()


func _on_toast_timeout() -> void:
	hint_label.add_theme_color_override("font_color", Styles.COLOR_MUTED)
	_render_actions()


func _refresh_controls() -> void:
	_refresh_supply()
	_render_actions()
	_render_reserved()
	board_view.set_interactive(
		_can_interact() and str(_view.get("phase", "")) == "main_action"
	)


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


func _token_total(tokens: Dictionary) -> int:
	var total := 0
	for key in Styles.COLOR_KEYS:
		total += int(tokens.get(key, 0))
	total += int(tokens.get("master", 0))
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
	var master_count := int(tokens.get("master", 0))
	if master_count > 0:
		token_parts.append("大师%d" % master_count)
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
	lines.append("捕捉：%s" % ("、".join(captured_parts) if not captured_parts.is_empty() else "无"))

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
	lines.append("%s · %d 分" % [str(card.get("name_zh", "?")), int(card.get("points", 0))])
	var bonus_key := str(card.get("bonus_color", "red"))
	lines.append("减免 %s +%d" % [
		Styles.COLOR_LABELS.get(bonus_key, "?"),
		int(card.get("bonus_count", 0)),
	])
	var cost: Dictionary = card.get("cost", {})
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
	if players_row.get_child_count() != 3:
		failures.append("玩家面板数量应为 3，实际 %d" % players_row.get_child_count())
	if board_view.board().is_empty():
		failures.append("BoardView 未注入棋盘")
	else:
		var level1: Variant = board_view.board().get("level1")
		if not (level1 is Array) or (level1 as Array).size() != 4:
			failures.append("1 阶行应有 4 个槽位")
	if _supply_buttons.size() != 5:
		failures.append("供应区按钮应为 5 个，实际 %d" % _supply_buttons.size())
	if not _state.get("human_turn", false):
		failures.append("开局应轮到人类")
	if events_label.text.is_empty():
		failures.append("对局记录为空")

	var actions: Variant = _view.get("legal_actions")
	var take_three: Dictionary = {}
	if actions is Array:
		for action_v in actions:
			if action_v is Dictionary and str(action_v.get("type", "")) == "take_three":
				take_three = action_v
				break
	if take_three.is_empty():
		failures.append("开局没有拿 3 色动作")
	else:
		_selection = {"red": 1, "blue": 1, "yellow": 1}
		var built := _take_action()
		if not _is_legal(built):
			failures.append("界面构造的拿 3 色动作与引擎合法动作不一致：%s" % JSON.stringify(built))
		_selection.clear()
		if actions is Array:
			for action_v in actions:
				if not (action_v is Dictionary) or str(action_v.get("type", "")) != "reserve":
					continue
				var source: Variant = action_v.get("source")
				if not (source is Dictionary) or str(source.get("type", "")) != "board":
					continue
				var row := BOARD_CATEGORIES.find(str(source.get("category", "")))
				if row < 0:
					continue
				var reserve := _reserve_board(row, int(source.get("index", 0)))
				if not _is_legal(reserve):
					failures.append("界面构造的保留动作与引擎合法动作不一致")
				break

	if failures.is_empty():
		var acted: Dictionary = await _smoke_request(
			HTTPClient.METHOD_POST, "/api/game/action", {"action": take_three}
		)
		if not bool(acted.get("ok", false)):
			failures.append("拿球动作失败：%s" % str(acted))
		else:
			_on_action(true, 200, acted["data"])
			_smoke_drain_steps()
			if str(_view.get("phase", "")) != "evolution":
				failures.append("拿球后应进入进化阶段，实际 %s" % str(_view.get("phase", "")))
			if not _is_legal({"type": "end_turn"}):
				failures.append("进化阶段应允许结束回合")
			if action_list.get_child_count() == 0:
				failures.append("进化阶段没有生成行动按钮")

		var ended: Dictionary = await _smoke_request(
			HTTPClient.METHOD_POST, "/api/game/action", {"action": {"type": "end_turn"}}
		)
		if not bool(ended.get("ok", false)):
			failures.append("结束回合失败：%s" % str(ended))
		else:
			var ended_data: Dictionary = ended["data"]
			var steps: Variant = ended_data.get("steps")
			if not (steps is Array) or (steps as Array).size() < 2:
				failures.append("结束回合后应包含至少 2 个 AI 动作")
			_on_action(true, 200, ended_data)
			_smoke_drain_steps()
			if int(_view.get("current_player", -1)) != 0:
				failures.append("AI 回合后应轮回到人类")
			if str(_view.get("phase", "")) != "main_action":
				failures.append("AI 回合后应回到主要行动阶段")
		await get_tree().process_frame

	var theme_font := get_theme_default_font()
	if theme_font == null or not theme_font.has_char("璀".unicode_at(0)):
		failures.append("默认字体缺少中文字形")

	if failures.is_empty():
		print("[battle-smoke] OK 玩家=%d 棋盘=%s 供应按钮=%d 记录=%d 字" % [
			players_row.get_child_count(),
			str(board_view.size),
			_supply_buttons.size(),
			events_label.text.length(),
		])
		get_tree().quit(0)
		return
	for message in failures:
		printerr("[battle-smoke] FAIL " + message)
	get_tree().quit(1)


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
	var ok := result == HTTPRequest.RESULT_SUCCESS and code >= 200 and code < 300 and bool(data.get("ok", true))
	return {"ok": ok, "code": code, "data": data}

