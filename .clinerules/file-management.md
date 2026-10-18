# 规则：文件管理边界【强制】

项目根是仓库根 `murdoku/`。以下约束对 shell 命令、脚本、编辑器一视同仁。

## 1. 删除范围

- 项目外一律不可删：父目录 `E:\Projects\Godot_v4.6.1\`（Godot 引擎目录）、用户目录、系统目录、其他项目、`%TEMP%`。
- 判定用解析后的绝对路径，防 `..`、相对路径、符号链接 / junction 逃逸。
- 经脚本间接删除同样受限：`Remove-Item`、`del /s /q`、`rd /s /q`、`find -delete`、`shutil.rmtree` 等。

## 2. 项目内禁区

- 默认不删：`.git/`、`.clinerules/`、`.agents/`、`.vscode/`、`backend/uv.lock`、`levels/`、`docs/`、`plan.md`、`progress.md`。
- `backend/.venv/` 需要重建时先确认，改由 `uv sync` 重建。
- 只清理明确可再生之物：`__pycache__/`、`.pytest_cache/`、`.ruff_cache/`、临时产物；先列清单，禁止宽泛通配符。

## 3. 破坏性命令

- 禁止：`git clean -fdx`、`git reset --hard`、`git checkout -- .`、`git restore .`、`git stash drop/clear`、`git branch -D`、force push、改写已推送历史。
- 禁止宽泛的 `rm -rf` / `Remove-Item -Recurse` 匹配，这类操作会不可逆丢失未提交改动。

## 4. 写入与移动

- 只在项目根内写；不写用户目录、系统目录、引擎安装目录、全局配置（`git config --global`、`uv` / `pip` 全局、PATH、环境变量）。
- 项目外路径只读不写；跨根 `mv` 等同移出项目，禁止。项目内移动 / 重命名可以，但 tracked 文件不得移出仓库。
- 临时产物统一放根目录 `tmp/`（已在 `.gitignore` 忽略）：临时脚本、一次性小工具、中间文件都放这里，用完清理，项目内其他位置不留临时文件。`tmp/` 不存在时按需创建。

## 5. 不确定时

- 路径归属不明、含变量 / 通配符，或涉及用户产出（`levels/`、`docs/`、存档、截图）时先问，不猜。
- 删除前说明删什么、为什么、能否恢复；发现未提交改动先停止并报告。
- 部分失败如实报告，不隐瞒、不静默重试。
