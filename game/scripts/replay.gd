extends Control
## Godot 回放可视化主控制器。
##
## 数据来源是 Rust CLI 写出的 replay JSON（tmp/*.json）。界面只读，不建游戏、
## 不改规则：每一帧都从 `frames[i].view` 重新绘制。私密信息遵循后端约定——
## 每帧视图来自执行动作的玩家：自己的保留牌全部可见，对手只显示从场上
## 保留的牌；从牌堆顶保留的牌只暴露数量。
##
## 回放路径解析顺序：命令行 `-- <path>` > 环境变量 SPLENDOR_REPLAY >
## 仓库 tmp/ 下最新的 *.json > 空状态提示。
## 环境变量 SPLENDOR_SMOKE=1 时执行无头冒烟（载入 + 逐帧渲染 + 断言），
## 完成后按 0 / 1 退出。

const Styles := preload("res://scripts/ui/styles.gd")

const PHASES := {
	"main_action": "主要行动",
	"discard": "弃球",
	"evolution": "进化 / 结束回合",
	"game_over": "终局",
}
const CONTROLLERS := {
	"human": "玩家",
	"random": "随机 bot",
	"heuristic": "启发式 bot",
	"heuristic-v1": "启发式 v1 bot",
	"minimax": "极小化极大 bot",
}
const SPEEDS := [
	{"label": "慢", "wait": 1.8},
	{"label": "中", "wait": 1.0},
	{"label": "快", "wait": 0.55},
	{"label": "极快", "wait": 0.25},
]
const SPEED_DEFAULT := 1

@onready var meta_label: Label = $Margin/Root/TopBar/Meta
@onready var open_button: Button = $Margin/Root/TopBar/OpenButton
@onready var reload_button: Button = $Margin/Root/TopBar/ReloadButton
@onready var battle_button: Button = $Margin/Root/TopBar/BattleButton
@onready var menu_button: Button = $Margin/Root/TopBar/MenuButton
@onready var prev_button: Button = $Margin/Root/Timeline/PrevButton
@onready var play_button: Button = $Margin/Root/Timeline/PlayButton
@onready var next_button: Button = $Margin/Root/Timeline/NextButton
@onready var timeline_slider: HSlider = $Margin/Root/Timeline/TimelineSlider
@onready var frame_label: Label = $Margin/Root/Timeline/FrameLabel
@onready var phase_label: Label = $Margin/Root/Timeline/PhaseLabel
@onready var speed_button: Button = $Margin/Root/Timeline/SpeedButton
@onready var banner_panel: PanelContainer = $Margin/Root/BannerPanel
@onready var banner_label: Label = $Margin/Root/BannerPanel/BannerLabel
@onready var board_panel: PanelContainer = $Margin/Root/Body/Left/BoardPanel
@onready var board_view: Control = $Margin/Root/Body/Left/BoardPanel/VBox/BoardView
@onready var players_panel: PanelContainer = $Margin/Root/Body/Left/PlayersPanel
@onready var players_row: HBoxContainer = $Margin/Root/Body/Left/PlayersPanel/VBox/PlayersRow
@onready var supply_panel: PanelContainer = $Margin/Root/Body/Right/SupplyPanel
@onready var supply_grid: GridContainer = $Margin/Root/Body/Right/SupplyPanel/VBox/SupplyGrid
@onready var action_panel: PanelContainer = $Margin/Root/Body/Right/ActionPanel
@onready var action_main: Label = $Margin/Root/Body/Right/ActionPanel/VBox/ActionMain
@onready var action_actor: Label = $Margin/Root/Body/Right/ActionPanel/VBox/ActionActor
@onready var events_label: RichTextLabel = $Margin/Root/Body/Right/ActionPanel/VBox/Events
@onready var file_dialog: FileDialog = $FileDialog
@onready var play_timer: Timer = $PlayTimer

var replay: Dictionary = {}
var frames: Array = []
var index := 0
var playing := false
var _speed_index := SPEED_DEFAULT
var _replay_path := ""
var _syncing_slider := false


func _ready() -> void:
	_speed_index = AppState.replay_speed_index
	_apply_styles()
	_wire_signals()
	get_window().min_size = Vector2i(1100, 700)
	speed_button.text = "速度：%s" % SPEEDS[_speed_index]["label"]
	if OS.get_environment("SPLENDOR_SMOKE") == "1":
		_run_smoke()
		return
	_load_initial()


