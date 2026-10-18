extends RefCounted
## 交互式界面的小型控件工厂。
##
## 只做外观，不含规则；回放界面保留自己的实例方法，避免影响既有布局。

const Styles := preload("res://scripts/ui/styles.gd")


static func panel(box: StyleBoxFlat) -> PanelContainer:
	var node := PanelContainer.new()
	node.add_theme_stylebox_override("panel", box)
	return node


static func label(text: String, font_size: int = 15, color: Color = Styles.COLOR_TEXT) -> Label:
	var node := Label.new()
	node.text = text
	node.add_theme_font_size_override("font_size", font_size)
	node.add_theme_color_override("font_color", color)
	return node


static func dot(color: Color) -> PanelContainer:
	var node := panel(Styles.dot_box(color))
	node.custom_minimum_size = Vector2(8, 8)
	node.size_flags_vertical = Control.SIZE_SHRINK_CENTER
	return node


static func pill(text: String, color: Color, dim: bool = false) -> PanelContainer:
	var node := panel(Styles.pill_box())
	node.size_flags_vertical = Control.SIZE_SHRINK_CENTER
	if dim:
		node.modulate = Color(1, 1, 1, 0.45)
	var row := HBoxContainer.new()
	row.add_theme_constant_override("separation", 4)
	node.add_child(row)
	row.add_child(dot(color))
	row.add_child(label(text, 11, Styles.COLOR_TEXT))
	return node


static func chip(text: String) -> PanelContainer:
	var node := panel(Styles.chip_box())
	node.size_flags_vertical = Control.SIZE_SHRINK_CENTER
	node.add_child(label(text, 10, Styles.COLOR_MUTED))
	return node


static func badge(text: String) -> PanelContainer:
	var node := panel(Styles.tile_box(Color("2d4a9e"), Color("3f63c8"), 8, 7, 1))
	node.size_flags_vertical = Control.SIZE_SHRINK_CENTER
	node.add_child(label(text, 10, Color("dfe8ff")))
	return node


static func spacer() -> Control:
	var node := Control.new()
	node.size_flags_horizontal = Control.SIZE_EXPAND_FILL
	return node


static func clear(container: Node) -> void:
	for child in container.get_children():
		container.remove_child(child)
		child.queue_free()


static func card_chip(card: Dictionary, prefix: String = "") -> PanelContainer:
	var bonus_key := str(card.get("bonus_color", "red"))
	return pill(
		"%s%s %d分" % [prefix, str(card.get("name_zh", "?")), int(card.get("points", 0))],
		Styles.TOKEN_COLORS.get(bonus_key, Styles.COLOR_ACCENT)
	)
