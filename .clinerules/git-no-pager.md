# 规则：git 命令一律禁用分页器【强制】

agent 的 shell 非交互、无 TTY。`git log` / `diff` / `show` / `blame` 等输出超过一屏会进 `less`，翻不了页也退不出，命令直接卡死。因此 git 读命令一律写成 `git --no-pager <子命令>`。

## ✅ 必须这样做

| 意图 | 命令 |
|---|---|
| 提交历史 | `git --no-pager log -5` |
| 工作区 / 已暂存改动 | `git --no-pager diff` / `git --no-pager diff --cached` |
| 某次提交 / 逐行来源 | `git --no-pager show <rev>` / `git --no-pager blame <file>` |
| 分支 / 标签 / 远程 / stash | `git --no-pager branch`、`tag`、`remote -v`、`stash list` |
| 引用变动 / 精简日志 | `git --no-pager reflog` / `git --no-pager shortlog -sn` |

- `--no-pager` 必须在子命令**之前**：`git --no-pager log`，不是 `git log --no-pager`。
- 对任意 git 子命令都适用，不必判断这次输出会不会超一屏；`git status` 不分页，可直接用。

## ❌ 禁止事项

1. 裸 `git log` / `git diff` / `git show` / `git blame`
2. `git … | head`，或管道接 `less` / `more` 规避分页
3. 改全局 `core.pager` 配置（属用户环境，agent 不得改动）
4. 交互式命令：`git add -p`、`git rebase -i`、不带 `-m` 的 `commit`、`git config --edit`

命令卡住先怀疑分页器，不要重复重试同一条裸命令。