func _apply_styles() -> void:
	Styles.style_button(open_button)
	Styles.style_button(reload_button)
	Styles.style_button(battle_button, true)
	Styles.style_button(prev_button)
	Styles.style_button(play_button, true)
	Styles.style_button(next_button)
	Styles.style_button(speed_button)
	Styles.style_button(menu_button)
	for button in [
		open_button,
		reload_button,
		battle_button,
		menu_button,
		prev_button,
		play_button,
		next_button,
		speed_button,
	]:
		button.focus_mode = Control.FOCUS_NONE
	timeline_slider.focus_mode = Control.FOCUS_NONE
	for panel in [board_panel, players_panel, supply_panel, action_panel]:
		panel.add_theme_stylebox_override("panel", Styles.panel_box())
	events_label.add_theme_color_override("default_color", Styles.COLOR_MUTED)
	events_label.add_theme_font_size_override("normal_font_size", 12)
	events_label.add_theme_font_size_override("bold_font_size", 12)


func _wire_signals() -> void:
	open_button.pressed.connect(_on_open)
	reload_button.pressed.connect(_on_reload)
	battle_button.pressed.connect(_on_battle_pressed)
	menu_button.pressed.connect(_on_menu_pressed)
	prev_button.pressed.connect(_on_prev)
	play_button.pressed.connect(_toggle_play)
	next_button.pressed.connect(_on_next)
	speed_button.pressed.connect(_on_speed)
	timeline_slider.value_changed.connect(_on_slider_changed)
	file_dialog.file_selected.connect(_on_file_selected)
	play_timer.timeout.connect(_on_play_tick)


# ---------------------------------------------------------------------------
# 回放载入
# ---------------------------------------------------------------------------


func _load_initial() -> void:
	var path := _initial_replay_path()
	if path.is_empty():
		_show_empty_state()
		return
	_load_replay(path)


func _load_replay(path: String) -> bool:
	var text := FileAccess.get_file_as_string(path)
	if text.is_empty():
		_show_error("无法读取回放文件：%s" % path)
		return false
	var parsed: Variant = JSON.parse_string(text)
	if not (parsed is Dictionary):
		_show_error("回放 JSON 解析失败：%s" % path)
		return false
	var data: Dictionary = _normalize_numbers(parsed)
	var loaded: Variant = data.get("frames", [])
	if not (loaded is Array) or (loaded as Array).is_empty():
		_show_error("回放没有 frames：%s" % path)
		return false
	replay = data
	frames = loaded
	_replay_path = path
	index = 0
	_stop_playing()
	meta_label.tooltip_text = path
	_render()
	return true


func _initial_replay_path() -> String:
	var args := OS.get_cmdline_user_args()
	if not args.is_empty() and not args[0].begins_with("-"):
		return _absolute_path(args[0])
	var env_path := OS.get_environment("SPLENDOR_REPLAY")
	if not env_path.is_empty():
		return _absolute_path(env_path)
	if not AppState.replay_path.is_empty():
		var picked := AppState.replay_path
		AppState.replay_path = ""
		return picked
	return _latest_replay_in_tmp()


func _latest_replay_in_tmp() -> String:
	var dir_path := _repo_dir().path_join("tmp")
	var dir := DirAccess.open(dir_path)
	if dir == null:
		return ""
	var best := ""
	var best_time := -1
	for file in dir.get_files():
		if not file.to_lower().ends_with(".json"):
			continue
		var path := dir_path.path_join(file)
		var modified := FileAccess.get_modified_time(path)
		if modified > best_time:
			best_time = modified
			best = path
	return best


func _repo_dir() -> String:
	return ProjectSettings.globalize_path("res://").trim_suffix("/").get_base_dir()


func _absolute_path(path: String) -> String:
	if path.is_absolute_path():
		return path
	return _repo_dir().path_join(path)


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


func _show_empty_state() -> void:
	replay = {}
	frames = []
	index = 0
	_replay_path = ""
	_stop_playing()
	board_view.set_view({})
	_clear(supply_grid)
	_clear(players_row)
	meta_label.text = "未加载回放 · 在 backend/ 运行 bot-vs-bot 或 pve 生成 tmp/*.json"
	meta_label.add_theme_color_override("font_color", Styles.COLOR_MUTED)
	meta_label.tooltip_text = ""
	action_main.text = "打开一个回放开始"
	action_actor.text = "支持 tmp/*.json 与任意 CLI 导出的 replay JSON"
	events_label.text = (
		"· 点击右上角「打开回放…」选择文件\n"
		+ "· 或启动时传参：godot --path game -- tmp/bot-42-2p.json\n"
		+ "· 键盘：← → 步进，空格 播放/暂停，Home/End 首尾"
	)
	frame_label.text = "0 / 0"
	phase_label.text = "—"
	_render_banner({})
	_sync_buttons()


func _show_error(message: String) -> void:
	_show_empty_state()
	meta_label.text = message
	meta_label.add_theme_color_override("font_color", Styles.COLOR_RED)
	action_main.text = "回放载入失败"
	action_actor.text = message



