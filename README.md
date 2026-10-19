# 璀璨宝石：宝可梦（离线原型）

**规则与 bot 在 Rust 后端；Godot 提供两种可视化：只读回放，以及通过本地 JSON HTTP 服务直接和 AI 对战。** 没有联网或远程 PvP；Godot 不复制规则，所有动作都由后端 `Game::apply` 判定。

三种 CLI 用法：

| 模式 | 操作方式 | 是否记录回放 |
|---|---|---|
| 本地热座 | 终端里多人轮流输入 | 否 |
| PvE（1 人 + 2–3 bot） | 终端交互，bot 自动行动 | 是，终局后写文件 |
| bot-vs-bot | 终端自动跑完 | 是，默认写文件 |

Godot 里另有：

| 模式 | 操作方式 | 是否记录回放 |
|---|---|---|
| 标题菜单 | 启动后的主场景：继续 / 新对局 / 观看回放 / 设置 | 不适用 |
| 回放 | 读取 `tmp/*.json` 逐帧查看 | 不适用 |
| 和 AI 对战 | 图形界面操作，1 人 + 2–3 bot；后端 `serve` 执行规则与 AI | 是，终局自动写 `tmp/pve-<seed>-<n>p.*` |

## 环境

需要 Rust 的 `cargo`。以下命令都在 `backend/` 下执行：

```bash
cd backend
```

如果 `cargo` 不在 PATH（本工作副本在项目根 `.rust/` 带了隔离工具链），在 `backend/` 下执行（Git Bash）：

```bash
export CARGO_HOME="$PWD/../.rust/cargo"
export RUSTUP_HOME="$PWD/../.rust/rustup"
export PATH="$CARGO_HOME/bin:$PATH"

cargo --version   # cargo 1.99.1
```

PowerShell 对应写法：

```powershell
$env:CARGO_HOME = "$PWD/../.rust/cargo"
$env:RUSTUP_HOME = "$PWD/../.rust/rustup"
$env:PATH = "$env:CARGO_HOME/bin;$env:PATH"

cargo --version
```

要点：

- `CARGO_HOME` 只决定 registry 与缓存位置，**不会**把 `cargo` 加进 `PATH`，必须单独把 `$CARGO_HOME/bin` 加进 `PATH`。
- `RUSTUP_HOME` 必需：`.rust/cargo/bin/cargo.exe` 是 rustup 代理，缺它会在用户目录 `~/.rustup` 找工具链并报 `no default is configured`。
- 以上 `export` / `$env:` 只影响当前终端会话。不要写入 `~/.bashrc`、用户级环境变量或 `setx`；`.rust/` 不随仓库分发。
- 不想改 `PATH` 时，用显式路径调用（同样在 `backend/` 下）：

  ```bash
  CARGO_HOME="$PWD/../.rust/cargo" RUSTUP_HOME="$PWD/../.rust/rustup" \
    ../.rust/cargo/bin/cargo.exe --version
  ```

## 1. 启动游戏

### 本地热座（同一终端轮流操作）

```bash
cargo run -p splendor-pokemon-cli -- --players 3 --seed 42
```

指定名字，人数由名字数量决定：

```bash
cargo run -p splendor-pokemon-cli -- --names 小明,小红,小刚
```

### PvE：1 人 vs 2–3 个 bot

```bash
cargo run -p splendor-pokemon-cli -- pve --bots 2 --seed 42
```

- 你是玩家 0，其余是 `Bot1`、`Bot2`；`--bots 3` 则为 1 人 vs 3 bot。
- bot 回合自动执行，轮到你时出现 `你>` 提示符。
- `--bot random` 可把 bot 换成随机策略（默认 `minimax` 搜索策略）。
- `--bot heuristic` 使用旧的启发式策略。
- `--bot minimax` 使用极小化极大搜索策略（捕捉阶段展开搜索树）。
- `--bot heuristic-v1` 是冻结的旧版启发式，仅用于回测对照。
- 终局后自动写入仓库根目录：`tmp/pve-42-3p.json` 和 `.html`（`3p` = 1 人 + 2 bot）。

