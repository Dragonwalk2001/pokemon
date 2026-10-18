# 璀璨宝石：宝可梦 — Rust 后端

当前形态：**规则引擎库 + 交互式 CLI**。HTTP / WebSocket 服务尚未实现。

## 目录

```text
backend/
├── Cargo.toml                       # workspace
└── crates/
    ├── splendor-pokemon-core/       # 规则引擎（库）
    │   ├── src/catalog.rs           # 编译期解析 data/cards.csv
    │   ├── src/game.rs              # 权威状态机
    │   ├── src/types.rs             # 卡牌、动作、事件、资源类型
    │   ├── src/view.rs              # 按查看者过滤私有信息的只读视图
    │   └── tests/                   # 卡牌数据与公开 API 测试
    └── splendor-pokemon-cli/        # 交互式对局 CLI
```

数据源是仓库根目录的 `data/cards.csv`（编译期 `include_str!` 嵌入）与 `data/rules.md`。

## 运行

需要 Rust stable 工具链（`cargo` / `rustc`）。

```bash
cd backend

# 3 名玩家，固定随机种子
cargo run -p splendor-pokemon-cli -- --players 3 --seed 42

# 指定名字（人数由名字数量决定）
cargo run -p splendor-pokemon-cli -- --names Alice,Bob,Carol

# 测试
cargo test
```

当前工作副本在项目根 `.rust/` 准备了隔离工具链（`.gitignore` 忽略，不随仓库分发）。若 PATH 中没有 `cargo`，在仓库根执行：

```bash
CARGO_HOME="$PWD/.rust/cargo" RUSTUP_HOME="$PWD/.rust/rustup" \
  ./.rust/cargo/bin/cargo.exe test --manifest-path backend/Cargo.toml --workspace

CARGO_HOME="$PWD/.rust/cargo" RUSTUP_HOME="$PWD/.rust/rustup" \
  ./.rust/cargo/bin/cargo.exe run --manifest-path backend/Cargo.toml \
  -p splendor-pokemon-cli -- --players 3 --seed 42
```

`.rust/` 缺失时需要自行安装 Rust；工具链不要放进 `tmp/`。

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

- HTTP / WebSocket 接口与房间、账号体系。
- AI 对手。
- 角色立牌的特殊能力（规则文档未提供角色效果数据，当前只记录进化次数用于平手判定）。
- 卡图与前端资源。
