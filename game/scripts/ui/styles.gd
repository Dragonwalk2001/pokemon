extends RefCounted
## 共享配色与样式工厂：Godot 回放界面唯一取色点，色板与 HTML 回放保持一致。
##
## 调用方用 `const Styles := preload("res://scripts/ui/styles.gd")`，
## 之后直接访问常量与静态函数，不创建实例。

const COLOR_BG := Color("10131a")
const COLOR_PANEL := Color("171c26")
const COLOR_PANEL_2 := Color("1e2532")
const COLOR_LINE := Color("2b3445")
const COLOR_TEXT := Color("e8edf7")
const COLOR_MUTED := Color("93a0b5")
const COLOR_ACCENT := Color("7c9cff")
const COLOR_GOLD := Color("f0c15a")
const COLOR_RED := Color("e5534b")
const COLOR_BLUE := Color("4b8ee5")
const COLOR_YELLOW := Color("d7b545")
const COLOR_PINK := Color("e56bb5")
const COLOR_BLACK := Color("8b93a7")
const COLOR_MASTER := Color("a78bfa")

const COLOR_KEYS := ["red", "blue", "yellow", "pink", "black", "master"]
const COLOR_LABELS := {
	"red": "红",
	"blue": "蓝",
	"yellow": "黄",
	"pink": "粉",
	"black": "黑",
	"master": "大师",
}
const TOKEN_COLORS := {
	"red": COLOR_RED,
	"blue": COLOR_BLUE,
	"yellow": COLOR_YELLOW,
	"pink": COLOR_PINK,
	"black": COLOR_BLACK,
	"master": COLOR_MASTER,
}


## 通用面板：深色底 + 1px 描边 + 圆角，内容留白 12 / 10。
static func panel_box(
	bg: Color = COLOR_PANEL,
	border: Color = COLOR_LINE,
	radius: int = 12,
	border_width: int = 1
) -> StyleBoxFlat:
	var box := StyleBoxFlat.new()
	box.bg_color = bg
	box.border_color = border
	box.set_border_width_all(border_width)
	box.set_corner_radius_all(radius)
	box.content_margin_left = 12
	box.content_margin_right = 12
	box.content_margin_top = 10
	box.content_margin_bottom = 10
	return box


## 紧凑格子：供应区 / 徽章等小面板。
static func tile_box(
	bg: Color = COLOR_PANEL_2,
	border: Color = COLOR_LINE,
	radius: int = 8,
	margin_x: int = 8,
	margin_y: int = 5
) -> StyleBoxFlat:
	var box := StyleBoxFlat.new()
	box.bg_color = bg
	box.border_color = border
	box.set_border_width_all(1)
	box.set_corner_radius_all(radius)
	box.content_margin_left = margin_x
	box.content_margin_right = margin_x
	box.content_margin_top = margin_y
	box.content_margin_bottom = margin_y
	return box


## 圆角药丸：球 / 减免数量。
static func pill_box(bg: Color = Color("232b3a"), border: Color = Color(0, 0, 0, 0)) -> StyleBoxFlat:
	var box := StyleBoxFlat.new()
	box.bg_color = bg
	box.border_color = border
	box.set_border_width_all(0)
	box.set_corner_radius_all(8)
	box.content_margin_left = 6
	box.content_margin_right = 6
	box.content_margin_top = 1
	box.content_margin_bottom = 1
	return box


## 小标签：统计与卡牌名。
static func chip_box(bg: Color = Color("232b3a")) -> StyleBoxFlat:
	var box := StyleBoxFlat.new()
	box.bg_color = bg
	box.set_border_width_all(0)
	box.set_corner_radius_all(5)
	box.content_margin_left = 6
	box.content_margin_right = 6
	box.content_margin_top = 1
	box.content_margin_bottom = 1
	return box


## 纯描边：场上牌悬停高亮。
static func border_box(border: Color, width: int = 2, radius: int = 10) -> StyleBoxFlat:
	var box := StyleBoxFlat.new()
	box.bg_color = Color(0, 0, 0, 0)
	box.border_color = border
	box.set_border_width_all(width)
	box.set_corner_radius_all(radius)
	return box