演示或测试时，可以让启发式 bot 代替你的席位自动打完：

```bash
cargo run -p splendor-pokemon-cli -- pve --bots 2 --auto --seed 42
```

常用终端命令：

```text
help                    查看全部命令
legal                   列出当前合法动作
take3 r b y             拿 3 种不同颜色的球
take2 r                 拿 2 个同色球
reserve board 1 0       保留场上 1 阶第 0 张
capture board 2 1       捕捉场上 2 阶第 1 张
capture reserved 0      捕捉自己的第 0 张保留牌
discard r 1 b 2 m 1     弃球（红 蓝 黄 粉 黑 大师）
evolve 0 board 2 1      把第 0 张已捕捉牌进化
end                     结束回合
quit                    退出
```

## 2. 让 AI 对打并记录结果

```bash
cargo run -p splendor-pokemon-cli -- bot-vs-bot --players 3 --seed 42
```

默认产出（仓库根目录）：

```text
tmp/bot-42-3p.json     # 逐动作回放时间线
tmp/bot-42-3p.html     # 可直接打开的可视化回放
```

自定义输出路径：

```bash
cargo run -p splendor-pokemon-cli -- bot-vs-bot --players 3 --seed 42 \
  --replay tmp/my-game.json --html tmp/my-game.html
```

参数：

| 参数 | 说明 |
|---|---|
| `--players 2\|3\|4` | 玩家人数，默认 2 |
| `--bot heuristic\|minimax\|heuristic-v1\|random` | bot 策略，默认 `minimax` |
| `--seed N` | 随机种子，默认 42；同 seed + 同策略结果完全一致 |
| `--replay <file>` | 回放 JSON 路径 |
| `--html <file>` | 回放 HTML 路径 |

JSON 的 `frames` 逐动作保存 `(Action, GameView)`；`result` 为胜者与分数，规则僵局时 `stalemate` 为 `true`。

## 3. AI 回测

固定赛制：2 人、10 个固定种子（1–10）+ 10 个增量种子，每个种子交换先后手各一局，共 40 局。候选得分率必须 ≥ 50%。

```bash
cargo run -p splendor-pokemon-cli -- arena \
  --candidate minimax --baseline heuristic-v1 \
  --require 50
```

官方模式会自动读取 `backend/arena-seeds.txt` 中的增量游标，跑完后把游标推进 10；游标文件随仓库提交，禁止重置或回退。调试可用 `--seeds N --seed-start M`，但不会消耗游标，也不能作为改动依据。

输出包含胜-负-平、得分率、固定/增量种子分项、双方平均分、先/后手分项与僵局数。`--verbose` 可逐局打印；`heuristic-v1` 是冻结的回测基线。`--candidate minimax` 可回测搜索策略。


## 4. 可视化结果

`bot-vs-bot` 和 PvE 都会同时生成 HTML，直接双击打开即可，或者：

```bash
# Windows Git Bash / cmd
start tmp/bot-42-3p.html
```

如果手上只有回放 JSON：

```bash
cargo run -p splendor-pokemon-cli -- replay tmp/my-game.json --html tmp/my-game.html
```

HTML 是单个自包含文件，没有服务器、CDN 或联网请求。支持：

- 上一步 / 下一步 / 自动播放 / 速度 / 进度条
- 键盘：`←` `→` 步进，空格播放/暂停
- 展示：场上牌、供应区、各玩家球与减免、分数、保留数、最近动作、终局或僵局提示
- 卡面：底色取减免颜色，中央是完整名字大字，右上角分数，左下角捕捉费用彩球

热座模式不产出回放；需要可视化请使用 PvE 或 `bot-vs-bot`。

## 5. Godot 回放可视化

