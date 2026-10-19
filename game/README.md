# 璀璨宝石：宝可梦 — Godot 前端

独立 Godot 4.6 工程，三个场景：

- **标题菜单**（`scenes/menu.tscn`，主场景）：继续对局 / 新对局 / 观看回放 / 设置 / 退出。
- **只读回放**（`scenes/replay.tscn`）：读取 Rust CLI 生成的 replay JSON，逐动作重绘界面。
- **和 AI 对战**（`scenes/battle.tscn`）：通过 `splendor-pokemon serve` 的本地 JSON HTTP 接口实时对局。

三种模式都不复制规则：回放直接渲染 `GameView`；对战只发送动作、渲染引擎返回的 `GameView`。
没有联网、远程 PvP 或服务器房间。

## 结构

```text
game/
├── project.godot              # 1440×900，canvas_items，主场景 menu，autoload AppState
├── scenes/menu.tscn           # 标题菜单 + 新对局设置 / 设置 / 回放选择
├── scenes/replay.tscn         # 回放：顶栏 / 时间轴 / 横幅 / 场上牌 / 玩家 / 供应区 / 事件
├── scenes/battle.tscn         # 对战：状态顶栏 / 棋盘 / 对手 / 球架 / 底栏行动区 / 模态
├── scripts/menu.gd            # 菜单导航、开局参数、设置、回放文件选择
├── scripts/replay.gd          # 回放载入、逐帧渲染、播放控制、冒烟
├── scripts/battle.gd          # 本地引擎连接、选择状态机、动作提交、逐步播放、冒烟
├── scripts/app_state.gd       # 跨场景参数与设置（user://settings.cfg）
├── scripts/game/board_view.gd # 场上 5 行牌 + 选中抬起 + 可操作集合 + 键盘浏览
├── scripts/game/token_strip.gd# 精灵球堆：左键 +1 / 右键 −1
├── scripts/ui/styles.gd       # 色板与 StyleBoxFlat 工厂（与 HTML 回放同色）
├── scripts/ui/widgets.gd      # 面板 / 药丸 / 徽章等小控件
├── theme/game_theme.tres      # SystemFont：中文走系统字体兜底
└── icon.svg
```

## 运行

在仓库根目录（本工作副本的 Godot 4.6 可执行文件在仓库上一级）：

```bash
# 默认进入标题菜单
../Godot_v4.6.1-stable_win64.exe --path game

# 直接打开回放场景
../Godot_v4.6.1-stable_win64.exe --path game res://scenes/replay.tscn

# 直接打开对战场景（跳过菜单，使用默认设置）
../Godot_v4.6.1-stable_win64.exe --path game res://scenes/battle.tscn
```

`godot` 已在 `PATH` 时用 `godot --path game`。

回放路径解析顺序：

1. 命令行 `-- <path>`：`... --path game res://scenes/replay.tscn -- tmp/bot-42-2p.json`
2. 环境变量 `SPLENDOR_REPLAY`
3. 菜单「观看回放」选择的文件（`AppState.replay_path`）
4. 仓库 `tmp/` 下最新的 `*.json`
5. 空状态：点「打开回放…」选择文件

相对路径按仓库根解析（`res://` 的上一级）。

## 交互模型（对战）

核心是「点对象 → 就地选项或行动篮 → 底栏确认」：选中不直接提交，确认按钮只有在动作合法时才启用。

- **场上牌**：左键和右键都是选中。选中牌抬起并描金边，卡片正下方弹出「捕捉 / 保留」；不可用的按钮置灰，并写出缺球原因（如「蓝缺 1」）或「保留牌已满」。既不能捕捉也不能保留的牌整体降亮度。
- **精灵球**：左键 +1、右键 −1，放入底栏行动篮（球堆上显示 `+N`）。同色最多 2 个、总数最多 3 个；同色第 2 个要求供应区该色至少 4 个；已选 2 同色后其它颜色自动禁用。底栏确认按钮直接写出「拿取 红×2 蓝×1」。
- **保留牌**：点击直接捕捉；不合法置灰。
- **弃球**：底栏「弃球…」打开球架。持有区左键 +1 / 右键 −1，弃球区点击取回；只有正好把球数降到 10 时确认按钮可用。
- **进化**：底栏「进化…」列出引擎给出的合法进化；「结束回合」在进化阶段常驻。
- **键盘**：`数字 1–5` 快速选球；`Tab` 在棋盘与按钮间移动焦点；棋盘聚焦后方向键移动、`Enter` / `空格` 选中；`Esc` 逐级取消，再按打开暂停菜单。
- **回合锁**：非我方回合棋盘不可点，顶栏显示「等待对手」；AI 动作按返回的 `steps` 逐个播放，播完才恢复操作。
- **暂停菜单**：继续游戏 / 重新开始（二次确认）/ 返回标题（二次确认）。对局保存在本地引擎里，返回标题后可用「继续对局」恢复。
- **终局**：居中结算面板显示胜者与分数；回放自动写入 `tmp/`，按钮为再来一局 / 保存回放 / 返回标题。

界面尺寸：窗口 1440×900，最小 1180×760；`canvas_items` + `aspect=expand`。
顶栏只放阶段、当前行动者与种子；开局参数在标题菜单的「新对局」里。

## 数据与私密信息