# ---------------------------------------------------------------------------
# 渲染
# ---------------------------------------------------------------------------


func _render() -> void:
	if frames.is_empty():
		_show_empty_state()
		return
	index = clampi(index, 0, frames.size() - 1)
	var frame: Dictionary = frames[index]
	var view: Dictionary = frame.get("view", {})
	board_view.set_view(view.get("board", {}))
	_render_meta(view)
	_render_supply(view.get("supply", {}))
	_render_action(frame, view)
	_render_players(view)
	_render_banner(view)
	_sync_controls(view)


func _render_meta(view: Dictionary) -> void:
	var names: Array = replay.get("players", [])
	var controllers: Array = replay.get("controllers", [])
	var parts: Array[String] = []
	for i in names.size():
		var controller := "?"
		if i < controllers.size():
			controller = str(controllers[i])
		parts.append("%s（%s）" % [
			str(names[i]),
			CONTROLLERS.get(controller, controller),
		])
	meta_label.text = "种子 %d · %d 人 · 第 %d 回合 · %s" % [
		int(replay.get("seed", 0)),
		names.size(),
		int(view.get("turn_number", 0)),
		"、".join(parts),
	]
	meta_label.add_theme_color_override("font_color", Styles.COLOR_MUTED)


func _render_supply(supply: Dictionary) -> void:
	_clear(supply_grid)
	for key in Styles.COLOR_KEYS:
		var count := int(supply.get(key, 0))
		var tile := _panel(Styles.tile_box())
		tile.size_flags_horizontal = Control.SIZE_EXPAND_FILL
		if count == 0:
			tile.modulate = Color(1, 1, 1, 0.45)
		var row := HBoxContainer.new()
		row.add_theme_constant_override("separation", 5)
		tile.add_child(row)
		row.add_child(_dot(Styles.TOKEN_COLORS[key]))
		row.add_child(_label(Styles.COLOR_LABELS[key], 12, Styles.COLOR_TEXT))
		row.add_child(_spacer())
		row.add_child(_label(str(count), 15, Styles.COLOR_TEXT))
		supply_grid.add_child(tile)


func _render_action(frame: Dictionary, view: Dictionary) -> void:
	var actor: Variant = frame.get("actor")
	var phase := str(view.get("phase", ""))
	var viewer_id := int(view.get("current_player", 0))
	if actor != null:
		viewer_id = int(actor)
	var viewer_name := _player_name(view, viewer_id)

	if actor == null:
		action_main.text = "开局"
		action_actor.text = "等待第一位玩家行动 · 第 %d 回合 · 视角：%s" % [
			int(view.get("turn_number", 0)),
			viewer_name,
		]
	else:
		action_main.text = str(frame.get("action_text", "—"))
		action_actor.text = "%s 执行 · 第 %d 回合 · %s · 视角：%s（保留牌按公开信息显示）" % [
			_player_name(view, int(actor)),
			int(view.get("turn_number", 0)),
			str(PHASES.get(phase, phase)),
			viewer_name,
		]

	var text := ""
	var lines: Variant = frame.get("events", [])
	if lines is Array:
		for line in lines:
			text += "· %s\n" % str(line)
	if text.is_empty():
		text = "· 本步没有新事件\n"
	events_label.text = text



func _render_players(view: Dictionary) -> void:
	_clear(players_row)
	var players: Array = view.get("players", [])
	var current := int(view.get("current_player", 0))
	var game_over := str(view.get("phase", "")) == "game_over"
	for player_v in players:
		var player: Dictionary = player_v
		players_row.add_child(_build_player_panel(player, current, game_over))


