# Ponytail（内置插件）

把 [Ponytail](https://github.com/DietrichGebert/ponytail) 随 dsh-xlink 交付：
启用后，工作台里多六个技能——写代码时先找现成的、再考虑标准库，最后才自己写；
另有评审、全仓体检、欠账清单三个命令型技能。

## 这是什么，不是什么

**是**：六个 `SKILL.md`。`host/index.js` 读它们、剥 frontmatter、注册进内核技能
表（`ctx.skills.registerProvider`，rank = `BUNDLED_SKILL_RANK` = 600）。没有网络
请求、没有凭据、没有账号，没有工作台内的设置卡。

**不是**上游那套完整的宿主集成。上游的档位切换（`/ponytail lite|full|ultra`）
与会话级常驻靠的是 Claude Code / Cursor 的**生命周期钩子 + 宿主状态**
（`hooks/*.js` 里的 mode tracker、instructions 注入），dsh 侧没有对应面。
我们给的是**规则集 + 六个按需调用的技能**，不含那套状态机——面板的说明里也这么
写，不让宣传口径大于实际能力。

## 为什么默认关闭

技能是指令文本、会整体进入模型上下文（见
[docs/features/extensions/skill-management.md](../../../docs/features/extensions/skill-management.md)
「安全边界」一节）。这与 OpenAI OAuth 那类内置插件有本质区别：后者不登录就毫无
副作用，而 Ponytail 是**第一条消息就改变行为**。因此它默认关闭，由用户决定何时
打开。

## 用户装了同名技能会怎样

rank 600 低于壳管理的活动视图（300）与内核默认根（400/500），同名时**用户自己
装的赢**。所以「随应用交付」不等于「强制生效」——用户改主意不需要先卸载应用。

## 出处与更新

| | |
| --- | --- |
| 上游 | <https://github.com/DietrichGebert/ponytail> |
| 版本 | 5.1.0 |
| 上游 tree | `9b58c1ffb790c075ca32e70a89cf1d80588f4abf` |
| 许可 | MIT（`LICENSE`，逐字照抄） |
| 归入本仓 | 2026-10-12 |

`skills/` 下的六个文件是**上游原文的逐字副本**，未做任何修改——包括 `ponytail-help`
里那些讲 Claude Code / Codex 的段落（那是上游给别的宿主写的说明，对本插件不适用，
但改动 vendored 内容会让「我们分发的是哪一版」这件事无从核对）。面向 dsh 的
差异写在上面这段文档与插件面板的说明里，不动原文。

**更新是一次人工动作**：改版本号 → 重新取上游六个文件 → 跑 `npm run check`。
`package.json` 的 `dsh.source` 记着上游 tree sha，核对的是「我们发的是哪一版」。

技能正文是英文原文（上游没有中文技能正文），面板说明是中文。