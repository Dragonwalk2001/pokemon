# 璀璨宝石：宝可梦 — Rust 后端

当前形态：**规则引擎库 + 交互式 CLI + 离线 bot / HTML 回放**；同仓库 `game/` 提供只读的 Godot 回放前端。HTTP / WebSocket 服务尚未实现，不含联网与 PvP。

## 目录

```text
backend/
├── Cargo.toml                       # workspace
├── arena-seeds.txt                  # 增量种子游标（官方回测自动推进）
└── crates/
    ├── splendor-pokemon-core/       # 规则引擎（库）
    │   ├── src/catalog.rs           # 编译期解析 data/cards.csv
    │   ├── src/game.rs              # 权威状态机
    │   ├── src/types.rs             # 卡牌、动作、事件、资源类型
    │   ├── src/view.rs              # 按查看者过滤私有信息的只读视图
    │   ├── src/bot.rs               # 可复现的随机 / 启发式 bot
    │   ├── src/minimax.rs           # 极小化极大搜索 bot（捕捉阶段）
    │   ├── src/heuristic_v1.rs      # 冻结的启发式 v1（回测基线）
    │   ├── src/replay.rs            # 逐动作回放时间线与 JSON
    │   └── tests/                   # 卡牌数据、公开 API、bot 测试
    └── splendor-pokemon-cli/        # 对局 CLI
        ├── src/main.rs              # 子命令分发与热座对局
        ├── src/pve.rs               # 1 人 + 2–3 bot
        ├── src/bot_cmd.rs           # bot-vs-bot / replay 子命令
        ├── src/arena.rs              # 固定 20 种子 × 先后手互换的 40 局回测
        ├── src/html.rs              # 回放 JSON → 自包含 HTML
        ├── src/ui.rs                # 终端渲染与命令解析
        ├── assets/replay.html       # 内嵌 CSS / JS 的离线回放模板
        └── tests/cli.rs             # CLI 端到端冒烟测试
```

数据源是仓库根目录的 `data/cards.csv`（编译期 `include_str!` 嵌入）与 `data/rules.md`。仓库根的 `game/` 是独立 Godot 4.6 工程，只读回放 JSON，不属于 Rust workspace。

## 运行

需要 Rust stable 工具链（`cargo` / `rustc`）。

```bash
cd backend

# 本地热座：3 名玩家，固定随机种子
cargo run -p splendor-pokemon-cli -- --players 3 --seed 42

# 指定名字（人数由名字数量决定）
cargo run -p splendor-pokemon-cli -- --names Alice,Bob,Carol

# PvE：1 人 + 2 个启发式 bot，终端交互
cargo run -p splendor-pokemon-cli -- pve --bots 2 --seed 42

# PvE 演示 / 测试：人类席位由启发式 bot 代打，可离开终端跑完
cargo run -p splendor-pokemon-cli -- pve --bots 2 --auto --seed 42

# bot-vs-bot：默认写 tmp/bot-<seed>-<n>p.json 与同名 .html
cargo run -p splendor-pokemon-cli -- bot-vs-bot --players 3 --seed 42 \
  --replay tmp/replay.json --html tmp/replay.html

# 40 局回测：10 个固定种子 + 10 个增量种子，先后手互换
cargo run -p splendor-pokemon-cli -- arena \
  --candidate minimax --baseline heuristic-v1 \
  --require 50

# 把已有回放 JSON 重新渲染成 HTML
cargo run -p splendor-pokemon-cli -- replay tmp/replay.json --html tmp/replay.html

# 测试
cargo test --workspace
```

当前工作副本在项目根 `.rust/` 准备了隔离工具链（`.gitignore` 忽略，不随仓库分发）。若 PATH 中没有 `cargo`，在仓库根执行：

```bash
CARGO_HOME="$PWD/.rust/cargo" RUSTUP_HOME="$PWD/.rust/rustup" \
  ./.rust/cargo/bin/cargo.exe test --manifest-path backend/Cargo.toml --workspace

CARGO_HOME="$PWD/.rust/cargo" RUSTUP_HOME="$PWD/.rust/rustup" \
  ./.rust/cargo/bin/cargo.exe run --manifest-path backend/Cargo.toml \
  -p splendor-pokemon-cli -- --players 3 --seed 42
```