func _build_player_panel(player: Dictionary, current_id: int, game_over: bool) -> PanelContainer:
	var player_id := int(player.get("id", 0))
	var is_current := player_id == current_id and not game_over
	var panel := _panel(Styles.panel_box(
		Styles.COLOR_PANEL_2,
		Styles.COLOR_ACCENT if is_current else Styles.COLOR_LINE,
		10,
		2 if is_current else 1
	))
	panel.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	panel.custom_minimum_size = Vector2(200, 0)

	var vbox := VBoxContainer.new()
	vbox.add_theme_constant_override("separation", 5)
	panel.add_child(vbox)

	# 名字、行动中徽章、分数。
	var head := HBoxContainer.new()
	head.add_theme_constant_override("separation", 7)
	vbox.add_child(head)
	head.add_child(_label(str(player.get("name", "?")), 16, Styles.COLOR_TEXT))
	if is_current:
		head.add_child(_badge("行动中"))
	head.add_child(_spacer())
	var score := _label(str(int(player.get("points", 0))), 21, Styles.COLOR_GOLD)
	score.vertical_alignment = VERTICAL_ALIGNMENT_CENTER
	head.add_child(score)
	head.add_child(_label("分", 11, Styles.COLOR_MUTED))

	# 持有球。
	var token_row := HFlowContainer.new()
	token_row.add_theme_constant_override("h_separation", 4)
	token_row.add_theme_constant_override("v_separation", 3)
	vbox.add_child(token_row)
	var tokens: Dictionary = player.get("tokens", {})
	for key in Styles.COLOR_KEYS:
		var count := int(tokens.get(key, 0))
		token_row.add_child(_pill(
			"%s %d" % [Styles.COLOR_LABELS[key], count],
			Styles.TOKEN_COLORS[key],
			count == 0
		))

	# 减免与统计。
	var stats_row := HFlowContainer.new()
	stats_row.add_theme_constant_override("h_separation", 4)
	stats_row.add_theme_constant_override("v_separation", 3)
	vbox.add_child(stats_row)
	stats_row.add_child(_chip("减免"))
	var bonuses: Dictionary = player.get("bonuses", {})
	for key in ["red", "blue", "yellow", "pink", "black"]:
		var count := int(bonuses.get(key, 0))
		if count > 0:
			stats_row.add_child(_pill(str(count), Styles.TOKEN_COLORS[key]))
	stats_row.add_child(_chip("捕捉 %d" % int(player.get("card_count", 0))))
	stats_row.add_child(_chip("保留 %d / 3" % int(player.get("reserved_count", 0))))
	stats_row.add_child(_chip("进化 %d" % int(player.get("evolution_count", 0))))
	stats_row.add_child(_chip("行动 %d 回合" % int(player.get("turns_taken", 0))))

	# 已捕捉与保留牌。
	var cards_row := HFlowContainer.new()
	cards_row.add_theme_constant_override("h_separation", 4)
	cards_row.add_theme_constant_override("v_separation", 3)
	vbox.add_child(cards_row)
	# 已捕捉 / 保留：面板内只放摘要，完整列表进 tooltip，避免长局把棋盘挤扁。
	var captured: Array = []
	var captured_v: Variant = player.get("captured", [])
	if captured_v is Array:
		var captured_src: Array = captured_v
		captured = captured_src.duplicate()
		captured.sort_custom(
			func(a, b): return int(a.get("points", 0)) > int(b.get("points", 0))
		)

	var reserved: Array = []
	var reserved_visible := false
	var reserved_v: Variant = player.get("reserved")
	if reserved_v is Array:
		reserved_visible = true
		var reserved_src: Array = reserved_v
		reserved = reserved_src.duplicate()
	var hidden_reserved := maxi(
		0,
		int(player.get("reserved_count", 0)) - reserved.size()
	)

	if captured.is_empty():
		cards_row.add_child(_chip("尚无宝可梦"))
	else:
		var captured_limit := mini(3, captured.size())
		for i in captured_limit:
			var card_v: Variant = captured[i]
			if card_v is Dictionary:
				cards_row.add_child(_card_chip(card_v, false))
		if captured.size() > captured_limit:
			cards_row.add_child(_chip("… +%d" % (captured.size() - captured_limit)))

	if reserved_visible:
		if reserved.is_empty() and hidden_reserved == 0:
			cards_row.add_child(_chip("保留牌：无"))
		else:
			var reserved_limit := mini(2, reserved.size())
			for i in reserved_limit:
				var card_v: Variant = reserved[i]
				if card_v is Dictionary:
					cards_row.add_child(_card_chip(card_v, true))
			if reserved.size() > reserved_limit:
				cards_row.add_child(_chip("… +%d" % (reserved.size() - reserved_limit)))
			if hidden_reserved > 0:
				cards_row.add_child(_chip("另 %d 张未公开" % hidden_reserved))
	elif hidden_reserved > 0:
		cards_row.add_child(_chip("保留 %d 张（未公开）" % hidden_reserved))

	panel.tooltip_text = _player_tooltip(player, captured, reserved, reserved_visible)
	return panel


func _render_banner(view: Dictionary) -> void:
	var last := frames.size() - 1
	var stalemate := bool(replay.get("stalemate", false)) and index == last
	if stalemate:
		banner_panel.add_theme_stylebox_override("panel", Styles.banner_box(Styles.COLOR_YELLOW))
		banner_label.add_theme_color_override("font_color", Color("ffe2a0"))
		banner_label.text = "对局僵局 · 供应区耗尽且当前玩家没有合法主行动（规则未定义 pass，无胜者）"
		banner_panel.visible = true
		return

	var winners: Variant = view.get("winners")
	if not (winners is Array) or (winners as Array).is_empty():
		banner_panel.visible = false
		banner_label.text = ""
		return

	var parts: Array[String] = []
	for winner_v in winners:
		var player_id := int(winner_v)
		parts.append("%s（%d 分）" % [
			_player_name(view, player_id),
			_player_points(view, player_id),
		])
	banner_label.text = "对局结束 · 胜者：%s" % "、".join(parts)
	banner_label.add_theme_color_override("font_color", Color("ffe2a0"))
	banner_panel.add_theme_stylebox_override("panel", Styles.banner_box(Styles.COLOR_GOLD))
	banner_panel.visible = true


