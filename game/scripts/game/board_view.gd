extends Control
## 场上牌自绘视图：五行（1/2/3 阶、稀有、传说）。
##
## 渲染、悬停高亮、点击命中与键盘浏览。回放注入只读视图；对战额外注入
## 由 legal_actions 派生出的可操作集合：
## - 左键和右键都是「选中」，不会直接提交动作；动作按钮由 battle.gd 贴卡片显示；
## - 既不能捕捉也不能保留的牌会降亮度；
## - 选中牌抬起并显示金色描边。
##
## 键盘：Tab 聚焦后，方向键移动焦点，Enter / 空格选中。

signal slot_clicked(row: int, slot: int, card: Dictionary)

const Styles := preload("res://scripts/ui/styles.gd")
const Widgets := preload("res://scripts/ui/widgets.gd")

const ROWS := [
	{"board_key": "level1", "label": "1阶", "accent": Color("6f8f66"), "slots": 4},
	{"board_key": "level2", "label": "2阶", "accent": Color("4b8ee5"), "slots": 4},
	{"board_key": "level3", "label": "3阶", "accent": Color("b07bd8"), "slots": 4},
	{"board_key": "rare", "label": "稀有", "accent": Color("f0c15a"), "slots": 1},
	{"board_key": "legendary", "label": "传说", "accent": Color("ff7a59"), "slots": 1},
]

const ROW_GAP := 8.0
const LABEL_W := 40.0
const CARD_GAP := 8.0
const MAX_SLOTS := 4.0
const MIN_CARD_W := 80.0
const COMPACT_H := 72.0
const LIFT := 4.0

var _board: Dictionary = {}
var _hover := Vector2i(-1, -1)
var _selected := Vector2i(-1, -1)
var _focused := Vector2i(-1, -1)
var _interactive := false
var _actionable: Dictionary = {}
var _boxes: Dictionary = {}


func _ready() -> void:
	mouse_filter = Control.MOUSE_FILTER_STOP
	clip_contents = true
	focus_mode = Control.FOCUS_ALL
	resized.connect(queue_redraw)
	mouse_exited.connect(_on_mouse_exited)
	focus_entered.connect(_on_focus_entered)
	focus_exited.connect(_on_focus_exited)


## 注入一帧的 `view.board`。空字典表示尚未载入数据。
func set_view(board: Dictionary) -> void:
	_board = board
	_hover = Vector2i(-1, -1)
	_selected = Vector2i(-1, -1)
	_focused = Vector2i(-1, -1)
	_boxes.clear()
	tooltip_text = ""
	queue_redraw()


## 可操作集合：键是 "row:slot"，值形如 {"capture": true, "reserve": true}。
func set_actionable(actionable: Dictionary) -> void:
	_actionable = actionable.duplicate(true)
	queue_redraw()


## 交互模式：开启后点击会选中并发出 slot_clicked；回放界面保持关闭。
func set_interactive(enabled: bool) -> void:
	_interactive = enabled
	mouse_default_cursor_shape = (
		Control.CURSOR_POINTING_HAND if enabled else Control.CURSOR_ARROW
	)
	if not enabled:
		_hover = Vector2i(-1, -1)
		_selected = Vector2i(-1, -1)
		tooltip_text = ""
	queue_redraw()


## 当前选中的 (行, 槽位)；(-1, -1) 表示无选择。
func set_selected(key: Vector2i) -> void:
	_selected = key
	queue_redraw()


func selected() -> Vector2i:
	return _selected


## 选中卡片在本控件内的矩形；battle.gd 用它定位贴卡动作条。
func selection_rect() -> Rect2:
	if not _boxes.has(_selected):
		return Rect2()
	return _boxes[_selected]


## 当前注入的 board（冒烟测试用）。
func board() -> Dictionary:
	return _board


