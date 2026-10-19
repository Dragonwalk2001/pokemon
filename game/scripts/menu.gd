extends Control
## 标题菜单：继续对局 / 新对局 / 观看回放 / 设置 / 退出。
##
## 菜单只收集启动参数，不创建对局；对局与回放分别在 battle.tscn / replay.tscn。
## 开局参数写入 AppState，对战场景读取后调用本地引擎。
## 环境变量 SPLENDOR_MENU_SMOKE=1 时执行无头冒烟，退出码 0 表示通过。

const Styles := preload("res://scripts/ui/styles.gd")
const Widgets := preload("res://scripts/ui/widgets.gd")

const BATTLE_SCENE := "res://scenes/battle.tscn"
const REPLAY_SCENE := "res://scenes/replay.tscn"
const PLAYER_OPTIONS := [
	{"id": 2, "label": "1 人 + 2 AI"},
	{"id": 3, "label": "1 人 + 3 AI"},
]
const BOT_OPTIONS := [
	{"id": "minimax", "label": "极小化极大"},
	{"id": "heuristic", "label": "启发式"},
	{"id": "random", "label": "随机"},
]
const AI_SPEED_LABELS := ["慢", "标准", "快"]
const REPLAY_SPEED_LABELS := ["慢", "中", "快", "极快"]

@onready var continue_button: Button = $Margin/Root/MenuButtons/ContinueButton
@onready var new_game_button: Button = $Margin/Root/MenuButtons/NewGameButton
@onready var replay_button: Button = $Margin/Root/MenuButtons/ReplayButton
@onready var settings_button: Button = $Margin/Root/MenuButtons/SettingsButton
@onready var quit_button: Button = $Margin/Root/MenuButtons/QuitButton
@onready var overlay: Control = $Overlay
@onready var modal: PanelContainer = $Overlay/Center/Modal
@onready var modal_title: Label = $Overlay/Center/Modal/VBox/ModalTitle
@onready var modal_body: VBoxContainer = $Overlay/Center/Modal/VBox/ModalBody
@onready var modal_options: VBoxContainer = $Overlay/Center/Modal/VBox/ModalOptions
@onready var cancel_button: Button = $Overlay/Center/Modal/VBox/ModalFooter/CancelButton
@onready var confirm_button: Button = $Overlay/Center/Modal/VBox/ModalFooter/ConfirmButton
@onready var file_dialog: FileDialog = $FileDialog

var _modal_kind := ""
var _setup_bots_option: OptionButton
var _setup_bot_option: OptionButton
var _setup_seed_spin: SpinBox
var _settings_ai_option: OptionButton
var _settings_replay_option: OptionButton


func _ready() -> void:
	get_window().min_size = Vector2i(1100, 700)
	_apply_styles()
	_wire_signals()
	_refresh_continue()
	if OS.get_environment("SPLENDOR_MENU_SMOKE") == "1":
		_run_smoke()
		return
	_focus_default()


func _apply_styles() -> void:
	Styles.style_button(continue_button, true)
	Styles.style_button(new_game_button)
	Styles.style_button(replay_button)
	Styles.style_button(settings_button)
	Styles.style_button(quit_button)
	Styles.style_button(cancel_button)
	Styles.style_button(confirm_button, true)
	modal.add_theme_stylebox_override("panel", Styles.panel_box())


func _wire_signals() -> void:
	continue_button.pressed.connect(_on_continue)
	new_game_button.pressed.connect(_open_setup)
	replay_button.pressed.connect(_on_replay_pressed)
	settings_button.pressed.connect(_open_settings)
	quit_button.pressed.connect(_on_quit)
	cancel_button.pressed.connect(_close_modal)
	confirm_button.pressed.connect(_on_modal_confirm)
	file_dialog.file_selected.connect(_on_replay_selected)


func _unhandled_input(event: InputEvent) -> void:
	if not (event is InputEventKey) or not event.pressed:
		return
	if event.keycode == KEY_ESCAPE and overlay.visible:
		_close_modal()
		get_viewport().set_input_as_handled()


func _refresh_continue() -> void:
	continue_button.disabled = not AppState.battle_in_progress
	continue_button.text = "继续对局" if AppState.battle_in_progress else "继续对局（暂无）"
	continue_button.tooltip_text = (
		"回到当前对局"
		if AppState.battle_in_progress
		else "开始一局后可以从这里继续"
	)


func _focus_default() -> void:
	if continue_button.disabled:
		new_game_button.grab_focus()
	else:
		continue_button.grab_focus()


func _on_continue() -> void:
	if not AppState.battle_in_progress:
		return
	AppState.battle_resume = true
	get_tree().change_scene_to_file(BATTLE_SCENE)


# ---------------------------------------------------------------------------
# 新对局设置
# ---------------------------------------------------------------------------


