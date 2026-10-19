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


## 编号精灵球：与卡面费用一致的彩色圆球，数字写在球内；dim 表示数量为 0 的占位。
static func ball(
	key: String, count: int, diameter: int = 20, dim: bool = false
) -> PanelContainer:
	var node := panel(
		Styles.ball_box(Styles.TOKEN_COLORS.get(key, Styles.COLOR_ACCENT), diameter)
	)
	node.custom_minimum_size = Vector2(diameter, diameter)
	node.size_flags_vertical = Control.SIZE_SHRINK_CENTER
	if dim:
		node.modulate = Color(1, 1, 1, 0.4)
	var value := label(str(count), maxi(10, int(diameter * 0.55)), Styles.ball_ink(key))
	value.horizontal_alignment = HORIZONTAL_ALIGNMENT_CENTER
	value.vertical_alignment = VERTICAL_ALIGNMENT_CENTER
	node.add_child(value)
	node.tooltip_text = "%s球 %d" % [Styles.COLOR_LABELS.get(key, key), count]
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


static func card_chip(
	card: Dictionary, prefix: String = "", tooltip: String = ""
) -> PanelContainer:
	var bonus_key := str(card.get("bonus_color", "red"))
	var node := pill(
		"%s%s %d分" % [prefix, str(card.get("name_zh", "?")), int(card.get("points", 0))],
		Styles.TOKEN_COLORS.get(bonus_key, Styles.COLOR_ACCENT)
	)
	if not tooltip.is_empty():
		node.tooltip_text = tooltip
	return node


## 进化链文本：`迷你龙 → 【哈克龙】 → 快龙`；mark_current 关闭时去掉【】。
## 旧回放缺少 `evolution_line` 时退回卡牌自身的 `evolves_to_zh`。
static func chain_text(card: Dictionary, mark_current: bool = true) -> String:
	var current := str(card.get("name_zh", "?"))
	var names: Array[String] = []
	var steps: Variant = card.get("evolution_line", [])
	if steps is Array:
		for step_v in steps:
			if step_v is Dictionary:
				names.append(str((step_v as Dictionary).get("name_zh", "?")))
	if names.size() > 1:
		var parts: Array[String] = []
		for name in names:
			parts.append("【%s】" % name if mark_current and name == current else name)
		return " → ".join(parts)
	var next := str(card.get("evolves_to_zh", ""))
	return "→ %s" % next if not next.is_empty() else ""


## 下一阶段形态名；已是最终形态或不在进化链里时返回空串。
static func next_evolution_name(card: Dictionary) -> String:
	var current := str(card.get("name_zh", "?"))
	var steps: Variant = card.get("evolution_line", [])
	if steps is Array:
		var found := false
		for step_v in steps:
			if not (step_v is Dictionary):
				continue
			var name := str((step_v as Dictionary).get("name_zh", "?"))
			if found:
				return name
			if name == current:
				found = true
	return str(card.get("evolves_to_zh", ""))


## 进化门槛文本，例如 `黄 4`；没有门槛时返回空串。
static func evolution_requirement_text(card: Dictionary) -> String:
	var raw: Variant = card.get("evolution_requirement")
	if not (raw is Dictionary):
		return ""
	var requirement: Dictionary = raw
	var count := int(requirement.get("count", 0))
	if count <= 0:
		return ""
	var color := str(requirement.get("color", ""))
	return "%s %d" % [Styles.COLOR_LABELS.get(color, color), count]


## 费用文本，例如 `蓝4 黄1`；免费返回 `免费`。
static func cost_text(card: Dictionary) -> String:
	var parts: Array[String] = []
	var raw: Variant = card.get("cost", {})
	if raw is Dictionary:
		var cost: Dictionary = raw
		for key in Styles.COLOR_KEYS:
			var count := int(cost.get(key, 0))
			if count > 0:
				parts.append("%s%d" % [Styles.COLOR_LABELS.get(key, key), count])
	return " ".join(parts) if not parts.is_empty() else "免费"


## 减免文本，例如 `黑+1`。
static func bonus_text(card: Dictionary) -> String:
	var key := str(card.get("bonus_color", "red"))
	return "%s+%d" % [Styles.COLOR_LABELS.get(key, key), int(card.get("bonus_count", 0))]


## 面板 tooltip 里的一张牌：名称 / 分数 / 减免 / 费用 / 进化门槛与进度 / 进化链。
static func detail_lines(
	card: Dictionary, bonuses: Dictionary, prefix: String = "· "
) -> Array[String]:
	var lines: Array[String] = []
	lines.append("%s%s %d分 ｜ 减免 %s ｜ 费 %s" % [
		prefix,
		str(card.get("name_zh", "?")),
		int(card.get("points", 0)),
		bonus_text(card),
		cost_text(card),
	])
	var next := next_evolution_name(card)
	if not next.is_empty():
		var progress := ""
		var requirement: Variant = card.get("evolution_requirement")
		if requirement is Dictionary:
			var color := str((requirement as Dictionary).get("color", ""))
			var needed := int((requirement as Dictionary).get("count", 0))
			progress = "%s %d/%d" % [
				Styles.COLOR_LABELS.get(color, color),
				int(bonuses.get(color, 0)),
				needed,
			]
		lines.append("  进化 → %s（需 %s）｜ 减免进度 %s" % [
			next,
			evolution_requirement_text(card),
			progress,
		])
	var chain := chain_text(card)
	if not chain.is_empty():
		lines.append("  进化链：" + chain)
	return lines

