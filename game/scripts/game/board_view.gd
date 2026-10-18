extends Control
## 场上牌自绘视图：五行（1/2/3 阶、稀有、传说）。
##
## 只负责渲染与悬停高亮，回放状态由 main.gd 通过 `set_view()` 注入。
## 每张牌画出名字、分数、减免、费用与进化需求；行高不足时自动隐藏进化行。
## 鼠标悬停会高亮牌面，并用 tooltip 展示完整卡牌信息。

const Styles := preload("res://scripts/ui/styles.gd")

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

var _board: Dictionary = {}
var _hover := Vector2i(-1, -1)
var _boxes: Dictionary = {}  # Vector2i(row, slot) -> Rect2


func _ready() -> void:
	mouse_filter = Control.MOUSE_FILTER_STOP
	clip_contents = true
	resized.connect(queue_redraw)
	mouse_exited.connect(_on_mouse_exited)


## 注入一帧的 `view.board`。空字典表示尚未载入回放。
func set_view(board: Dictionary) -> void:
	_board = board
	_hover = Vector2i(-1, -1)
	_boxes.clear()
	tooltip_text = ""
	queue_redraw()


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
			if card is Dictionary:
				_draw_card(font, box, card, row["accent"], compact)
			else:
				_draw_empty_slot(font, box, compact)
			if _hover == Vector2i(row_index, slot_index):
				draw_style_box(Styles.border_box(Styles.COLOR_ACCENT, 2, 10), box)


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