func _draw() -> void:
	_boxes.clear()
	if _board.is_empty():
		_draw_empty_state()
		return

	var font := get_theme_default_font()
	if font == null:
		font = ThemeDB.fallback_font

	var row_count := ROWS.size()
	var row_h := (size.y - ROW_GAP * float(row_count - 1)) / float(row_count)
	var card_w := (size.x - LABEL_W - CARD_GAP - CARD_GAP * (MAX_SLOTS - 1.0)) / MAX_SLOTS
	card_w = maxf(card_w, MIN_CARD_W)
	var compact := row_h < COMPACT_H

	for row_index in row_count:
		var row: Dictionary = ROWS[row_index]
		var row_origin := Vector2(0.0, float(row_index) * (row_h + ROW_GAP))
		_draw_row_label(font, row, Rect2(row_origin, Vector2(LABEL_W, row_h)))

		var slots: Array = []
		var raw: Variant = _board.get(str(row["board_key"]), [])
		if raw is Array:
			slots = raw
		for slot_index in int(row["slots"]):
			var box := Rect2(
				Vector2(
					LABEL_W + CARD_GAP + float(slot_index) * (card_w + CARD_GAP),
					row_origin.y
				),
				Vector2(card_w, row_h)
			)
			_boxes[Vector2i(row_index, slot_index)] = box
			var card: Variant = slots[slot_index] if slot_index < slots.size() else null
			var key := Vector2i(row_index, slot_index)
			var legal := _actionable.has(_key_text(key))
			var display := box
			if key == _selected or key == _hover:
				var lift := minf(LIFT, box.position.y)
				display.position.y -= lift
				display.size.y += lift
			if card is Dictionary:
				_draw_card(font, display, card, row["accent"], compact)
				if _interactive and not _actionable.is_empty() and not legal:
					draw_rect(display, Color(0.04, 0.05, 0.08, 0.58))
				if legal:
					_draw_legal_dots(display, key)
			else:
				_draw_empty_slot(font, display, compact)
			if _hover == key:
				draw_style_box(Styles.border_box(Styles.COLOR_ACCENT, 2, 10), display)
			if has_focus() and _focused == key:
				draw_style_box(Styles.border_box(Styles.COLOR_ACCENT, 2, 10), display)
			if _selected == key:
				draw_style_box(Styles.border_box(Styles.COLOR_GOLD, 3, 10), display)


## 左上角小圆点：金色 = 可捕捉，蓝色 = 可保留，两个都有则并排。
func _draw_legal_dots(box: Rect2, key: Vector2i) -> void:
	var entry: Dictionary = _actionable.get(_key_text(key), {})
	var center := Vector2(box.position.x + 9.0, box.position.y + 9.0)
	if bool(entry.get("capture", false)):
		draw_circle(center, 4.0, Styles.COLOR_GOLD)
		center.x += 11.0
	if bool(entry.get("reserve", false)):
		draw_circle(center, 4.0, Styles.COLOR_ACCENT)
func _draw_row_label(font: Font, row: Dictionary, rect: Rect2) -> void:
	var label := str(row["label"])
	var font_size := 12
	var width := font.get_string_size(label, HORIZONTAL_ALIGNMENT_LEFT, -1, font_size).x
	var baseline := rect.position.y + rect.size.y * 0.5 + font_size * 0.36
	draw_string(
		font,
		Vector2(rect.position.x + (rect.size.x - width) * 0.5, baseline),
		label,
		HORIZONTAL_ALIGNMENT_LEFT,
		-1,
		font_size,
		Styles.COLOR_MUTED
	)