func _sync_controls(view: Dictionary) -> void:
	var last := frames.size() - 1
	_syncing_slider = true
	timeline_slider.max_value = float(maxi(last, 0))
	timeline_slider.value = float(index)
	_syncing_slider = false
	frame_label.text = "%d / %d" % [index + 1, frames.size()]
	var phase := str(view.get("phase", ""))
	var stalemate := bool(replay.get("stalemate", false)) and index == last
	phase_label.text = "僵局" if stalemate else str(PHASES.get(phase, phase))
	phase_label.add_theme_color_override(
		"font_color",
		Styles.COLOR_YELLOW if stalemate else Styles.COLOR_ACCENT
	)
	_sync_buttons()



# ---------------------------------------------------------------------------
# 控件工厂
# ---------------------------------------------------------------------------


func _panel(box: StyleBoxFlat) -> PanelContainer:
	var panel := PanelContainer.new()
	panel.add_theme_stylebox_override("panel", box)
	return panel


func _label(text: String, font_size: int = 15, color: Color = Styles.COLOR_TEXT) -> Label:
	var label := Label.new()
	label.text = text
	label.add_theme_font_size_override("font_size", font_size)
	label.add_theme_color_override("font_color", color)
	return label


func _dot(color: Color) -> PanelContainer:
	var dot := _panel(Styles.dot_box(color))
	dot.custom_minimum_size = Vector2(8, 8)
	dot.size_flags_vertical = Control.SIZE_SHRINK_CENTER
	return dot


func _pill(text: String, color: Color, dim: bool = false) -> PanelContainer:
	var pill := _panel(Styles.pill_box())
	pill.size_flags_vertical = Control.SIZE_SHRINK_CENTER
	if dim:
		pill.modulate = Color(1, 1, 1, 0.45)
	var row := HBoxContainer.new()
	row.add_theme_constant_override("separation", 4)
	pill.add_child(row)
	row.add_child(_dot(color))
	row.add_child(_label(text, 11, Styles.COLOR_TEXT))
	return pill


func _chip(text: String) -> PanelContainer:
	var chip := _panel(Styles.chip_box())
	chip.size_flags_vertical = Control.SIZE_SHRINK_CENTER
	chip.add_child(_label(text, 10, Styles.COLOR_MUTED))
	return chip


func _badge(text: String) -> PanelContainer:
	var badge := _panel(Styles.tile_box(Color("2d4a9e"), Color("3f63c8"), 8, 7, 1))
	badge.size_flags_vertical = Control.SIZE_SHRINK_CENTER
	badge.add_child(_label(text, 10, Color("dfe8ff")))
	return badge


func _card_chip(card: Dictionary, reserved: bool) -> PanelContainer:
	var bonus_key := str(card.get("bonus_color", "red"))
	var prefix := "保留 " if reserved else ""
	var text := "%s%s %d分" % [
		prefix,
		str(card.get("name_zh", "?")),
		int(card.get("points", 0)),
	]
	return _pill(text, Styles.TOKEN_COLORS.get(bonus_key, Styles.COLOR_ACCENT))


func _player_tooltip(
	player: Dictionary,
	captured: Array,
	reserved: Array,
	reserved_visible: bool
) -> String:
	var lines: Array[String] = []
	lines.append("%s · %d 分" % [
		str(player.get("name", "?")),
		int(player.get("points", 0)),
	])

	var bonus_parts: Array[String] = []
	var bonuses: Dictionary = player.get("bonuses", {})
	for key in ["red", "blue", "yellow", "pink", "black"]:
		var count := int(bonuses.get(key, 0))
		if count > 0:
			bonus_parts.append("%s%d" % [Styles.COLOR_LABELS[key], count])
	lines.append("减免：" + (" ".join(bonus_parts) if not bonus_parts.is_empty() else "无"))

	var captured_parts: Array[String] = []
	for card_v in captured:
		if card_v is Dictionary:
			var card: Dictionary = card_v
			captured_parts.append("%s(%d)" % [
				str(card.get("name_zh", "?")),
				int(card.get("points", 0)),
			])
	lines.append("捕捉 %d 张：%s" % [
		captured.size(),
		"、".join(captured_parts) if not captured_parts.is_empty() else "无",
	])

	var reserved_count := int(player.get("reserved_count", 0))
	var hidden_reserved := maxi(0, reserved_count - reserved.size())
	if reserved_visible:
		var reserved_parts: Array[String] = []
		for card_v in reserved:
			if card_v is Dictionary:
				var card: Dictionary = card_v
				reserved_parts.append(str(card.get("name_zh", "?")))
		if hidden_reserved > 0:
			reserved_parts.append("%d 张未公开" % hidden_reserved)
		lines.append("保留 %d 张：%s" % [
			reserved_count,
			"、".join(reserved_parts) if not reserved_parts.is_empty() else "无",
		])
	else:
		lines.append("保留：%d 张（未公开）" % reserved_count)
	return "\n".join(lines)