`game/` 是独立的 Godot 4.6 工程，主场景是标题菜单（`scenes/menu.tscn`）：继续对局、新对局、观看回放、设置、退出。回放场景（`scenes/replay.tscn`）读取 `bot-vs-bot` / `pve` 写出的回放 JSON，逐动作绘制场上牌、供应区、玩家与事件；交互式 PvE 见 §6。回放界面只读，不建游戏、不改规则；私密信息按后端约定处理——`reserved` 只显示当前查看者可见的保留牌（自己的全部、对手从场上保留的牌），对手从牌堆顶保留的牌只显示数量。

在仓库根目录启动（本工作副本的 Godot 4.6 可执行文件在仓库上一级）：

```bash
# 打开标题菜单（主场景）
../Godot_v4.6.1-stable_win64.exe --path game

# 直接打开回放场景
../Godot_v4.6.1-stable_win64.exe --path game res://scenes/replay.tscn
```

`godot` 已在 `PATH` 时用 `godot --path game`。

- 回放路径解析：命令行 `-- <path>` > `SPLENDOR_REPLAY` > 菜单选择（`AppState.replay_path`）> 仓库 `tmp/` 下最新的 `*.json` > 空状态（点「打开回放…」选文件）。
- 场上牌与 HTML 回放同款卡面：减免色底、中央完整名字大字、右上角分数、左下角捕捉费用球。
- 载入指定回放：

  ```bash
  ../Godot_v4.6.1-stable_win64.exe --path game res://scenes/replay.tscn -- tmp/bot-42-2p.json
  ```

  也可以用环境变量 `SPLENDOR_REPLAY=tmp/bot-42-2p.json`。
- 快捷键与控件：

  | 操作 | 说明 |
  |---|---|
  | `←` / `→` | 上一帧 / 下一帧 |
  | `空格` / `P` | 自动播放 / 暂停 |
  | `Home` / `End` | 首帧 / 末帧 |
  | `Esc` | 返回标题菜单 |
  | 进度条 | 任意跳转 |
  | 速度 | 慢 / 中 / 快 / 极快（默认值来自设置） |
  | 打开回放… / 重新载入 | 选择任意回放 JSON / 重读当前文件 |
  | 悬停场上牌 | 显示完整卡牌 tooltip |
  | 悬停玩家面板 | 显示该玩家的完整捕捉 / 保留列表 |

- 横幅：终局显示胜者与分数；规则僵局显示无胜者说明。
- Godot 的导入缓存 `game/.godot/` 已被 `.gitignore` 忽略；`icon.svg.import` 与脚本 `.uid` 随仓库提交。

## 6. Godot 和 AI 对战

`game/scenes/battle.tscn` 是交互式 PvE：你是玩家 0，对手是 2–3 个 bot。规则判定、AI 决策与回放记录全部在 Rust 后端，Godot 只发送动作并渲染引擎返回的 `GameView`，不复制规则。服务只监听 `127.0.0.1`，没有联网或远程 PvP。

从标题菜单进入：「新对局」设置对手数量（2 / 3）、对手强度与随机种子；「继续对局」在存在进行中对局时可用，通过 `GET /api/game/state` 恢复。也可以直接打开对战场景，跳过菜单并使用默认设置（2 个 AI、极小化极大、种子 42）：

```bash
# 方式 A：先启动本地服务，再打开对战场景
cd backend
cargo run -p splendor-pokemon-cli -- serve          # 默认 127.0.0.1:43110
cd ..
../Godot_v4.6.1-stable_win64.exe --path game res://scenes/battle.tscn

# 方式 B：打开标题菜单，从「新对局」进入
../Godot_v4.6.1-stable_win64.exe --path game
```

菜单进入对战、而对战场景发现默认端口没有服务时，会尝试启动 `backend/target/{debug,release}/splendor-pokemon[.exe]`；该可执行文件不存在时退回 `cargo run`（首次需要编译，界面会显示启动状态）。可以用环境变量覆盖：

| 环境变量 | 说明 |
|---|---|
| `SPLENDOR_SERVER_URL` | 直接连接指定地址，例如 `http://127.0.0.1:43110`；设置后 Godot 不再自动启动服务 |
| `SPLENDOR_CLI` | 指定 `splendor-pokemon` 可执行文件路径，供自动启动使用 |