`.rust/` 缺失时需要自行安装 Rust；工具链不要放进 `tmp/`。Git Bash / PowerShell 的会话级 `PATH` 写法见根 [`README.md`](../README.md) 的「环境」一节。

CLI 启动后输入 `help` 查看命令。主要命令：

| 命令 | 说明 |
|---|---|
| `show` | 显示场上、供应区、玩家状态与自己的保留牌 |
| `legal` | 列出当前阶段全部合法动作 |
| `take3 r b y` | 拿 3 种不同普通球；供应区不足 3 种时只能拿剩余种类各 1 个 |
| `take2 r` | 拿 2 个同色普通球，要求供应区该色至少 4 个 |
| `reserve board 1 0` | 保留场上 1 阶第 0 张，并获得 1 个大师球 |
| `reserve deck 2` | 保留 2 阶牌堆顶（牌面隐藏） |
| `capture board 2 1` | 捕捉场上 2 阶第 1 张 |
| `capture reserved 0` | 捕捉自己的第 0 张保留牌 |
| `discard r 1 b 2 m 1` | 弃球；也可写 6 个数字：红 蓝 黄 粉 黑 大师 |
| `evolve 0 board 2 1` | 把第 0 张已捕捉牌进化为场上 2 阶第 1 张 |
| `evolve 0 reserved 0` | 进化为自己的第 0 张保留牌 |
| `end` | 结束回合 |
| `save <path>` / `load <path>` | 保存/读取完整对局 JSON（含目录与牌堆） |
| `quit` | 退出 |

## Bot 与回放

- **策略**：默认对局使用 `MinimaxBot`，它在出现捕捉机会时用公开信息模拟对局并跑 alpha-beta 极小化极大搜索（默认看 5 个回合），拿球 / 保留 / 弃球 / 进化沿用启发式底座；`HeuristicBot` 以 `heuristic-v1` 为底座，只在捕捉选择上加入「推进自己的进化门槛 / 阻止对手进化」的评分；`RandomBot` 在合法动作中均匀随机；`HeuristicV1Bot` 是冻结的回测基线，不参与默认对局。
- **搜索边界**：`MinimaxBot` 只用 `GameView` 的公开信息，对手保留牌只保留数量，牌堆顶不可见且模拟中不补牌；搜索固定深度与分支宽度，不读墙钟时间，同一 seed 可逐字节复现。规则层僵局按平局评估，避免搜索把「让对手无行动」误判为优势。
- **回测**：`arena` 官方模式使用 10 个固定种子（1–10）+ 10 个增量种子，每个种子交换先后手，共 40 局 2 人对局；增量游标保存在 `backend/arena-seeds.txt`，完整跑完后自动前进 10。输出胜-负-平、得分率、固定/增量种子分项、双方平均分、先/后手分项与僵局数。`--require 50` 在得分率低于门槛或出现僵局时返回失败。调试用 `--seeds N --seed-start M`，不消耗游标。完整规矩见 [`.clinerules/ai-backtest.md`](../.clinerules/ai-backtest.md)。
- **可复现**：bot 使用独立 `BotRng`（SplitMix64），种子由 `derive_seed(game_seed, player_id)` 派生；同一 seed + 同一 bot 组合产生逐字节一致的回放。分数相同的候选由种子随机打破平局。
- **只读决策**：bot 只接收 `GameView`，与未来 HTTP / Godot 客户端使用同一信息边界；不修改规则语义。
- **回放结构**：`Replay { version, seed, players, controllers, frames, result, stalemate }`。`frames[0]` 是开局视图，之后每帧保存 `actor + Action + GameView + 本步事件文本`。为避免整局日志在每帧重复，帧内 `view.log` 只保留本步新增事件。
- **规则僵局**：3 人局可能把普通球全部持有到供应区只剩大师球，此时若当前玩家无牌可捕捉，引擎没有合法主行动且规则未定义 pass。回放以 `stalemate: true`、无胜者收尾，不新增动作、不改变规则语义。
- **输出位置**：`pve` / `bot-vs-bot` 默认写 `tmp/pve-<seed>-<n>p.*` 或 `tmp/bot-<seed>-<n>p.*`；`--replay` / `--html` 可覆盖。`tmp/` 已被 `.gitignore` 忽略。
- **HTML 回放**：单个文件内嵌全部 CSS / JS / JSON，不引用 CDN、字体或图片，`file://` 直接打开。支持上一步 / 下一步 / 自动播放 / 速度 / 进度条 / 键盘方向键，展示场上牌、供应区、各玩家球与减免、分数、保留数、最近动作与终局横幅。场上牌以减免色为底，中央是完整名字大字（按名字长度自适应字号），左下角是捕捉费用彩球与数字，右上角是分数。选 HTML 而非 ratatui：无需额外终端依赖、卡片信息密度更高、可截图分享，且同一份回放 JSON 就是 `game/` Godot 前端的数据源。
- **Godot 回放**：`game/` 是独立 Godot 4.6 工程，读取同一份回放 JSON，逐帧绘制场上牌、供应区、玩家与事件；场上牌使用与 HTML 相同的卡面语言（减免色底、中央大字、左下费用球），只读界面，不改规则。启动、操作与无头冒烟命令见根 README 与 [`game/README.md`](../game/README.md)。

