# 璀璨宝石：宝可梦 — Godot 回放前端

独立 Godot 4.6 工程，只读回放可视化：读取 Rust CLI 生成的 replay JSON（默认仓库 `tmp/*.json`），逐动作重绘界面。不建游戏、不联网、不改规则。

## 结构

```text
game/
├── project.godot              # 1440×900，canvas_items 拉伸，gl_compatibility
├── scenes/main.tscn           # 顶栏 / 时间轴 / 横幅 / 场上牌 / 玩家 / 供应区 / 事件
├── scripts/main.gd            # 回放载入、逐帧渲染、播放控制、冒烟
├── scripts/game/board_view.gd # 场上 5 行牌的自绘视图 + 悬停 tooltip
├── scripts/ui/styles.gd       # 色板与 StyleBoxFlat 工厂（与 HTML 回放同色）
├── theme/game_theme.tres      # SystemFont：中文走系统字体兜底
└── icon.svg
```

## 运行

在仓库根目录（本工作副本的 Godot 4.6 可执行文件在仓库上一级）：

```bash
../Godot_v4.6.1-stable_win64.exe --path game
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
- 视图按执行动作的玩家生成：自己的 `reserved` 是数组，对手为 `null` 且只有 `reserved_count`。界面不额外泄露牌面。
- 场上牌、费用、减免、进化需求全部来自卡牌对象；Godot 不重新计算规则。

## 操作

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

界面尺寸：窗口 1440×900，最小 1100×700；`canvas_items` + `aspect=expand`，窗口缩放时整体等比缩放并扩展布局。场上牌行高不足 72px 时自动切紧凑模式（隐藏进化行）。玩家面板内只显示最高分 3 张捕捉牌与 2 张保留牌的摘要，完整列表在 tooltip 中，避免长局把棋盘挤扁。

## 冒烟

在仓库根目录：

```bash
SPLENDOR_SMOKE=1 SPLENDOR_REPLAY=tmp/bot-42-2p.json \
  ../Godot_v4.6.1-stable_win64_console.exe --headless --path game
```

退出码 0 通过。检查项：回放载入、5 行牌槽位、玩家面板数量、首末帧按钮状态、播放 tick、终局横幅、控件尺寸、SystemFont 中文字形。

无头模式只验证逻辑与 Control 排版，不验证绘制像素；需要看画面时去掉 `--headless`。