func _draw_card(font: Font, box: Rect2, card: Dictionary, _accent: Color, compact: bool) -> void:
	var bonus_key := str(card.get("bonus_color", "red"))
	var bonus_color: Color = Styles.TOKEN_COLORS.get(bonus_key, Styles.COLOR_ACCENT)
	draw_style_box(
		Styles.panel_box(
			Styles.COLOR_PANEL_2.lerp(bonus_color, 0.22),
			Styles.COLOR_LINE.lerp(bonus_color, 0.60),
			10
		),
		box
	)

	var pad := 7.0 if compact else 10.0
	var inner_x := box.position.x + pad
	var inner_w := box.size.x - pad * 2.0
	var height := box.size.y
	var small_size := 8 if compact else 9
	var name := str(card.get("name_zh", "?"))
	var glyph_limit := minf(
		32.0 if compact else 50.0, height * (0.38 if compact else 0.42)
	)
	var glyph_size := int(maxf(
		20.0, minf(glyph_limit, (inner_w - 8.0) / maxf(float(name.length()), 1.0))
	))

	# 分数徽章（右上角）：黄色方块，与精灵球区分。
	var points := int(card.get("points", 0))
	var badge_side := 13.0 if compact else 18.0
	var badge_rect := Rect2(
		Vector2(box.position.x + box.size.x - pad - badge_side, box.position.y + pad),
		Vector2(badge_side, badge_side)
	)
	draw_style_box(
		Styles.score_box(Styles.COLOR_GOLD if points > 0 else Color("2a3140")),
		badge_rect
	)
	var badge_font_size := 10 if compact else 11
	draw_string(
		font,
		Vector2(
			badge_rect.position.x,
			badge_rect.position.y + badge_rect.size.y * 0.5 + badge_font_size * 0.36
		),
		str(points),
		HORIZONTAL_ALIGNMENT_CENTER,
		badge_rect.size.x,
		badge_font_size,
		Color("1a1a22") if points > 0 else Styles.COLOR_MUTED
	)

	# 中央大字：完整名字，按名字长度自适应字号。
	var glyph := name
	var glyph_w := font.get_string_size(glyph, HORIZONTAL_ALIGNMENT_LEFT, -1, glyph_size).x
	var glyph_pos := Vector2(
		box.position.x + (box.size.x - glyph_w) * 0.5,
		box.position.y + height * (0.46 if compact else 0.48) + glyph_size * 0.36
	)
	draw_string(
		font,
		glyph_pos + Vector2(1.5, 2.0),
		glyph,
		HORIZONTAL_ALIGNMENT_LEFT,
		-1,
		glyph_size,
		Color(0, 0, 0, 0.35)
	)
	draw_string(
		font,
		glyph_pos,
		glyph,
		HORIZONTAL_ALIGNMENT_LEFT,
		-1,
		glyph_size,
		Color("f4f7ff")
	)

	# 捕捉费用：左下角彩色球 + 数字；零费用写「免费」。
	var cost: Dictionary = {}
	var cost_raw: Variant = card.get("cost", {})
	if cost_raw is Dictionary:
		cost = cost_raw
	var ball_radius := 6.0 if compact else 7.5
	var cost_center_y := box.position.y + height - pad - ball_radius
	var number_size := 8 if compact else 9
	var cursor := inner_x + ball_radius
	var has_cost := false
	for key in Styles.COLOR_KEYS:
		var count := int(cost.get(key, 0))
		if count <= 0:
			continue
		has_cost = true
		_draw_ball(font, Vector2(cursor, cost_center_y), ball_radius, key, count, number_size)
		cursor += ball_radius * 2.0 + 3.0
	if not has_cost:
		draw_string(
			font,
			Vector2(inner_x, cost_center_y + small_size * 0.36),
			"免费",
			HORIZONTAL_ALIGNMENT_LEFT,
			-1,
			small_size,
			Styles.COLOR_MUTED
		)

	# 进化需求（紧凑模式隐藏；放不下时省略）。
	if compact:
		return
	var evolves_to := str(card.get("evolves_to_zh", ""))
	var requirement_raw: Variant = card.get("evolution_requirement")
	if evolves_to.is_empty() or not (requirement_raw is Dictionary):
		return
	var requirement: Dictionary = requirement_raw
	var evo_color := str(requirement.get("color", "red"))
	var evo_count := int(requirement.get("count", 0))
	var evo_prefix := "→ %s · " % evolves_to
	var evo_ball_radius := 6.0
	var evo_text_w := font.get_string_size(
		evo_prefix, HORIZONTAL_ALIGNMENT_LEFT, -1, small_size
	).x
	var evo_w := evo_text_w + evo_ball_radius * 2.0
	var evo_x := box.position.x + box.size.x - pad - evo_w
	if evo_x < cursor + 6.0:
		return
	draw_string(
		font,
		Vector2(evo_x, cost_center_y + small_size * 0.36),
		evo_prefix,
		HORIZONTAL_ALIGNMENT_LEFT,
		-1,
		small_size,
		Color("d9c9ff")
	)
	_draw_ball(
		font,
		Vector2(evo_x + evo_text_w + evo_ball_radius, cost_center_y),
		evo_ball_radius,
		evo_color,
		evo_count,
		8
	)


