# Ponytail 内置插件（适配层）

把 [DietrichGebert/ponytail](https://github.com/DietrichGebert/ponytail) 的
「懒惰阶梯」接进 dsh-xlink 的工作台。**本仓库只有适配层，上游内容不随应用分发。**

## 这里发什么、不发什么

| | 随应用分发 | 不分发 |
| --- | --- | --- |
| 内容 | `host/`（Host 适配器）、`adapter/`（更新脚本 + `ponytail-update` 技能）、`LICENSE` | 六个 `SKILL.md`、`hooks/`（规则注入与档位切换） |

上游内容由**桌面壳**在启用时从 npm 拉取（默认 registry 是 npmmirror），校验
`dist.integrity` 后解包到实例的物化目录。Host 适配器从那里读。

**为什么不 vendor 进仓库**（前一版真的 vendor 了，还为此写过一套逐字落盘 + 哈希
锁定的脚本，那套脚本本身是对的）：vendor 会**制造两份内容** —— 仓库里钉死的那
一份，和用户手上的那一份。上游每改一次就要重发一版应用才能跟上，用户升级之前
一直用旧内容；更要命的是「面板上写的版本」与「工作台里跑的内容」会分叉，而两边
都不报错。顺带一个真实事故：第一版手写转录时丢了一个逗号，文件**大小完全相同**
（2901 字节）而内容有 122 字节位移 —— 当时 README 写着「逐字 vendor」。

## 上游坐标与许可

| | |
| --- | --- |
| 上游 | <https://github.com/DietrichGebert/ponytail> |
| npm 包 | `@dietrichgebert/ponytail` |
| 许可 | MIT（`LICENSE`），Copyright © Dietrich Gebert |
| 原生支持 | 上游自己支持 16+ 个 agent，**能直接用的宿主不必装本适配层** |

本适配层代码与文档以 MIT 发布。第三方归属见 `LICENSE`。

## 什么时候更新

| 触发 | 时机 |
| --- | --- |
| 启用插件 | 同步一次；**失败会当场拦下启用**并说明原因（用户手上因此不会有一个「装上了但没有内容」的插件） |
| 每次启动工作台 | 检查一次，按 24 小时节流；失败走 30 分钟短退避、不消耗每日额度 |
| 面板「检查更新」 | 立即拉一次 |
| 工作台里 `/ponytail-update` | 立即拉一次，并汇报结果（技能会跑 `adapter/update.mjs`，它请求桌面壳的���环桥） |

同步**只写数据目录**（`<DSH_HOME>/plugins/builtin/ponytail/upstream/<版本>/`），
永远不改随包产物，也不碰实例的物化目录与接线行。失败时继续用手上那一版。

换内容要**重启工作台**才生效（工作台已经加载过旧的），面板与技能文案都这么说。

## hook 怎么用

ponytail 的 hook 是一份**标准 Claude Code `hooks.json`**：`SessionStart` 注入规则集、
`UserPromptSubmit` 切档位、`SubagentStart` 传给子 agent。dsh 内核自带
`@deepseek-ai/dsh-hooks-claude-code` 这个桥，跑的就是 unmodified 的 Claude Code
hook 配置，所以**上游脚本一个字都不用改**。

挂它的只有一处：`host/index.js` 的 `apply()` 里 `ctx.plugin(...)`。桥**不扫插件目录**
（`configPath` 必填），所以必须显式挂。

核对过的三处（`@deepseek-ai/*` 0.2.1-alpha.1 实读）：

1. `SessionStartSource = 'startup'|'resume'|'clear'|'compact'`，与上游 matcher
   `startup|resume|clear|compact` 的四个词**逐字相同**；桥按 claude-code 方言把
   `a|b|c` 当**字面** alternation 比对。
2. 脚本吐 `hookSpecificOutput.additionalContext`，协议包解析该键后由桥
   `agent.inject(...)` 注入 —— 规则**只注入一次**，不是每轮重发。
3. `${CLAUDE_PLUGIN_ROOT}` 由桥的 `pluginRoot` 替换成物化目录。

**此前文档写「档位切换依赖别家 agent 的生命周期钩子，dsh 没有对应面」是错的**，
代价是白扔了一个核心能力。

两个必须记住的前提：

- **`hooks/package.json`（`{"type":"commonjs"}`）不能少**。上游脚本是 `require()`
  的 CommonJS，而插件根的 `package.json` 写着 `"type": "module"`；Node 按**离文件
  最近**的 package.json 判定模块类型。缺了它 `require()` 抛 ERR_REQUIRE_ESM，
  而 **hook 失败是非阻塞的** —— 症状是「配好了但什么都没发生」，日志里只有一行 warn。
  壳在物化时也会补这一份，于是两条路径的模块类型一致。
- **hook 需要 `node` 在 PATH 上**。命令串是 `node -e "require(...)"`，由内核 spawn。
  本仓起内核时用 `process::command_with_path` 的合并 PATH，Node 因此在 PATH 上。

## 默认关闭

技能与规则是指令文本、会整体进模型上下文。这与 OpenAI OAuth 那类「不登录就没副作用」
的内置插件不是一回事 —— 它第一条消息就改变行为。同名时**用户自己装的技能优先**
（bundled rank 600 低于壳管理的活动视图 300 与内核默认根 400/500）。

## 做不到的部分

- **statusline 徽标**（`ponytail-statusline.sh` / `.ps1`）：宿主没有可配置的
  状态栏，没有对应面。
- `ponytail-gain` 的收益记分牌依赖上游的基准数据与宿主侧统计展示。
- 规则正文与技能正文是**上游英文原文**（上游没有中文版）；面板说明是中文。

## 目录结构

```
plugins/ponytail/
  host/index.js                 Host 适配器：注册 provider + 挂 hook 桥
  host/skills.js                发现 / 解析 / provider 工厂（无外部依赖，可被测试 import）
  adapter/update.mjs            /ponytail-update 的执行体（请求壳同步）
  adapter/skills/ponytail-update/SKILL.md
  test/smoke.test.mjs           跑真实 provider，抄内核的 validateCandidate 校验
  package.json / LICENSE / README.md
```

`host/skills.js` 与 `index.js` 分开是有原因的：前者**不依赖任何 `@deepseek-ai`
包**（rank 由调用方传入），所以 `npm run check` 里的冒烟测试能 import 它并**真的
把 provider 跑一遍**。第一版把两者写在一起，测试只好读源码文本，于是「`list()`
少填了 source / rank / provider」一路绿灯直到真机启动才炸。
