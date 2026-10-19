extends Node
## 跨场景共享的启动参数与界面设置（autoload：AppState）。
##
## 只保存界面状态：开局参数、对局是否进行中、待打开的回放路径、动画速度。
## 不缓存对局视图、不复制规则；对局数据始终来自本地引擎。
## 设置写入 `user://settings.cfg`，仓库内不产生额外文件。

const SETTINGS_PATH := "user://settings.cfg"
const AI_SPEED_LABELS := ["慢", "标准", "快"]
const AI_SPEED_DELAYS := [0.70, 0.45, 0.20]
const REPLAY_SPEED_LABELS := ["慢", "中", "快", "极快"]

## 开局设置：bots 是 AI 数量，bot 是策略 id，seed 是随机种子。
var battle_config := {"bots": 2, "bot": "minimax", "seed": 42}
## 菜单「继续对局」是否可用；对局结束或未开局时置 false。
var battle_in_progress := false
## 进入对战场景时是否先用 GET /api/game/state 恢复现有对局。
var battle_resume := false
## 菜单选中的回放文件；回放场景读取后清空。
var replay_path := ""
## 0 慢 / 1 标准 / 2 快。
var ai_speed_index := 1
## 对应回放场景的 慢 / 中 / 快 / 极快。
var replay_speed_index := 1
## 由 Godot 自动启动的 serve 进程；跨场景保留，应用退出时结束。
var server_pid := -1


func _ready() -> void:
	_load_settings()


func _exit_tree() -> void:
	if server_pid > 0:
		OS.kill(server_pid)
		server_pid = -1


func ai_step_delay() -> float:
	return float(AI_SPEED_DELAYS[clampi(ai_speed_index, 0, AI_SPEED_DELAYS.size() - 1)])


func ai_speed_label() -> String:
	return str(AI_SPEED_LABELS[clampi(ai_speed_index, 0, AI_SPEED_LABELS.size() - 1)])


func replay_speed_label() -> String:
	return str(REPLAY_SPEED_LABELS[clampi(replay_speed_index, 0, REPLAY_SPEED_LABELS.size() - 1)])


func save_settings() -> void:
	var config := ConfigFile.new()
	config.set_value("battle", "ai_speed", ai_speed_index)
	config.set_value("replay", "speed", replay_speed_index)
	config.save(SETTINGS_PATH)


func _load_settings() -> void:
	var config := ConfigFile.new()
	if config.load(SETTINGS_PATH) != OK:
		return
	ai_speed_index = clampi(
		int(config.get_value("battle", "ai_speed", ai_speed_index)),
		0,
		AI_SPEED_DELAYS.size() - 1
	)
	replay_speed_index = clampi(
		int(config.get_value("replay", "speed", replay_speed_index)),
		0,
		REPLAY_SPEED_LABELS.size() - 1
	)
