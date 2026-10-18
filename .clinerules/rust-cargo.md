# 规则：Rust 一律通过 cargo 运行【强制】

本项目的后端是 Rust workspace：`backend/Cargo.toml` + `backend/Cargo.lock` + `backend/crates/`。没有 Python 后端，禁止用 Python / uv / pytest 运行或测试本项目后端。

优先使用 PATH 中的 `cargo`。本工作副本在项目根 `.rust/` 放置了隔离工具链（`CARGO_HOME=.rust/cargo`、`RUSTUP_HOME=.rust/rustup`）；`.rust/` 已被 `.gitignore` 忽略，不随仓库分发，也不能放进 `tmp/`。

## ✅ 必须这样做

| 意图 | 命令（在 `backend/` 下执行） |
|---|---|
| 构建 | `cargo build --workspace` |
| 跑测试 | `cargo test --workspace` |
| 格式检查 | `cargo fmt --all -- --check` |
| 自动格式化 | `cargo fmt --all` |
| 静态检查 | `cargo clippy --workspace --all-targets -- -D warnings` |
| 跑 CLI | `cargo run -p splendor-pokemon-cli -- --players 3 --seed 42` |
| 新增依赖 | `cargo add <pkg>`（或改 `backend/Cargo.toml` 后构建） |

- 所有 Rust 命令在 `backend/` 下执行，或使用 `--manifest-path backend/Cargo.toml`。
- 依赖变更后 `backend/Cargo.lock` 必须一并提交；不要手改 lock 文件。
- 提交前至少跑过 `cargo test --workspace`、`cargo clippy --workspace --all-targets -- -D warnings`、`cargo fmt --all -- --check`。

## ✅ 项目内隔离工具链

若 `cargo` 不在 PATH，在仓库根用下面的前缀（把 `<cargo args>` 换成上表命令）：

```bash
CARGO_HOME="$PWD/.rust/cargo" RUSTUP_HOME="$PWD/.rust/rustup" \
  ./.rust/cargo/bin/cargo.exe <cargo args>
```

例如：

```bash
CARGO_HOME="$PWD/.rust/cargo" RUSTUP_HOME="$PWD/.rust/rustup" \
  ./.rust/cargo/bin/cargo.exe test --manifest-path backend/Cargo.toml --workspace
```

- 不要把 `.rust/` 加入 PATH 后再长期依赖；每条命令显式带 `CARGO_HOME` / `RUSTUP_HOME`，避免污染全局环境。
- `.rust/` 缺失且系统没有 `cargo` 时，告知用户需要安装 Rust 或恢复 `.rust/`，不要改用其他语言实现后端。

## ✅ 测试必须可终止【强制】

- Rust 没有内置的全局测试超时；测试必须自身有明确终止条件，禁止无界循环、无限等待或需要人工输入的测试。
- 怀疑某条测试可能挂起时，用外部超时包裹，例如 `timeout 120 cargo test --workspace`。
- 新增会启动服务、子进程或网络的测试，必须在测试内设置超时并在结束时清理进程。

## ❌ 禁止事项

1. 用 `python` / `python3` / `py` / `uv` / `pytest` 跑本项目后端。
2. 把 `.rust/`、`backend/target/` 提交到仓库。
3. 修改全局 Cargo / Rustup 配置或用户目录（`.cargo/`、`.rustup/`、PATH、环境变量）；本项目工具链固定在 `.rust/`。
4. 用 `cargo clean` 清理整个仓库或其他项目；只允许在 `backend/` 清理本项目 target。
5. 跳过 `cargo clippy` / `cargo fmt` 后宣称验证通过。

## 执行判定

- “跑测试 / 构建 / 起 CLI / 验证后端”一律等价于上表的 Cargo 命令，不退回 Python。