## 颜色圆点（8×8 的 Panel 用）。
static func dot_box(color: Color) -> StyleBoxFlat:
	var box := StyleBoxFlat.new()
	box.bg_color = color
	box.border_color = color
	box.set_border_width_all(0)
	box.set_corner_radius_all(4)
	return box


## 编号精灵球：彩色正圆 + 浅色描边，数字写在球内。
static func ball_box(color: Color, diameter: int = 20) -> StyleBoxFlat:
	var box := StyleBoxFlat.new()
	box.bg_color = color
	box.border_color = Color(1, 1, 1, 0.35)
	box.set_border_width_all(1)
	box.set_corner_radius_all(int(diameter * 0.5))
	box.content_margin_left = 0
	box.content_margin_right = 0
	box.content_margin_top = 0
	box.content_margin_bottom = 0
	return box


## 球内数字：浅色球用深字，深色球用白字（与 HTML 卡面 .ball 一致）。
static func ball_ink(key: String) -> Color:
	match key:
		"yellow":
			return Color("2a2408")
		"pink":
			return Color("2c0f21")
		"black":
			return Color("14161c")
		"master":
			return Color("1a1230")
		_:
			return Color.WHITE


## 分数方块：卡牌胜利分用方块，避免和精灵球混淆。
static func score_box(bg: Color) -> StyleBoxFlat:
	var box := StyleBoxFlat.new()
	box.bg_color = bg
	box.border_color = Color(0, 0, 0, 0)
	box.set_border_width_all(0)
	box.set_corner_radius_all(3)
	box.content_margin_left = 0
	box.content_margin_right = 0
	box.content_margin_top = 0
	box.content_margin_bottom = 0
	return box


## 终局 / 僵局横幅。
static func banner_box(accent: Color) -> StyleBoxFlat:
	var box := StyleBoxFlat.new()
	box.bg_color = Color("2a2415") if accent == COLOR_GOLD else Color("2b1f16")
	box.border_color = accent
	box.set_border_width_all(1)
	box.set_corner_radius_all(10)
	box.content_margin_left = 16
	box.content_margin_right = 16
	box.content_margin_top = 8
	box.content_margin_bottom = 8
	return box


## 统一按钮外观；primary 用于自动播放。
static func style_button(button: Button, primary: bool = false) -> void:
	var normal_bg := Color("2d4a9e") if primary else COLOR_PANEL_2
	var normal_border := Color("3f63c8") if primary else COLOR_LINE
	var hover_bg := Color("3658b8") if primary else Color("263042")
	var hover_border := COLOR_ACCENT
	var pressed_bg := Color("24407f") if primary else Color("1a2130")

	button.add_theme_stylebox_override("normal", button_box(normal_bg, normal_border))
	button.add_theme_stylebox_override("hover", button_box(hover_bg, hover_border))
	button.add_theme_stylebox_override("pressed", button_box(pressed_bg, hover_border))
	button.add_theme_stylebox_override("disabled", button_box(COLOR_PANEL, COLOR_LINE))
	button.add_theme_stylebox_override("focus", border_box(COLOR_ACCENT, 1, 8))
	button.add_theme_color_override("font_color", COLOR_TEXT)
	button.add_theme_color_override("font_hover_color", Color.WHITE)
	button.add_theme_color_override("font_pressed_color", Color.WHITE)
	button.add_theme_color_override("font_disabled_color", Color(0.45, 0.50, 0.60))
	button.add_theme_font_size_override("font_size", 14)


static func button_box(bg: Color, border: Color) -> StyleBoxFlat:
	var box := StyleBoxFlat.new()
	box.bg_color = bg
	box.border_color = border
	box.set_border_width_all(1)
	box.set_corner_radius_all(8)
	box.content_margin_left = 12
	box.content_margin_right = 12
	box.content_margin_top = 6
	box.content_margin_bottom = 6
	return box