func _spacer() -> Control:
	var spacer := Control.new()
	spacer.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	return spacer


func _collect_text(node: Node) -> String:
	var parts: Array[String] = []
	if node is Label:
		parts.append((node as Label).text)
	elif node is Button:
		parts.append((node as Button).text)
	for child in node.get_children():
		var child_text := _collect_text(child)
		if not child_text.is_empty():
			parts.append(child_text)
	return "\n".join(parts)


func _clear(container: Node) -> void:
	for child in container.get_children():
		container.remove_child(child)
		child.queue_free()


func _player_name(view: Dictionary, player_id: int) -> String:
	var players: Array = view.get("players", [])
	for player_v in players:
		var player: Dictionary = player_v
		if int(player.get("id", -1)) == player_id:
			return str(player.get("name", "玩家 %d" % player_id))
	return "玩家 %d" % player_id


func _player_points(view: Dictionary, player_id: int) -> int:
	var players: Array = view.get("players", [])
	for player_v in players:
		var player: Dictionary = player_v
		if int(player.get("id", -1)) == player_id:
			return int(player.get("points", 0))
	return 0


# ---------------------------------------------------------------------------
# 播放与跳转
# ---------------------------------------------------------------------------


func _toggle_play() -> void:
	if frames.is_empty():
		return
	if playing:
		_stop_playing()
	else:
		_start_playing()


func _start_playing() -> void:
	if frames.is_empty():
		return
	if index >= frames.size() - 1:
		index = 0
		_render()
	playing = true
	play_timer.start(float(SPEEDS[_speed_index]["wait"]))
	_sync_buttons()


func _stop_playing() -> void:
	playing = false
	play_timer.stop()
	if is_node_ready():
		play_button.text = "▶ 自动播放"


func _on_play_tick() -> void:
	if index >= frames.size() - 1:
		_stop_playing()
		_sync_buttons()
		return
	_go(index + 1)


func _go(target: int) -> void:
	if frames.is_empty():
		return
	index = clampi(target, 0, frames.size() - 1)
	_render()


func _on_prev() -> void:
	_stop_playing()
	_go(index - 1)


func _on_next() -> void:
	_stop_playing()
	_go(index + 1)


func _on_slider_changed(value: float) -> void:
	if _syncing_slider or frames.is_empty():
		return
	_stop_playing()
	_go(int(round(value)))


func _on_speed() -> void:
	_speed_index = (_speed_index + 1) % SPEEDS.size()
	speed_button.text = "速度：%s" % SPEEDS[_speed_index]["label"]
	if playing:
		play_timer.start(float(SPEEDS[_speed_index]["wait"]))


func _sync_buttons() -> void:
	var has_frames := not frames.is_empty()
	var last := frames.size() - 1
	prev_button.disabled = not has_frames or index <= 0
	next_button.disabled = not has_frames or index >= last
	play_button.disabled = not has_frames
	reload_button.disabled = _replay_path.is_empty()
	speed_button.disabled = not has_frames
	timeline_slider.editable = has_frames
	play_button.text = "⏸ 暂停" if playing else "▶ 自动播放"
	speed_button.text = "速度：%s" % SPEEDS[_speed_index]["label"]


# ---------------------------------------------------------------------------
# 文件与键盘
# ---------------------------------------------------------------------------


func _on_open() -> void:
	var tmp_dir := _repo_dir().path_join("tmp")
	file_dialog.current_dir = tmp_dir if DirAccess.dir_exists_absolute(tmp_dir) else _repo_dir()
	file_dialog.popup_centered(Vector2i(900, 600))


func _on_file_selected(path: String) -> void:
	_load_replay(path)


func _on_reload() -> void:
	if _replay_path.is_empty():
		return
	_load_replay(_replay_path)


func _on_battle_pressed() -> void:
	_stop_playing()
	AppState.battle_resume = AppState.battle_in_progress
	get_tree().change_scene_to_file("res://scenes/battle.tscn")


