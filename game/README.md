# 璀璨宝石：宝可梦 — Godot 前端

独立 Godot 4.6 工程，包含两个场景：

- **只读回放**（`scenes/main.tscn`）：读取 Rust CLI 生成的 replay JSON，逐动作重绘界面。
- **和 AI 对战**（`scenes/battle.tscn`）：通过 `splendor-pokemon serve` 的本地 JSON HTTP 接口实时对局。

两种模式都不复制规则：回放直接渲染 `GameView`；对战只发送动作、渲染引擎返回的 `GameView`。没有联网、远程 PvP 或服务器房间。

## 结构

```text
game/
├── project.godot              # 1440×900，canvas_items 拉伸，gl_compatibility
├── scenes/main.tscn           # 回放：顶栏 / 时间轴 / 横幅 / 场上牌 / 玩家 / 供应区 / 事件
├── scenes/battle.tscn         # 对战：顶栏设置 / 棋盘 / 玩家 / 保留牌 / 供应区 / 行动与记录
├── scripts/main.gd            # 回放载入、逐帧渲染、播放控制、冒烟
├── scripts/battle.gd          # 本地引擎连接、动作提交、逐步播放、终局保存、冒烟
├── scripts/game/board_view.gd # 场上 5 行牌 + 悬停 tooltip + 可选点击命中
├── scripts/ui/styles.gd       # 色板与 StyleBoxFlat 工厂（与 HTML 回放同色）
├── scripts/ui/widgets.gd      # 对战界面的面板 / 药丸 / 徽章等小控件
├── theme/game_theme.tres      # SystemFont：中文走系统字体兜底
└── icon.svg
```

## 运行

在仓库根目录（本工作副本的 Godot 4.6 可执行文件在仓库上一级）：

```bash
# 默认打开回放场景
../Godot_v4.6.1-stable_win64.exe --path game

# 直接打开对战场景
../Godot_v4.6.1-stable_win64.exe --path game res://scenes/battle.tscn
```

`godot` 已在 `PATH` 时用 `godot --path game`。

回放路径解析顺序：

1. 命令行 `-- <path>`：`... --path game -- tmp/bot-42-2p.json`
2. 环境变量 `SPLENDOR_REPLAY`
3. 仓库 `tmp/` 下最新的 `*.json`
4. 空状态：点「打开回放…」选择文件

相对路径按仓库根解析（`res://` 的上一级）。

## 数据与私密信息

- 每帧使用 `frames[i].view`，字段与后端 `GameView` 一致：`board`、`supply`、`players`、`current_player`、`phase`、`winners`、`legal_actions`。
- `frames[0]` 是开局视图，之后每帧保存 `actor + Action + GameView + 本步事件文本`。
- 视图按执行动作的玩家生成：`reserved` 只包含当前查看者可见的保留牌。自己能看到全部；对手从场上保留的牌牌面公开，从牌堆顶保留的牌只通过 `hidden_reserved_count` 暴露数量。界面不额外泄露牌面。
- 场上牌、费用、减免、进化需求全部来自卡牌对象；Godot 不重新计算规则。
- 卡面中央大字直接取 `name_zh`，按名字长度自适应字号；卡面不重复显示小名字，完整信息仍在 tooltip 中。
- 对战模式的每个视图来自 `Game::view_for(0)`：`legal_actions` 只在轮到人类时填充；弃球阶段引擎不枚举弃球组合，界面按球数与上限自行组织 `discard` 动作，最终仍由引擎校验。

## 回放操作

| 操作 | 说明 |
|---|---|
| `←` / `→` | 上一帧 / 下一帧 |
| `空格` / `P` | 自动播放 / 暂停 |
| `Home` / `End` | 首帧 / 末帧 |
| 进度条 | 任意跳转 |
| 速度 | 慢 / 中 / 快 / 极快（1.8 / 1.0 / 0.55 / 0.25 秒） |
| 打开回放… | 文件对话框，默认打开仓库 `tmp/` |
| 重新载入 | 重读当前 JSON |
| 悬停场上牌 | 显示名字、分数、减免、费用、进化需求的 tooltip |
| 悬停玩家面板 | 显示该玩家的完整捕捉 / 保留列表 |

界面尺寸：窗口 1440×900，最小 1100×700；`canvas_items` + `aspect=expand`，窗口缩放时整体等比缩放并扩展布局。卡面底色取减免颜色，中央是完整名字大字（2–4 字，按名字长度自适应字号），右上角分数，左下角捕捉费用（彩球 + 数字），右下角进化需求；完整费用与减免数值见 tooltip。场上牌行高不足 72px 时自动切紧凑模式（隐藏进化行）。玩家面板内只显示最高分 3 张捕捉牌与 2 张保留牌的摘要，完整列表在 tooltip 中，避免长局把棋盘挤扁。

## 和 AI 对战

`scenes/battle.tscn` 连接 `splendor-pokemon serve`（默认 `http://127.0.0.1:43110`），你是玩家 0，对手为 2–3 个 bot。规则、AI、回放记录和私有信息过滤都在 Rust 后端完成。

启动：

| 方式 | 操作 |
|---|---|
| 自动 | 打开主场景，点右上角「和 AI 对战」；端口无服务时自动启动后端可执行文件，缺失时退回 `cargo run`（首次编译较慢） |
| 手动 | 在 `backend/` 运行 `cargo run -p splendor-pokemon-cli -- serve`，再用 `--path game res://scenes/battle.tscn` 打开场景 |

- `SPLENDOR_SERVER_URL`：指定服务地址；设置后不再自动启动。
- `SPLENDOR_CLI`：指定 `splendor-pokemon` 可执行文件，用于自动启动。
- 顶栏可选择 AI 数量（2 / 3）、AI 策略（极小化极大 / 启发式 / 随机）与种子；「新对局」重开，「回放」返回回放场景。
- 主要行动：左键选中场上牌后点「捕捉 / 保留」，或右键直接保留；供应区点击颜色循环 `0 → 1 → 2 → 0`，再点「拿取所选」。拿 3 色或拿 2 同色由选择自动判定，并先在本地与 `legal_actions` 核对。
- 我的保留牌可直接点击捕捉。
- 弃球阶段按颜色（含大师球）循环选择弃球数量，达到「球数 − 10」后确认。
- 进化阶段列出所有合法进化动作，「结束回合」跳过进化。
- 人类动作后服务端连续推进 bot 回合；界面按返回的 `steps` 逐个更新棋盘与事件，播完才恢复操作。
- 终局 / 僵局显示横幅；完成后自动把 `tmp/pve-<seed>-<n>p.json` 与 `.html` 写好，也可以点「保存回放」重写。

## 冒烟

回放（在仓库根目录）：

```bash
SPLENDOR_SMOKE=1 SPLENDOR_REPLAY=tmp/bot-42-2p.json \
  ../Godot_v4.6.1-stable_win64_console.exe --headless --path game
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

对战冒烟真实调用服务：开局、渲染 3 个玩家 / 5 行棋盘 / 供应区，校验界面构造的拿球与保留动作，提交拿球与结束回合，确认 AI 回合后回到人类并生成进化阶段按钮。

无头模式只验证逻辑与 Control 排版，不验证绘制像素；需要看画面时去掉 `--headless`。