func _open_setup() -> void:
	_begin_modal("setup", "新对局")
	var cfg: Dictionary = AppState.battle_config

	_setup_bots_option = OptionButton.new()
	var bots_selected := 0
	for index in PLAYER_OPTIONS.size():
		var option: Dictionary = PLAYER_OPTIONS[index]
		_setup_bots_option.add_item(str(option["label"]), int(option["id"]))
		if int(option["id"]) == int(cfg.get("bots", 2)):
			bots_selected = index
	_setup_bots_option.select(bots_selected)
	_style_option(_setup_bots_option)
	_add_form_row("对手数量", _setup_bots_option)

	_setup_bot_option = OptionButton.new()
	var bot_selected := 0
	for index in BOT_OPTIONS.size():
		var option: Dictionary = BOT_OPTIONS[index]
		_setup_bot_option.add_item(str(option["label"]), index)
		_setup_bot_option.set_item_metadata(index, str(option["id"]))
		if str(option["id"]) == str(cfg.get("bot", "minimax")):
			bot_selected = index
	_setup_bot_option.select(bot_selected)
	_style_option(_setup_bot_option)
	_add_form_row("对手强度", _setup_bot_option)

	_setup_seed_spin = SpinBox.new()
	_setup_seed_spin.min_value = 1
	_setup_seed_spin.max_value = 999999
	_setup_seed_spin.step = 1
	_setup_seed_spin.value = int(cfg.get("seed", 42))
	_setup_seed_spin.custom_minimum_size = Vector2(180, 0)
	_add_form_row("随机种子", _setup_seed_spin)

	modal_body.add_child(Widgets.label(
		"同一组种子与对手强度可以复现同一局。", 12, Styles.COLOR_MUTED
	))
	if AppState.battle_in_progress:
		modal_body.add_child(Widgets.label(
			"当前进行中的对局会被放弃；想保留请先「取消」。",
			12,
			Styles.COLOR_YELLOW
		))
	cancel_button.text = "取消"
	confirm_button.text = "开始对局"
	confirm_button.focus_mode = Control.FOCUS_ALL
	_setup_bots_option.grab_focus()


func _collect_setup_config() -> Dictionary:
	var bots := 2
	if _setup_bots_option != null and _setup_bots_option.selected >= 0:
		bots = int(_setup_bots_option.get_item_id(_setup_bots_option.selected))
	var bot := "minimax"
	if _setup_bot_option != null and _setup_bot_option.selected >= 0:
		bot = str(_setup_bot_option.get_item_metadata(_setup_bot_option.selected))
	var seed := 42
	if _setup_seed_spin != null:
		seed = int(_setup_seed_spin.value)
	return {"bots": bots, "bot": bot, "seed": seed}


func _start_battle() -> void:
	AppState.battle_config = _collect_setup_config()
	AppState.battle_in_progress = true
	AppState.battle_resume = false
	get_tree().change_scene_to_file(BATTLE_SCENE)


# ---------------------------------------------------------------------------
# 设置
# ---------------------------------------------------------------------------


func _open_settings() -> void:
	_begin_modal("settings", "设置")

	_settings_ai_option = OptionButton.new()
	for index in AI_SPEED_LABELS.size():
		_settings_ai_option.add_item(str(AI_SPEED_LABELS[index]), index)
	_settings_ai_option.select(AppState.ai_speed_index)
	_style_option(_settings_ai_option)
	_add_form_row("AI 行动速度", _settings_ai_option)

	_settings_replay_option = OptionButton.new()
	for index in REPLAY_SPEED_LABELS.size():
		_settings_replay_option.add_item(str(REPLAY_SPEED_LABELS[index]), index)
	_settings_replay_option.select(AppState.replay_speed_index)
	_style_option(_settings_replay_option)
	_add_form_row("回放默认速度", _settings_replay_option)

	modal_body.add_child(Widgets.label(
		"设置保存在 user://settings.cfg，只影响动画节奏，不影响规则。",
		12,
		Styles.COLOR_MUTED
	))
	cancel_button.text = "取消"
	confirm_button.text = "保存"
	confirm_button.grab_focus()


func _save_settings() -> void:
	if _settings_ai_option != null:
		AppState.ai_speed_index = _settings_ai_option.selected
	if _settings_replay_option != null:
		AppState.replay_speed_index = _settings_replay_option.selected
	AppState.save_settings()
	_close_modal()


# ---------------------------------------------------------------------------
# 回放与退出
# ---------------------------------------------------------------------------


func _on_replay_pressed() -> void:
	var tmp_dir := _repo_dir().path_join("tmp")
	if DirAccess.dir_exists_absolute(tmp_dir):
		file_dialog.current_dir = tmp_dir
	file_dialog.popup_centered(Vector2i(960, 620))