## 规则覆盖

- 2–4 人初始化：普通球每种 4 / 5 / 7 个，大师球 5 个；1/2/3 阶各翻 4 张，稀有与传说各 1 张。
- 主要行动：拿 3 种不同球、拿 2 个同色球、保留牌、捕捉牌。
- 保留：最多 3 张，从供应区拿 1 个大师球；稀有与传说牌不可保留；从牌堆顶保留时对对手隐藏牌面。
- 捕捉：已捕捉牌的永久减免生效，大师球可替代任意普通球；支付后立即补牌。
- 回合末：球数超过 10 必须弃球；之后可进化一只，也可以不进化。
- 进化：1→2、2→3，只计算已捕捉牌提供的减免；目标必须在场上或自己的保留牌中；旧牌移入立牌下，不再提供分数与减免。
- 终局：有人达到 18 分后补完本轮，按「分数 → 进化次数 → 面板牌数」排序，仍相同则并列。

## 设计约定

- **权威判定**：所有状态变更只能经 `Game::apply(player, action)`；非法动作返回 `GameError`，且不会留下部分修改。
- **确定性**：同一 `seed` 产生同一牌序，便于复现与测试。
- **支付策略**：彩色球优先支付，颜色不足的差额用大师球补，最后加上卡牌印刷的大师球费用。引擎不接受「有彩色球却主动用大师球支付」的显式指定。
- **阶段机**：`MainAction → Discard?（球数 > 10）→ Evolution → 下一回合`。进化阶段可以再次调用 `Evolve` 会被拒绝，直到 `EndTurn`。
- **私有信息**：`Game::view_for(player)` 只对本人暴露保留牌牌面，对对手仅暴露数量；`GameView::legal_actions` 也只对当前玩家填充。`Game` 的完整 JSON 含牌堆顺序与所有保留牌，仅用于存档/调试，禁止直接发给客户端。
- **弃球枚举**：`legal_actions()` 在 `Discard` 阶段返回空列表，因为组合数量过大；调用方构造 `Action::Discard`，由引擎校验。

## 尚未实现

- 联网、PvP、HTTP / WebSocket 接口与房间、账号体系。
- 搜索 / 学习型 bot（当前为启发式 + 定向改写，未做博弈搜索）。
- 角色立牌的特殊能力（规则文档未提供角色效果数据，当前只记录进化次数用于平手判定）。
- 卡图与音效：HTML / Godot 回放均用纯色块 + 文字绘制，无宝可梦立绘与音效。