交互模型是「点对象 → 就地选项或行动篮 → 底栏确认」：

| 操作 | 说明 |
|---|---|
| 场上牌（左键 / 右键） | 选中；卡片抬起，正下方弹出「捕捉 / 保留」，不可用按钮置灰并写出缺球原因 |
| 精灵球 | 左键 +1、右键 −1，放入底栏行动篮；同色最多 2、总数最多 3，同色第 2 个要求该色供应至少 4 |
| 底栏确认 | 文案带具体内容（如「拿取 红×2 蓝×1」），不合法时禁用 |
| 我的保留牌 | 点击直接捕捉；不合法置灰 |
| 底栏「弃球…」 | 打开球架：持有区左键 +1 / 右键 −1，弃球区点击取回；正好降到 10 才能确认 |
| 底栏「进化…」 | 列出合法进化；「结束回合」在进化阶段常驻 |
| `数字 1–5` | 快速选择对应颜色精灵球 |
| `Tab` / 方向键 / `Enter` | 在棋盘与按钮间移动焦点，方向键选牌，Enter 选中 |
| `Esc` | 逐级取消（模态 → 选中 / 行动篮 → 打开暂停菜单） |
| 顶栏「菜单」 | 继续 / 重新开始（二次确认）/ 返回标题（二次确认，可从菜单继续） |
| 顶栏状态 | 阶段、当前行动者、种子、AI 数量与强度；不承载开局参数 |

- 非我方回合棋盘不可点，顶栏显示「等待对手」；AI 动作按返回的 `steps` 逐个播放，播完才恢复操作。
- 终局 / 僵局显示居中结算面板；完成后自动写入 `tmp/pve-<seed>-<n>p.json` 与同名 `.html`，可保存回放、再来一局或返回标题。
- 设置（标题菜单）保存 AI 行动速度与回放默认速度到 `user://settings.cfg`，不影响规则。
- 本地接口：`GET /api/health`、`GET /api/game/state`、`POST /api/game/new`、`POST /api/game/action`、`POST /api/game/replay/save`。请求 / 响应均为 JSON，单会话、不持久化。

## 验证

在 `backend/` 下：

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

Godot 菜单冒烟（在仓库根目录；退出码 0 表示通过）：

```bash
SPLENDOR_MENU_SMOKE=1 \
  ../Godot_v4.6.1-stable_win64_console.exe --headless --path game
```

检查五个菜单项、「继续对局」禁用状态、新对局设置三行参数、设置页选项与回放文件对话框。

Godot 回放冒烟：

```bash
SPLENDOR_SMOKE=1 SPLENDOR_REPLAY=tmp/bot-42-2p.json \
  ../Godot_v4.6.1-stable_win64_console.exe --headless --path game res://scenes/replay.tscn
```

冒烟会载入回放、逐帧渲染、检查 5 行牌 / 玩家面板 / 按钮边界 / 播放推进 / 终局横幅 / 布局尺寸 / 中文字形。

Godot 对战冒烟（需要先构建后端；终端 1 启动服务，终端 2 跑无头场景）：

```bash
# 终端 1（在 backend/ 下）
cargo run -p splendor-pokemon-cli -- serve --port 43111 --idle-timeout 60

# 终端 2（仓库根目录）
SPLENDOR_BATTLE_SMOKE=1 SPLENDOR_SERVER_URL=http://127.0.0.1:43111 \
  ../Godot_v4.6.1-stable_win64_console.exe --headless --path game res://scenes/battle.tscn
```

冒烟会真实调用本地服务：开局、检查对手面板 / 棋盘 / 球架 / 行动底栏、校验行动篮拼出的拿球动作、检查贴卡动作条、提交一次拿球和结束回合、打开进化 / 弃球 / 暂停模态，并检查中文字形。

更详细的模块与设计说明见 [`backend/README.md`](backend/README.md)；Godot 工程结构见 [`game/README.md`](game/README.md)。

