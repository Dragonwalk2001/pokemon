extends Control
## 可点击的精灵球计量条。
##
## 供应区（拿球）与弃球弹窗共用：每个球堆显示颜色、存量与已选数量。
## 左键 +1，右键 −1；`blocked` 中的颜色不可点（例如大师球、已满 2 个的同色）。
## 只负责显示与命中，选择是否合法由 battle.gd 校验。

signal pile_clicked(key: String)
signal pile_right_clicked(key: String)

const Styles := preload("res://scripts/ui/styles.gd")
const KEYS := ["red", "blue", "yellow", "pink", "black", "master"]
const GAP := 6.0

var _counts: Dictionary = {}
var _picked: Dictionary = {}
var _blocked: Dictionary = {}
var _enabled := true
var _hover := ""
var _boxes: Dictionary = {}


func _ready() -> void:
	mouse_filter = Control.MOUSE_FILTER_STOP
	mouse_default_cursor_shape = Control.CURSOR_POINTING_HAND
	resized.connect(queue_redraw)
	mouse_exited.connect(_on_mouse_exited)


## counts：每个颜色的存量；picked：已选数量；blocked：不可点的颜色。
func set_state(
	counts: Dictionary, picked: Dictionary, enabled: bool, blocked: Array = []
) -> void:
	_counts = counts.duplicate()
	_picked = picked.duplicate()
	_enabled = enabled
	_blocked.clear()
	for key in blocked:
		_blocked[str(key)] = true
	queue_redraw()


func picked() -> Dictionary:
	return _picked.duplicate()


func _draw() -> void:
	_boxes.clear()
	var font := get_theme_default_font()
	if font == null:
		font = ThemeDB.fallback_font
	var cell_w := (size.x - GAP * float(KEYS.size() - 1)) / float(KEYS.size())
	if cell_w <= 1.0 or size.y <= 1.0:
		return
	var cell_h := minf(size.y, 58.0)
	var top := (size.y - cell_h) * 0.5
	for index in KEYS.size():
		var key: String = KEYS[index]
		var rect := Rect2(float(index) * (cell_w + GAP), top, cell_w, cell_h)
		_boxes[key] = rect
		_draw_pile(font, key, rect)


func _draw_pile(font: Font, key: String, rect: Rect2) -> void:
	var color: Color = Styles.TOKEN_COLORS.get(key, Styles.COLOR_ACCENT)
	var count := int(_counts.get(key, 0))
	var chosen := int(_picked.get(key, 0))
	var available := _enabled and not _blocked.has(key) and count > 0
	var hovered := _hover == key and available

	var bg := Styles.COLOR_PANEL_2.lerp(color, 0.14 if count > 0 else 0.03)
	if hovered:
		bg = bg.lerp(color, 0.22)
	var border := Styles.COLOR_LINE
	if count > 0:
		border = Styles.COLOR_LINE.lerp(color, 0.45)
	if chosen > 0:
		border = Styles.COLOR_GOLD
	if hovered:
		border = color.lightened(0.15)
	draw_style_box(Styles.tile_box(bg, border, 10, 0, 0), rect)

	var dot_color := color if count > 0 else color.darkened(0.55)
	draw_circle(Vector2(rect.position.x + 14.0, rect.position.y + 16.0), 7.0, dot_color)

	var label := str(Styles.COLOR_LABELS.get(key, key))
	var label_color := Styles.COLOR_MUTED if count > 0 else Color(0.42, 0.47, 0.56)
	draw_string(
		font,
		Vector2(rect.position.x + 26.0, rect.position.y + 21.0),
		label,
		HORIZONTAL_ALIGNMENT_LEFT,
		-1,
		12,
		label_color
	)
	draw_string(
		font,
		Vector2(rect.position.x + 6.0, rect.position.y + rect.size.y - 9.0),
		str(count),
		HORIZONTAL_ALIGNMENT_LEFT,
		-1,
		15,
		Styles.COLOR_TEXT if count > 0 else Color(0.42, 0.47, 0.56)
	)
	if chosen > 0:
		var badge := "+%d" % chosen
		var badge_w := font.get_string_size(badge, HORIZONTAL_ALIGNMENT_LEFT, -1, 11).x
		draw_string(
			font,
			Vector2(rect.position.x + rect.size.x - badge_w - 6.0, rect.position.y + rect.size.y - 9.0),
			badge,
			HORIZONTAL_ALIGNMENT_LEFT,
			-1,
			11,
			Styles.COLOR_GOLD
		)
	# 不可点的球堆整体压暗，保持数字仍可读。
	if _blocked.has(key):
		draw_rect(rect, Color(0.04, 0.05, 0.08, 0.42))


func _gui_input(event: InputEvent) -> void:
	if event is InputEventMouseMotion:
		var found := _key_at(event.position)
		if found != _hover:
			_hover = found
			tooltip_text = _tooltip_for(found)
			queue_redraw()
		return
	if not (event is InputEventMouseButton) or not event.pressed:
		return
	var key := _key_at(event.position)
	if key.is_empty() or not _enabled or _blocked.has(key):
		return
	if event.button_index == MOUSE_BUTTON_LEFT:
		pile_clicked.emit(key)
	elif event.button_index == MOUSE_BUTTON_RIGHT:
		pile_right_clicked.emit(key)


func _key_at(position: Vector2) -> String:
	for key in _boxes:
		var rect: Rect2 = _boxes[key]
		if rect.has_point(position):
			return str(key)
	return ""


func _tooltip_for(key: String) -> String:
	if key.is_empty():
		return ""
	var count := int(_counts.get(key, 0))
	var chosen := int(_picked.get(key, 0))
	var text := "%s精灵球 · 现有 %d" % [str(Styles.COLOR_LABELS.get(key, key)), count]
	if chosen > 0:
		text += " · 已选 %d" % chosen
	if key == "master":
		return text + "\n大师球只能通过保留获得"
	if _blocked.has(key):
		return text + "\n当前选择下不可再拿"
	return text + "\n左键 +1 · 右键 −1"


func _on_mouse_exited() -> void:
	if _hover.is_empty():
		return
	_hover = ""
	tooltip_text = ""
	queue_redraw()