func _draw_card(font: Font, box: Rect2, card: Dictionary, accent: Color, compact: bool) -> void:
	draw_style_box(Styles.panel_box(Styles.COLOR_PANEL_2, Styles.COLOR_LINE, 10), box)

	var pad := 10.0
	var inner_x := box.position.x + pad
	var inner_w := box.size.x - pad * 2.0
	var height := box.size.y
	var name_size := 11 if compact else (12 if height < 100.0 else 14)
	var small_size := 9 if compact else (10 if height < 100.0 else 11)
	var name_baseline := box.position.y + height * 0.26
	var bonus_baseline := box.position.y + height * 0.48
	var cost_baseline := box.position.y + height * 0.68

	# 顶部类别色条（HTML 回放的 border-top 对应物）。
	draw_line(
		Vector2(box.position.x + 9.0, box.position.y + 2.0),
		Vector2(box.position.x + box.size.x - 9.0, box.position.y + 2.0),
		accent,
		3.0
	)

	# 分数徽章。
	var points := int(card.get("points", 0))
	var badge_radius := 8.0 if compact else (9.0 if height < 100.0 else 11.0)
	var badge_center := Vector2(
		box.position.x + box.size.x - pad - badge_radius,
		box.position.y + 7.0 + badge_radius
	)
	draw_circle(badge_center, badge_radius, Styles.COLOR_GOLD if points > 0 else Color("2a3140"))
	var badge_font_size := 11 if compact else 12
	var badge_text := str(points)
	var badge_text_w := font.get_string_size(
		badge_text, HORIZONTAL_ALIGNMENT_LEFT, -1, badge_font_size
	).x
	draw_string(
		font,
		Vector2(badge_center.x - badge_text_w * 0.5, badge_center.y + badge_font_size * 0.36),
		badge_text,
		HORIZONTAL_ALIGNMENT_LEFT,
		-1,
		badge_font_size,
		Color("1a1a22") if points > 0 else Styles.COLOR_MUTED
	)

	# 名字。
	var name := str(card.get("name_zh", "?"))
	draw_string(
		font,
		Vector2(inner_x, name_baseline),
		name,
		HORIZONTAL_ALIGNMENT_LEFT,
		inner_w - badge_radius * 2.0 - 8.0,
		name_size,
		Styles.COLOR_TEXT
	)

	# 减免。
	var bonus_key := str(card.get("bonus_color", "red"))
	var bonus_count := int(card.get("bonus_count", 0))
	var bonus_color: Color = Styles.TOKEN_COLORS.get(bonus_key, Styles.COLOR_ACCENT)
	draw_circle(Vector2(inner_x + 4.0, bonus_baseline - 4.0), 4.0, bonus_color)
	draw_string(
		font,
		Vector2(inner_x + 13.0, bonus_baseline),
		"减免 %s +%d" % [Styles.COLOR_LABELS.get(bonus_key, "?"), bonus_count],
		HORIZONTAL_ALIGNMENT_LEFT,
		inner_w - 13.0,
		small_size,
		Styles.COLOR_MUTED
	)

	# 费用：彩色圆点 + 数量；零费用写「免费」。
	var cost: Dictionary = {}
	var cost_raw: Variant = card.get("cost", {})
	if cost_raw is Dictionary:
		cost = cost_raw
	var cursor := inner_x
	var has_cost := false
	for key in Styles.COLOR_KEYS:
		var count := int(cost.get(key, 0))
		if count <= 0:
			continue
		has_cost = true
		var label := str(count)
		var label_w := font.get_string_size(label, HORIZONTAL_ALIGNMENT_LEFT, -1, small_size).x
		if cursor + 10.0 + label_w > inner_x + inner_w:
			break
		draw_circle(Vector2(cursor + 3.5, cost_baseline - 4.0), 3.5, Styles.TOKEN_COLORS[key])
		draw_string(
			font,
			Vector2(cursor + 10.0, cost_baseline),
			label,
			HORIZONTAL_ALIGNMENT_LEFT,
			-1,
			small_size,
			Styles.COLOR_TEXT
		)
		cursor += 10.0 + label_w + 7.0
	if not has_cost:
		draw_string(
			font,
			Vector2(inner_x, cost_baseline),
			"免费",
			HORIZONTAL_ALIGNMENT_LEFT,
			-1,
			small_size,
			Styles.COLOR_MUTED
		)

	# 进化需求（紧凑模式隐藏）。
	if compact:
		return
	var evolves_to := str(card.get("evolves_to_zh", ""))
	var requirement_raw: Variant = card.get("evolution_requirement")
	if evolves_to.is_empty() or not (requirement_raw is Dictionary):
		return
	var requirement: Dictionary = requirement_raw
	var requirement_key := str(requirement.get("color", "red"))
	var requirement_count := int(requirement.get("count", 0))
	draw_string(
		font,
		Vector2(inner_x, box.position.y + height * 0.87),
		"→ %s · 需 %d%s减免" % [
			evolves_to,
			requirement_count,
			Styles.COLOR_LABELS.get(requirement_key, "?"),
		],
		HORIZONTAL_ALIGNMENT_LEFT,
		inner_w,
		small_size,
		Color("c9a7ff")
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
		"尚未加载回放",
		HORIZONTAL_ALIGNMENT_CENTER,
		size.x,
		20,
		Styles.COLOR_MUTED
	)
	draw_string(
		font,
		Vector2(0.0, center_y + 30.0),
		"点击右上角「打开回放…」，选择 CLI 生成的 JSON",
		HORIZONTAL_ALIGNMENT_CENTER,
		size.x,
		13,
		Color("66728a")
	)
	draw_string(
		font,
		Vector2(0.0, center_y + 52.0),
		"backend/ 下运行 bot-vs-bot 或 pve 会写入 tmp/*.json",
		HORIZONTAL_ALIGNMENT_CENTER,
		size.x,
		12,
		Color("4e586c")
	)


func _gui_input(event: InputEvent) -> void:
	if event is InputEventMouseMotion:
		var found := Vector2i(-1, -1)
		for key in _boxes:
			var rect: Rect2 = _boxes[key]
			if rect.has_point(event.position):
				found = key
				break
		if found != _hover:
			_hover = found
			tooltip_text = _tooltip_for(found)
			queue_redraw()


func _on_mouse_exited() -> void:
	if _hover == Vector2i(-1, -1):
		return
	_hover = Vector2i(-1, -1)
	tooltip_text = ""
	queue_redraw()


func _tooltip_for(key: Vector2i) -> String:
	if key.x < 0 or key.x >= ROWS.size():
		return ""
	var row: Dictionary = ROWS[key.x]
	var raw: Variant = _board.get(str(row["board_key"]), [])
	if not (raw is Array):
		return ""
	var slots: Array = raw
	if key.y < 0 or key.y >= slots.size():
		return ""
	var card: Variant = slots[key.y]
	if not (card is Dictionary):
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
	return "\n".join(lines)