func _on_menu_pressed() -> void:
	_stop_playing()
	get_tree().change_scene_to_file("res://scenes/menu.tscn")


func _unhandled_input(event: InputEvent) -> void:
	if file_dialog.visible:
		return
	if not (event is InputEventKey) or not event.pressed or event.echo:
		return
	if event.keycode == KEY_ESCAPE:
		_on_menu_pressed()
		get_viewport().set_input_as_handled()
		return
	match event.keycode:
		KEY_LEFT:
			_stop_playing()
			_go(index - 1)
		KEY_RIGHT:
			_stop_playing()
			_go(index + 1)
		KEY_SPACE, KEY_P:
			_toggle_play()
		KEY_HOME:
			_stop_playing()
			_go(0)
		KEY_END:
			_stop_playing()
			_go(frames.size() - 1)
		_:
			return
	get_viewport().set_input_as_handled()



# ---------------------------------------------------------------------------
# 无头冒烟（SPLENDOR_SMOKE=1）
# ---------------------------------------------------------------------------


func _run_smoke() -> void:
	var path := _initial_replay_path()
	if path.is_empty():
		_fail_smoke("没有找到回放 JSON；请设置 SPLENDOR_REPLAY 或先生成 tmp/*.json")
		return
	if not _load_replay(path):
		_fail_smoke("回放载入失败：" + path)
		return
	await get_tree().process_frame

	var failures: Array[String] = []
	if frames.size() < 2:
		failures.append("frames 少于 2：%d" % frames.size())

	var first_frame: Dictionary = frames[0]
	var first_view: Dictionary = first_frame.get("view", {})
	var first_board: Dictionary = {}
	var board_v: Variant = first_view.get("board", {})
	if board_v is Dictionary:
		first_board = board_v
	else:
		failures.append("首帧缺少 board")

	for row_key in ["level1", "level2", "level3"]:
		var slots_v: Variant = first_board.get(row_key, [])
		var slots_ok := false
		if slots_v is Array:
			var slots: Array = slots_v
			slots_ok = slots.size() == 4
		if not slots_ok:
			failures.append("%s 槽位数应为 4" % row_key)
	for row_key in ["rare", "legendary"]:
		if not first_board.has(row_key):
			failures.append("缺少 %s 行" % row_key)
	if board_view.board().is_empty():
		failures.append("BoardView 没有收到 board")
	var board_tip: String = board_view._tooltip_for(Vector2i(0, 0))
	if board_tip.find("分") < 0:
		failures.append("场上牌 tooltip 生成失败")

	var replay_players: Array = replay.get("players", [])
	var first_players_v: Variant = first_view.get("players", [])
	var first_players: Array = []
	if first_players_v is Array:
		first_players = first_players_v
	if first_players.size() != replay_players.size():
		failures.append("view.players 与 replay.players 数量不一致")

	# 逐帧渲染：能走完末帧且玩家面板数量正确。
	var max_turn := 0
	for i in frames.size():
		index = i
		_render()
		await get_tree().process_frame
		var frame: Dictionary = frames[i]
		var view: Dictionary = frame.get("view", {})
		max_turn = maxi(max_turn, int(view.get("turn_number", 0)))
		var players_v: Variant = view.get("players", [])
		var expected := 0
		if players_v is Array:
			var players: Array = players_v
			expected = players.size()
		if players_row.get_child_count() != expected:
			failures.append("第 %d 帧玩家面板数 %d != %d" % [
				i, players_row.get_child_count(), expected,
			])
			break

	# 公开保留牌必须实际渲染到玩家面板；旧回放没有公开保留牌时跳过。
	var reserved_frame := -1
	var reserved_player := -1
	var reserved_name := ""
	for i in frames.size():
		var candidate: Dictionary = frames[i]
		var candidate_view: Variant = candidate.get("view", {})
		if not (candidate_view is Dictionary):
			continue
		var actor_v: Variant = candidate.get("actor")
		if actor_v == null:
			continue
		var actor_id := int(actor_v)
		var players_v: Variant = (candidate_view as Dictionary).get("players", [])
		if not (players_v is Array):
			continue
		for player_v in players_v:
			if not (player_v is Dictionary):
				continue
			var player: Dictionary = player_v
			var player_id := int(player.get("id", 0))
			if player_id == actor_id:
				continue
			var reserved_v: Variant = player.get("reserved")
			if not (reserved_v is Array) or (reserved_v as Array).is_empty():
				continue
			var cards: Array = reserved_v
			var card_v: Variant = cards[0]
			if card_v is Dictionary:
				reserved_frame = i
				reserved_player = player_id
				reserved_name = str((card_v as Dictionary).get("name_zh", ""))
				break
		if reserved_frame >= 0:
			break
	if reserved_frame >= 0:
		_go(reserved_frame)
		await get_tree().process_frame
		if reserved_player < players_row.get_child_count():
			var panel_text := _collect_text(players_row.get_child(reserved_player))
			if panel_text.find(reserved_name) < 0:
				failures.append(
					"第 %d 帧公开保留牌「%s」未渲染" % [reserved_frame, reserved_name]
				)
			else:
				print("[smoke] 公开保留牌渲染 OK：第 %d 帧 / %s" % [
					reserved_frame, reserved_name,
				])
	else:
		print("[smoke] 回放没有公开保留牌，跳过保留牌渲染断言")

	# 按钮边界。
	_stop_playing()
	_go(frames.size() - 1)
	if not next_button.disabled:
		failures.append("末帧 next 按钮应为禁用")
	_go(0)
	if not prev_button.disabled:
		failures.append("首帧 prev 按钮应为禁用")

	# 键盘右箭头。
	var key_event := InputEventKey.new()
	key_event.keycode = KEY_RIGHT
	key_event.pressed = true
	_unhandled_input(key_event)
	if index != mini(1, frames.size() - 1):
		failures.append("→ 键未推进帧，实际 %d" % index)

	# 滑块跳转。
	if frames.size() > 5:
		timeline_slider.value = 5.0
		if index != 5:
			failures.append("滑块未跳转到第 6 帧，实际 %d" % index)

	# 播放推进：直接触发一次 tick，不等待真实时间。
	if frames.size() > 1:
		_go(0)
		_start_playing()
		_on_play_tick()
		if index != 1:
			failures.append("播放 tick 未推进到第 2 帧，实际 %d" % index)
		_stop_playing()

	# 终局横幅。
	_go(frames.size() - 1)
	var last_frame: Dictionary = frames[frames.size() - 1]
	var last_view: Dictionary = last_frame.get("view", {})
	var winners_v: Variant = last_view.get("winners")
	var has_winners := false
	if winners_v is Array:
		var winners: Array = winners_v
		has_winners = not winners.is_empty()
	var stalemate := bool(replay.get("stalemate", false))
	if not stalemate and has_winners and not banner_panel.visible:
		failures.append("终局帧横幅未显示")

	# 玩家面板 tooltip 必须包含完整捕捉列表。
	if players_row.get_child_count() > 0:
		var player_panel: Control = players_row.get_child(0)
		if player_panel.tooltip_text.find("捕捉") < 0:
			failures.append("玩家面板 tooltip 生成失败")

	# 布局：无头模式也走 Control 排版，尺寸异常能直接暴露。
	await get_tree().process_frame
	if board_view.size.x < 400.0 or board_view.size.y < 200.0:
		failures.append("BoardView 尺寸异常：%s" % str(board_view.size))
	if players_row.get_child_count() > 0:
		var first_panel: Control = players_row.get_child(0)
		if first_panel.size.x < 180.0 or first_panel.size.y < 70.0:
			failures.append("玩家面板尺寸异常：%s" % str(first_panel.size))
	if supply_grid.get_child_count() != 6:
		failures.append("供应区格子数应为 6，实际 %d" % supply_grid.get_child_count())
	if events_label.size.y < 60.0:
		failures.append("事件区高度异常：%.0f" % events_label.size.y)

	# 中文兜底：SystemFont 主题必须能取到 CJK 字形。
	var theme_font := get_theme_default_font()
	if theme_font == null or not theme_font.has_char("璀".unicode_at(0)):
		failures.append("默认字体缺少中文字形（检查 SystemFont 主题）")

	if failures.is_empty():
		print("[smoke] OK 载入 %s：%d 帧 / %d 名玩家 / 最长第 %d 回合" % [
			path, frames.size(), replay_players.size(), max_turn,
		])
		print("[smoke] 布局 BoardView=%s 玩家面板=%s 事件区=%s" % [
			str(board_view.size),
			str((players_row.get_child(0) as Control).size) if players_row.get_child_count() > 0 else "无",
			str(events_label.size),
		])
		print("[smoke] 容器 Root=%s Margin=%s Body=%s Left=%s Right=%s" % [
			str(size),
			str(($Margin as Control).size),
			str(($Margin/Root/Body as Control).size),
			str(($Margin/Root/Body/Left as Control).size),
			str(($Margin/Root/Body/Right as Control).size),
		])
		get_tree().quit(0)
		return
	for message in failures:
		printerr("[smoke] FAIL " + message)
	get_tree().quit(1)


func _fail_smoke(message: String) -> void:
	printerr("[smoke] FAIL " + message)
	get_tree().quit(1)

