# 璀璨宝石：宝可梦（离线原型）

**操作全部在命令行；可视化是“对局结束后生成 HTML，用浏览器打开回放”。**
没有常驻 GUI、Godot 实时界面、联网或 PvP。

三种用法：

| 模式 | 操作方式 | 是否记录回放 |
|---|---|---|
| 本地热座 | 终端里多人轮流输入 | 否 |
| PvE（1 人 + 2–3 bot） | 终端交互，bot 自动行动 | 是，终局后写文件 |
| bot-vs-bot | 终端自动跑完 | 是，默认写文件 |

## 环境

需要 Rust 的 `cargo`。以下命令都在 `backend/` 下执行：

```bash
cd backend
```

如果 `cargo` 不在 PATH（本工作副本在根目录 `.rust/` 带了隔离工具链），先执行（Git Bash；PowerShell 用 `$env:CARGO_HOME` / `$env:RUSTUP_HOME`）：

```bash
export CARGO_HOME="$PWD/../.rust/cargo"
export RUSTUP_HOME="$PWD/../.rust/rustup"
# 然后把下面所有 cargo 换成 ../.rust/cargo/bin/cargo.exe
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
- `--bot random` 可把 bot 换成随机策略（默认 `heuristic` 启发式）。
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
| `--bot heuristic\|random` | bot 策略，默认 `heuristic` |
| `--seed N` | 随机种子，默认 42；同 seed + 同策略结果完全一致 |
| `--replay <file>` | 回放 JSON 路径 |
| `--html <file>` | 回放 HTML 路径 |

JSON 的 `frames` 逐动作保存 `(Action, GameView)`；`result` 为胜者与分数，规则僵局时 `stalemate` 为 `true`。

## 3. 可视化结果

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

热座模式不产出回放；需要可视化请使用 PvE 或 `bot-vs-bot`。

## 验证

```bash
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

更详细的模块与设计说明见 [`backend/README.md`](backend/README.md)。
