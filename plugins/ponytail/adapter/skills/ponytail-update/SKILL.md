---
name: ponytail-update
description: 立刻从上游拉取最新版 ponytail 规则与技能并汇报结果。Use when the user asks to update ponytail, refresh it from upstream, check whether a newer version exists, or types "/ponytail-update".
---

# ponytail-update

拉一次上游最新版，并把结果如实告诉用户。

## 做什么

用 shell 工具执行**一条**命令：

```sh
node "<插件目录>/adapter/update.mjs"
```

插件目录就是这份技能所在的位置（`skills/ponytail-update/` 的上一级）。若不确定
路径，用 shell 工具在本插件的物化目录里找 `adapter/update.mjs`（可用
`find` / `ls`，工作台里通常就是当前插件目录）。

## 汇报规则

脚本的三种结果，照原样转述，不要加工成「已完成」：

- 退出码 0 且输出含「已同步到 …」→ 告诉用户版本号，并说明**要重启工作台才生效**
  （换掉的是实例里的技能正文与规则，工作台已经加载过旧的）。
- 输出「同步失败：…」→ **如实说失败**，连同原因。**不要**说成「已是最新」——
  用户是在主动要求更新，谎报成功会让他以为规则已经变了。
- 找不到桥（退出码 2）→ 说明这次工作台不是由桌面壳启动的，请用户在插件面板里
  点「检查更新」。

## 不要做的事

- 不要自己 `npm install` / `git pull` 去替换插件目录。同步由桌面壳做，它才有取源
  校验与代理配置；自己换文件会绕过校验，且工作台未必加载到新内容。
- 不要在没有执行上面那条命令的情况下声称「已经是最新」。