## 编号精灵球：彩色正圆 + 浅色描边，数字写在球内（与 HTML 卡面 .ball 一致）。
func _draw_ball(
	font: Font, center: Vector2, radius: float, key: String, count: int, font_size: int
) -> void:
	var ball_color: Color = Styles.TOKEN_COLORS.get(key, Styles.COLOR_ACCENT)
	draw_circle(center, radius, ball_color)
	draw_arc(center, radius, 0.0, TAU, 24, Color(1, 1, 1, 0.35), 1.0, true)
	var label := str(count)
	var label_w := font.get_string_size(label, HORIZONTAL_ALIGNMENT_LEFT, -1, font_size).x
	draw_string(
		font,
		Vector2(center.x - label_w * 0.5, center.y + font_size * 0.36),
		label,
		HORIZONTAL_ALIGNMENT_LEFT,
		-1,
		font_size,
		Styles.ball_ink(key)
	)


func _draw_empty_slot(font: Font, box: Rect2, compact: bool) -> void:
	draw_style_box(Styles.panel_box(Color("141a24"), Color("242c3a"), 10), box)
	var font_size := 11 if compact else 12
	draw_string(
		font,
		Vector2(box.position.x, box.position.y + box.size.y * 0.5 + font_size * 0.36),
		"空",
		HORIZONTAL_ALIGNMENT_CENTER,
		box.size.x,
		font_size,
		Color(0.35, 0.40, 0.50, 0.8)
	)


func _draw_empty_state() -> void:
	var font := get_theme_default_font()
	if font == null:
		font = ThemeDB.fallback_font
	var center_y := size.y * 0.44
	draw_string(
		font,
		Vector2(0.0, center_y),
		"尚未载入对局数据",
		HORIZONTAL_ALIGNMENT_CENTER,
		size.x,
		20,
		Styles.COLOR_MUTED
	)
	draw_string(
		font,
		Vector2(0.0, center_y + 30.0),
		"对战：先选一张牌，或点击右侧精灵球",
		HORIZONTAL_ALIGNMENT_CENTER,
		size.x,
		13,
		Color("66728a")
	)
	draw_string(
		font,
		Vector2(0.0, center_y + 52.0),
		"回放：从标题菜单选择 CLI 导出的 JSON",
		HORIZONTAL_ALIGNMENT_CENTER,
		size.x,
		12,
		Color("4e586c")
	)


func _gui_input(event: InputEvent) -> void:
	if event is InputEventKey:
		_handle_key(event)
		return
	if event is InputEventMouseMotion:
		var found := _slot_at(event.position)
		if found != _hover:
			_hover = found
			tooltip_text = _tooltip_for(found)
			queue_redraw()
		return
	if not (event is InputEventMouseButton) or not event.pressed:
		return
	if not _interactive:
		return
	if event.button_index != MOUSE_BUTTON_LEFT and event.button_index != MOUSE_BUTTON_RIGHT:
		return
	var key := _slot_at(event.position)
	if key.x < 0:
		_activate(Vector2i(-1, -1), {})
		return
	var card := _card_at(key)
	if card.is_empty():
		return
	_activate(key, card)


func _handle_key(event: InputEventKey) -> void:
	if not event.pressed or event.echo:
		return
	if not _interactive:
		return
	var delta := Vector2i.ZERO
	match event.keycode:
		KEY_LEFT:
			delta = Vector2i(0, -1)
		KEY_RIGHT:
			delta = Vector2i(0, 1)
		KEY_UP:
			delta = Vector2i(-1, 0)
		KEY_DOWN:
			delta = Vector2i(1, 0)
		KEY_ENTER, KEY_KP_ENTER, KEY_SPACE:
			if _focused.x < 0:
				_focused = _first_card()
			if _focused.x < 0:
				return
			var card := _card_at(_focused)
			if card.is_empty():
				return
			_activate(_focused, card)
			accept_event()
			return
		_:
			return

	if _focused.x < 0:
		_focused = _first_card()
	else:
		_focused = _nearest(_focused, delta)
	if _focused.x < 0:
		return
	_hover = _focused
	tooltip_text = _tooltip_for(_focused)
	queue_redraw()
	accept_event()


## 点击与键盘共用：选中并把选择交给 battle.gd（空 card 表示清空）。
func _activate(key: Vector2i, card: Dictionary) -> void:
	_selected = key
	_focused = key
	_hover = key
	queue_redraw()
	slot_clicked.emit(key.x, key.y, card)


