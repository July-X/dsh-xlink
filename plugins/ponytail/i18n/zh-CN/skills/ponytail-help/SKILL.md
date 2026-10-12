---
name: ponytail-help
description: >
  ponytail 全部档位、技能与命令的速查卡。一次性展示。
  用于 /ponytail-help、「ponytail help」「ponytail 怎么用」。
---

# Ponytail Help

被调用时展示这张速查卡。一次性：**不要**改档位、写标志文件或持久化任何东西。

## 档位

| 档位 | 触发 | 改变什么 |
|-------|---------|-------------|
| **Lite** | `/ponytail lite` | 只做被要求的改动，用一行点出更小的选项。 |
| **Full** | `/ponytail` | 最小的完整改动、逻辑该有的地方配上检查，以及点明略过项与风险的回复。默认档。 |
| **Ultra** | `/ponytail ultra` | 还要质疑需求本身，动手之前先顶回去。 |

档位一直保持，直到改档或会话结束。

## 技能

| 技能 | 触发 | 做什么 |
|-------|---------|--------------|
| **ponytail** | `/ponytail` | 懒人模式本身：最少的新代码，以及点明略过项与风险的清楚回复。 |
| **ponytail-review** | `/ponytail-review` | 对一个 diff 做质量评审：bug、安全、负载、缺测试、速度、能砍什么。每条发现都说清会坏在哪、怎么修。 |
| **ponytail-audit** | `/ponytail-audit` | 同样那套质量评审，用在整个仓库上，按重要性排序。 |
| **ponytail-debt** | `/ponytail-debt` | 把 `shortcut:` 注释收成一份可跟踪的台账。 |
| **ponytail-gain** | `/ponytail-gain` | 实测收益记分板：更少代码、更低成本、更快速度。 |
| **ponytail-help** | `/ponytail-help` | 就是这张卡。 |

在 Codex CLI 与 IDE 扩展里用 `$ponytail:ponytail`、`$ponytail:ponytail-review`
或 `$ponytail:ponytail-help` 调用技能。Claude Code 与 OpenCode 用上面这些斜杠命令形式
（OpenCode 把六个都做成斜杠命令）。

## 关闭

说「stop ponytail」或「normal mode」。随时用 `/ponytail` 恢复。
`/ponytail off` 同样有效。

## 配置默认档位

默认档位 = `full`，每个会话自动生效。要改：

**环境变量**（优先级最高）：
```bash
export PONYTAIL_DEFAULT_MODE=ultra
```

**配置文件**（`~/.config/ponytail/config.json`，Windows：`%APPDATA%\ponytail\config.json`）：
```json
{ "defaultMode": "lite" }
```

设成 `"off"` 可以关掉会话启动时的自动激活，需要时手动 `/ponytail`。

优先级：环境变量 > 配置文件 > `full`。

## 更新

> **本宿主（dsh-xlink 工作台）的更新走桌面壳，不走上面那段。**
> 在工作台里用 `/ponytail-update`，或者在插件面板点「检查更新」。

> **Claude Code 用户**：`/plugin` → Marketplaces → 选 ponytail → Enable auto-update，
> 之后 Claude Code 会在启动时拉新版本（它提示时跑 `/reload-plugins`）。
> 手动刷新：`/plugin marketplace update ponytail` 然后 `/reload-plugins`。
> 如果 `/plugin` 不认识，说明你的 Claude Code 版本太旧，先升级再重启。

## 更多

完整文档与示例：https://github.com/DietrichGebert/ponytail