func _on_replay_selected(path: String) -> void:
	AppState.replay_path = path
	get_tree().change_scene_to_file(REPLAY_SCENE)


func _on_quit() -> void:
	if not AppState.battle_in_progress:
		get_tree().quit()
		return
	_begin_modal("quit", "退出游戏")
	modal_body.add_child(Widgets.label(
		"进行中的对局只保存在本次运行里，退出后无法继续。",
		13,
		Styles.COLOR_YELLOW
	))
	cancel_button.text = "取消"
	confirm_button.text = "退出"
	cancel_button.grab_focus()


func _repo_dir() -> String:
	return ProjectSettings.globalize_path("res://").trim_suffix("/").get_base_dir()


# ---------------------------------------------------------------------------
# 模态与冒烟
# ---------------------------------------------------------------------------


func _begin_modal(kind: String, title: String) -> void:
	_modal_kind = kind
	overlay.visible = true
	modal_title.text = title
	Widgets.clear(modal_body)
	Widgets.clear(modal_options)
	cancel_button.visible = true
	confirm_button.visible = true
	confirm_button.disabled = false
	confirm_button.text = "确定"
	cancel_button.text = "取消"


func _add_form_row(label_text: String, control: Control) -> void:
	var row := HBoxContainer.new()
	row.add_theme_constant_override("separation", 12)
	var label := Widgets.label(label_text, 14, Styles.COLOR_MUTED)
	label.custom_minimum_size = Vector2(96, 0)
	label.vertical_alignment = VERTICAL_ALIGNMENT_CENTER
	row.add_child(label)
	control.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	row.add_child(control)
	modal_options.add_child(row)


func _style_option(option: OptionButton) -> void:
	Styles.style_button(option)
	option.custom_minimum_size = Vector2(0, 34)


func _close_modal() -> void:
	overlay.visible = false
	_modal_kind = ""
	Widgets.clear(modal_body)
	Widgets.clear(modal_options)
	_setup_bots_option = null
	_setup_bot_option = null
	_setup_seed_spin = null
	_settings_ai_option = null
	_settings_replay_option = null
	_focus_default()


func _on_modal_confirm() -> void:
	match _modal_kind:
		"setup":
			_start_battle()
		"settings":
			_save_settings()
		"quit":
			get_tree().quit()
		_:
			_close_modal()


func _run_smoke() -> void:
	await get_tree().process_frame
	var failures: Array[String] = []
	var was_in_progress := AppState.battle_in_progress

	if $Margin/Root/MenuButtons.get_child_count() != 5:
		failures.append("菜单按钮应为 5 个")
	AppState.battle_in_progress = false
	_refresh_continue()
	if not continue_button.disabled:
		failures.append("没有进行中对局时「继续对局」应为禁用")

	_open_setup()
	await get_tree().process_frame
	if not overlay.visible:
		failures.append("新对局设置未打开")
	if modal_body.get_child_count() < 1:
		failures.append("新对局设置缺少说明")
	if modal_options.get_child_count() != 3:
		failures.append("新对局设置应有三行参数，实际 %d" % modal_options.get_child_count())
	var cfg := _collect_setup_config()
	if int(cfg.get("bots", 0)) < 2 or int(cfg.get("bots", 0)) > 3:
		failures.append("对手数量默认值异常：%s" % str(cfg.get("bots")))
	_setup_bots_option.select(1)
	_setup_seed_spin.value = 321
	var changed := _collect_setup_config()
	if int(changed.get("bots", 0)) != 3 or int(changed.get("seed", 0)) != 321:
		failures.append("新对局参数未按控件取值：%s" % str(changed))

	_open_settings()
	await get_tree().process_frame
	if _settings_ai_option == null or _settings_ai_option.item_count != 3:
		failures.append("AI 速度选项异常")
	if _settings_replay_option == null or _settings_replay_option.item_count != 4:
		failures.append("回放速度选项异常")
	if modal_body.get_child_count() < 1:
		failures.append("设置页缺少说明")
	_close_modal()
	if overlay.visible:
		failures.append("设置页未关闭")

	if file_dialog.filters.is_empty():
		failures.append("回放文件对话框缺少过滤器")

	AppState.battle_in_progress = true
	_on_quit()
	if not overlay.visible or confirm_button.text != "退出":
		failures.append("进行中对局退出时缺少二次确认")
	_close_modal()

	AppState.battle_in_progress = was_in_progress
	_refresh_continue()

	if failures.is_empty():
		print("[menu-smoke] OK 按钮、设置页、回放对话框与默认值")
		get_tree().quit(0)
		return
	for message in failures:
		printerr("[menu-smoke] FAIL " + message)
	get_tree().quit(1)