func _nearest(from: Vector2i, delta: Vector2i) -> Vector2i:
	var key := from
	for _step in 4:
		var next := key + delta
		if next.x < 0 or next.x >= ROWS.size():
			break
		var slots := int(ROWS[next.x]["slots"])
		next.y = clampi(next.y, 0, slots - 1)
		if next == key:
			break
		key = next
		if not _card_at(key).is_empty():
			return key
	return from


func _first_card() -> Vector2i:
	for row_index in ROWS.size():
		var slots := int(ROWS[row_index]["slots"])
		for slot_index in slots:
			if not _card_at(Vector2i(row_index, slot_index)).is_empty():
				return Vector2i(row_index, slot_index)
	return Vector2i(-1, -1)


func _key_text(key: Vector2i) -> String:
	return "%d:%d" % [key.x, key.y]


func _slot_at(position: Vector2) -> Vector2i:
	for key in _boxes:
		var rect: Rect2 = _boxes[key]
		if rect.has_point(position):
			return key
	return Vector2i(-1, -1)


func _card_at(key: Vector2i) -> Dictionary:
	if key.x < 0 or key.x >= ROWS.size():
		return {}
	var row: Dictionary = ROWS[key.x]
	var slots_count := int(row["slots"])
	if key.y < 0 or key.y >= slots_count:
		return {}
	var raw: Variant = _board.get(str(row["board_key"]), [])
	if not (raw is Array):
		return {}
	var slots: Array = raw
	if key.y >= slots.size():
		return {}
	var card: Variant = slots[key.y]
	return card if card is Dictionary else {}


func _on_mouse_exited() -> void:
	if _hover == Vector2i(-1, -1):
		return
	_hover = Vector2i(-1, -1)
	tooltip_text = ""
	queue_redraw()


func _on_focus_entered() -> void:
	if _focused.x < 0:
		_focused = _first_card()
	if _focused.x >= 0:
		_hover = _focused
		tooltip_text = _tooltip_for(_focused)
	queue_redraw()


func _on_focus_exited() -> void:
	_hover = Vector2i(-1, -1)
	tooltip_text = ""
	queue_redraw()


func _tooltip_for(key: Vector2i) -> String:
	if key.x < 0 or key.x >= ROWS.size():
		return ""
	var row: Dictionary = ROWS[key.x]
	var card := _card_at(key)
	if card.is_empty():
		return ""

	var lines: Array[String] = []
	var points := int(card.get("points", 0))
	lines.append("%s · %d 分" % [str(card.get("name_zh", "?")), points])

	var bonus_key := str(card.get("bonus_color", "red"))
	lines.append("减免 %s +%d" % [
		Styles.COLOR_LABELS.get(bonus_key, "?"),
		int(card.get("bonus_count", 0)),
	])

	var cost_raw: Variant = card.get("cost", {})
	var cost_parts: Array[String] = []
	if cost_raw is Dictionary:
		var cost: Dictionary = cost_raw
		for color_key in Styles.COLOR_KEYS:
			var count := int(cost.get(color_key, 0))
			if count > 0:
				cost_parts.append("%s%d" % [Styles.COLOR_LABELS.get(color_key, "?"), count])
	lines.append("费用：" + (" ".join(cost_parts) if not cost_parts.is_empty() else "免费"))

	var chain := Widgets.chain_text(card)
	if not chain.is_empty():
		lines.append("进化链：" + chain)

	var evolves_to := str(card.get("evolves_to_zh", ""))
	var requirement_raw: Variant = card.get("evolution_requirement")
	if not evolves_to.is_empty() and requirement_raw is Dictionary:
		var requirement: Dictionary = requirement_raw
		lines.append("进化 → %s（需 %d%s减免）" % [
			evolves_to,
			int(requirement.get("count", 0)),
			Styles.COLOR_LABELS.get(str(requirement.get("color", "red")), "?"),
		])
	elif str(row["board_key"]) == "rare":
		lines.append("稀有牌：不可保留、不可进化")
	elif str(row["board_key"]) == "legendary":
		lines.append("传说牌：不可保留、不可进化")

	if _interactive and not _actionable.is_empty():
		var entry: Dictionary = _actionable.get(_key_text(key), {})
		var actions: Array[String] = []
		if bool(entry.get("capture", false)):
			actions.append("捕捉")
		if bool(entry.get("reserve", false)):
			actions.append("保留")
		lines.append("可执行：" + (" / ".join(actions) if not actions.is_empty() else "当前不可操作"))
	return "\n".join(lines)

