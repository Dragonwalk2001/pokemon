# 规则：Python 一律通过 uv 运行（禁止裸 python）【强制】

本项目的 Python 环境由 uv 管理：`backend/.venv` + `backend/uv.lock` + `backend/.python-version`（钉住 3.12）。
机器上还存在其他解释器（系统 `C:\Python314`、miniconda），它们**不属于本项目**，禁止直接使用。

## ✅ 必须这样做

| 意图 | 唯一正确命令 |
|---|---|
| 跑测试 | `cd backend && uv run pytest`（timeout 已由 pytest 配置强制生效，见下节） |
| 跑任意 Python 代码 | `cd backend && uv run python …` |
| 跑 CLI 入口 | `cd backend && uv run murdoku …` |
| 重建/同步环境 | `cd backend && uv sync --extra dev` |
| 新增依赖 | `cd backend && uv add <pkg>`（或改 `pyproject.toml` 后 `uv sync`） |

- 所有命令在 `backend/` 目录下执行（`.python-version` 在那里，uv 会自动选对解释器）。
- **不需要激活虚拟环境**：`uv run` 自动使用 `backend/.venv`。

## ✅ 测试必须带 timeout【强制】

- 所有测试任务（`pytest` 及任何会执行测试的命令）**必须带超时保护**，防止死循环/卡死拖垮会话。
- 项目已在 `backend/pyproject.toml` 的 `[tool.pytest.ini_options]` 中配置 `timeout = 60`（依赖 `pytest-timeout`，在 dev extras 中），跑 `uv run pytest` 时自动生效，无需额外参数。
- 单个慢测试需要更长时限时，用 `@pytest.mark.timeout(N)` 标记覆盖，不要调大全局值，也不要去掉全局配置。
- 若新增测试入口（脚本、CI 等）不经过 pytest 配置，必须显式加 `--timeout=60`（或更大合理值）。

## ❌ 禁止事项

1. 直接调用 `python` / `python3` / `python.exe` / `py` —— 它们指向系统 Python 3.14 或 miniconda，不是项目环境
2. 直接调用 `backend/.venv/Scripts/python.exe` —— 绕过 uv 会让环境与 `uv.lock` 失去同步保证
3. 手动激活 venv（`source .venv/Scripts/activate` / `.venv\Scripts\activate`）后再运行
4. 手动 `pip install` 到任何环境；依赖变更只能经 `pyproject.toml` + `uv sync`/`uv add`
5. `python -m pytest` / `python -m murdoku…` 等任何裸解释器形式
6. 跑测试不带任何 timeout 保护（删除 `pyproject.toml` 里的 `timeout` 配置、跳过 `pytest-timeout`，或用无超时的方式调用测试）

## 执行判定

- 凡是"跑测试 / 跑脚本 / 起 CLI / 验证后端"的意图，一律等价于 `uv run …`，不要退回系统 python。
- 若 shell 中 `uv` 不在 PATH：优先用完整路径 `C:/Users/Administrator/.local/bin/uv.exe`；仍失败则提示用户重启 shell 或安装 uv，**而不是改用裸 python**。
- 依赖变更提交时，`uv.lock` 必须一并提交。