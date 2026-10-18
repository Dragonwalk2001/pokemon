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

`game/` 是独立的 Godot 4.6 工程：回放场景读取 `bot-vs-bot` / `pve` 写出的同一份回放 JSON，逐动作绘制场上牌、供应区、玩家与事件；另有交互式的「和 AI 对战」场景（见 §6）。回放界面只读，不建游戏、不改规则；私密信息按后端约定处理——`reserved` 只显示当前查看者可见的保留牌（自己的全部、对手从场上保留的牌），对手从牌堆顶保留的牌只显示数量。

在仓库根目录启动（本工作副本的 Godot 4.6 可执行文件在仓库上一级）：

```bash
../Godot_v4.6.1-stable_win64.exe --path game
```

`godot` 已在 `PATH` 时用 `godot --path game`。

- 首次打开会自动载入仓库 `tmp/` 下最新的 `*.json`；没有回放时显示空状态，点「打开回放…」选文件。
- 场上牌与 HTML 回放同款卡面：减免色底、中央完整名字大字、右上角分数、左下角捕捉费用球。
- 载入指定回放：

  ```bash
  ../Godot_v4.6.1-stable_win64.exe --path game -- tmp/bot-42-2p.json
  ```

  也可以用环境变量 `SPLENDOR_REPLAY=tmp/bot-42-2p.json`。
- 快捷键与控件：

  | 操作 | 说明 |
  |---|---|
  | `←` / `→` | 上一帧 / 下一帧 |
  | `空格` / `P` | 自动播放 / 暂停 |
  | `Home` / `End` | 首帧 / 末帧 |
  | 进度条 | 任意跳转 |
  | 速度 | 慢 / 中 / 快 / 极快 |
  | 打开回放… / 重新载入 | 选择任意回放 JSON / 重读当前文件 |
  | 悬停场上牌 | 显示完整卡牌 tooltip |
  | 悬停玩家面板 | 显示该玩家的完整捕捉 / 保留列表 |

- 横幅：终局显示胜者与分数；规则僵局显示无胜者说明。
- Godot 的导入缓存 `game/.godot/` 已被 `.gitignore` 忽略；`icon.svg.import` 与脚本 `.uid` 随仓库提交。

## 6. Godot 和 AI 对战

`game/scenes/battle.tscn` 是交互式 PvE：你是玩家 0，对手是 2–3 个 bot。规则判定、AI 决策与回放记录全部在 Rust 后端，Godot 只发送动作并渲染引擎返回的 `GameView`，不复制规则。服务只监听 `127.0.0.1`，没有联网或远程 PvP。

启动方式二选一：

```bash
# 方式 A：先启动本地服务，再打开 Godot 对战场景
cd backend
cargo run -p splendor-pokemon-cli -- serve          # 默认 127.0.0.1:43110
cd ..
../Godot_v4.6.1-stable_win64.exe --path game res://scenes/battle.tscn

# 方式 B：直接打开 Godot 主场景，点右上角「和 AI 对战」
../Godot_v4.6.1-stable_win64.exe --path game
```

方式 B 发现默认端口没有服务时，会尝试启动 `backend/target/{debug,release}/splendor-pokemon[.exe]`；该可执行文件不存在时退回 `cargo run`（首次需要编译，界面会显示启动状态）。可以用环境变量覆盖：

| 环境变量 | 说明 |
|---|---|
| `SPLENDOR_SERVER_URL` | 直接连接指定地址，例如 `http://127.0.0.1:43110`；设置后 Godot 不再自动启动服务 |
| `SPLENDOR_CLI` | 指定 `splendor-pokemon` 可执行文件路径，供自动启动使用 |

界面操作：

| 操作 | 说明 |
|---|---|
| 场上牌左键 | 选中卡牌，在右侧出现「捕捉 / 保留」按钮 |
| 场上牌右键 | 直接尝试保留 |
| 供应区颜色 | 点击循环 `0 → 1 → 2 → 0`；「拿取所选」提交拿 3 色或拿 2 同色 |
| 我的保留牌 | 点击尝试捕捉 |
| 弃球阶段 | 点击颜色循环选择弃球数量，达到要求后「确认弃球」 |
| 进化阶段 | 按钮列出合法进化目标，「结束回合」跳过进化 |
| 顶栏 | 选择 AI 数量 / 策略 / 种子；「新对局」重开；「回放」返回只读回放界面 |

- 人类动作后，服务端连续推进 bot 回合，界面按返回的步骤逐个播放；再次轮到人类时才恢复操作。
- 终局或规则僵局会显示横幅；完成后自动写入 `tmp/pve-<seed>-<n>p.json` 与同名 `.html`。
- 本地接口：`GET /api/health`、`GET /api/game/state`、`POST /api/game/new`、`POST /api/game/action`、`POST /api/game/replay/save`。请求 / 响应均为 JSON，单会话、不持久化。

## 验证

在 `backend/` 下：

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

Godot 回放冒烟（在仓库根目录；退出码 0 表示通过）：

```bash
SPLENDOR_SMOKE=1 SPLENDOR_REPLAY=tmp/bot-42-2p.json \
  ../Godot_v4.6.1-stable_win64_console.exe --headless --path game
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

冒烟会真实调用本地服务：开局、检查棋盘 / 玩家 / 供应区、校验界面构造的拿球与保留动作、提交一次拿球和结束回合、确认 AI 回合后回到人类，并检查中文字形。

更详细的模块与设计说明见 [`backend/README.md`](backend/README.md)；Godot 工程结构见 [`game/README.md`](game/README.md)。