- 每帧使用 `frames[i].view`，字段与后端 `GameView` 一致：`board`、`supply`、`players`、`current_player`、`phase`、`winners`、`legal_actions`。
- `frames[0]` 是开局视图，之后每帧保存 `actor + Action + GameView + 本步事件文本`。
- 视图按执行动作的玩家生成：`reserved` 只包含当前查看者可见的保留牌。自己能看到全部；对手从场上保留的牌牌面公开，从牌堆顶保留的牌只通过 `hidden_reserved_count` 暴露数量。界面不额外泄露牌面。
- 场上牌、费用、减免、进化需求全部来自卡牌对象；Godot 不重新计算规则。
- 卡面中央大字直接取 `name_zh`，按名字长度自适应字号；完整信息在 tooltip 中。
- 对战模式的每个视图来自 `Game::view_for(0)`：`legal_actions` 只在轮到人类时填充；界面用 `legal_actions` 生成可操作集合与按钮禁用状态。弃球阶段引擎不枚举组合，界面按球数与上限自行组织 `discard` 动作，最终仍由引擎校验。
- 可选动作、行动篮、缺球提示都是界面提示，不是规则来源；提交后仍以引擎返回的 `steps` / `state` 为准。

## 回放操作

| 操作 | 说明 |
|---|---|
| `←` / `→` | 上一帧 / 下一帧 |
| `空格` / `P` | 自动播放 / 暂停 |
| `Home` / `End` | 首帧 / 末帧 |
| `Esc` | 返回标题菜单 |
| 进度条 | 任意跳转 |
| 速度 | 慢 / 中 / 快 / 极快（默认值来自设置） |
| 打开回放… | 文件对话框，默认打开仓库 `tmp/` |
| 重新载入 | 重读当前 JSON |
| 悬停场上牌 | 显示名字、分数、减免、费用、进化需求的 tooltip |
| 悬停玩家面板 | 显示该玩家的完整捕捉 / 保留列表 |

场上牌行高不足 72px 时自动切紧凑模式（隐藏进化行）。玩家面板内只显示最高分 3 张捕捉牌与 2 张保留牌的摘要，完整列表在 tooltip 中。
横幅在终局显示胜者与分数；规则僵局显示无胜者说明。

## 和 AI 对战

`scenes/battle.tscn` 连接 `splendor-pokemon serve`（默认 `http://127.0.0.1:43110`），你是玩家 0，对手为 1–3 个 bot。
规则、AI、回放记录和私有信息过滤都在 Rust 后端完成。

启动：

| 方式 | 操作 |
|---|---|
| 菜单 | 打开主场景，点「新对局」设置对手数量（1 / 2 / 3） / 强度 / 种子后开始 |
| 直接 | `--path game res://scenes/battle.tscn`，使用 `AppState` 默认设置（2 个 AI、minimax、种子 42） |
| 服务 | 在 `backend/` 运行 `cargo run -p splendor-pokemon-cli -- serve`；无服务时界面会自动启动可执行文件，缺失时退回 `cargo run`（首次编译较慢） |

- `SPLENDOR_SERVER_URL`：指定服务地址；设置后不再自动启动。
- `SPLENDOR_CLI`：指定 `splendor-pokemon` 可执行文件，用于自动启动。
- 「继续对局」通过 `GET /api/game/state` 恢复当前对局；对局结束、未开局或重启应用后该按钮禁用。
- 对局结束后自动把 `tmp/pve-<seed>-<n>p.json` 与 `.html` 写好，也可以在结算面板点「保存回放」重写。
- 顶栏不提供开局参数；修改对手数量 / 强度 / 种子需要回到标题菜单重新开局。

## 设置

标题菜单「设置」保存两项，写入 `user://settings.cfg`：

| 设置 | 作用 |
|---|---|
| AI 行动速度 | 对战逐步播放 AI 动作的间隔（慢 / 标准 / 快） |
| 回放默认速度 | 回放场景启动时的播放速度（慢 / 中 / 快 / 极快） |

设置只影响动画节奏，不影响规则、AI 决策或回放内容。

## 冒烟

菜单（在仓库根目录）：

```bash
SPLENDOR_MENU_SMOKE=1 \
  ../Godot_v4.6.1-stable_win64_console.exe --headless --path game
```

回放：

```bash
SPLENDOR_SMOKE=1 SPLENDOR_REPLAY=tmp/bot-42-2p.json \
  ../Godot_v4.6.1-stable_win64_console.exe --headless --path game res://scenes/replay.tscn
```

退出码 0 通过。检查项：回放载入、5 行牌槽位、玩家面板数量、公开保留牌渲染、首末帧按钮状态、播放 tick、终局横幅、控件尺寸、SystemFont 中文字形。

对战（先在 `backend/` 启动服务，再在仓库根目录运行）：

```bash
# 终端 1
cargo run -p splendor-pokemon-cli -- serve --port 43111 --idle-timeout 60

# 终端 2
SPLENDOR_BATTLE_SMOKE=1 SPLENDOR_SERVER_URL=http://127.0.0.1:43111 \
  ../Godot_v4.6.1-stable_win64_console.exe --headless --path game res://scenes/battle.tscn
```

对战冒烟真实调用服务：开局、渲染 2 个对手 / 5 行棋盘 / 6 格球架 / 底栏、校验行动篮拼出的拿球动作、检查贴卡动作条、提交拿球与结束回合、打开进化 / 弃球 / 暂停模态，确认 AI 回合后回到人类。

无头模式只验证逻辑与 Control 排版，不验证绘制像素；需要看画面时去掉 `--headless`。

