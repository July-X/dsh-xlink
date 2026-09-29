# dsh-scope-suspend — 作用域短暂缺席时挂起，不再把整个工作台带走

## 一句话

内核渲染器在模块图重组的那一瞬间会丢掉已安装的作用域 adapter，`ScopeProvider` 原本对这种缺席**直接抛错**，于是整个工作台黑掉、必须重开窗口。这个补丁让它**挂起等待**而不是抛错。

## 现象（2026-09-29 本机实测）

工作台抛 `scope 'session-maybe' rendered without an installed adapter` 后整页黑掉，
仅剩管理面板。事故面板在引入 `kernel-boot` 归因之前，把它显示成
「前端 bundle 异常（未定位到包名）」并建议**停用第三方插件** —— 那个建议既治不了，
又会把无辜插件写进嫌疑。

## 复现步骤

在 release 壳的工作台正常运行期间，删除一个**没有被使用**的同级内核目录：

```sh
# 记录基线
curl -s http://127.0.0.1:3090/ | grep -o '__DSH_BOOT__'          # 200，boot rev = d46638d97b76
# 基线无 last-incident.json
Remove-Item -Recurse -Force <xlink_home>\dsh\desktop\kernels\0.1.7-rc.1
```

结果：

```
基线          index=200  内核活着  无事故文件
删除 t+00s    index=200  内核活着  无事故文件
删除 t+006s   index=200  内核活着  ← 事故文件出现
删除 t+034s   删除完成（34.2 s / 25643 个文件），index 始终 200，内核始终活着
```

`boot rev` 从 `d46638d97b76` 变成 `b3516d7cee6d`，`entries` 仍是 67。

## 根因

1. `remove_dir_all` 删掉 25643 个文件 —— 文件系统事件风暴。
2. 内核的文件监视器把这场风暴当成模块图变更并**重组**，
   **即使变化发生在一个它根本不服务的目录里**。
   （bundle 字节其实早已 `readFileSync` 进内存缓存，HTTP 全程 200 —— 所以这不是
   「服务被打断」，而是「图被换掉了」。）
3. 页面看到图变了，`ClientEntries.reconcile()` 换掉一批模块行，
   `dsh-client-ui-session` 被销毁。
4. `slots.installScope` 把作用域装在一个 `ctx.effect` 里，**disposer 会把它删掉**：

   ```js
   installScope(scope, adapter) {
       this.ctx.effect(() => {
           this._scopes.set(scope, adapter);
           this.publishScopeRevision();
           return () => {                          // ← disposer
               if (this._scopes.get(scope) === adapter) {
                   this._scopes.delete(scope);
                   this.publishScopeRevision();
               }
           };
       }, `slots.installScope(${JSON.stringify(scope)})`);
   }
   ```

5. `ScopeProvider` 订阅着 `scopeRevision`，于是只要在「已删除、尚未重装」的窗口里
   重渲染一次，就抛错把整页带走。

## 补丁做什么

把抛错换成挂起：

```diff
- if (adapter === void 0) throw new SlotAssemblyError(`scope '${scope}' rendered without an installed adapter`);
+ if (adapter === void 0) return null;
```

**为什么 `return null` 就够了**：`ScopeProvider` 上一行已经
`observableHook(host.scopeRevision)((value) => value)`，而 `publishScopeRevision()`
在安装与移除时都会调用。也就是说 adapter 装回去的那一刻，本组件**本来就会自动
重渲染**并把子节点补上。抛错是这个组件自己**已经具备恢复能力却拒绝等待**。

**为什么不挂起（throw promise）**：那需要上方有 `Suspense` 边界，而这里没有；
没有边界的挂起会一路冒到根，效果和抛错一样糟。

**长期空白怎么办**：万一作用域真的永远装不上，页面会停在空白 —— 这比死掉好，
因为壳的 `harness-health.js` 已经在跑空白探针（5 s / 9 s 两次检查），
会照常上报 `blank` 事故并弹出面板。**空白可被检测，死页不能。**

## 适用范围

| 内核 | 目标文件 SHA-256 | 结论 |
| --- | --- | --- |
| 0.1.7-rc.2 | `d92aaa305d56631110f46d7b9cd3b21547eaf2cb0c5ed34055391e0a46d9e2ce` | 逐字节命中 |
| 0.2.0-rc.1 | `d92aaa305d56631110f46d7b9cd3b21547eaf2cb0c5ed34055391e0a46d9e2ce` | 逐字节命中（同一份文件） |

`search` 串在两份文件里**各只出现一次**（已核对），`replace` 是全文替换，
不会误伤别处。`required: true` —— 将来内核改了实现导致搜索串不命中时，
补丁会中止并报「内核版本可能已升级」，而不是默默打歪。

## 与壳侧改动的关系

本补丁是**根治的一半**，另一半在壳里（`0.3.5-rc.3`）：

- 壳侧堵住了两个入口：工作台运行期间禁止安装 / 删除内核版本
  （「装一个新内核」和「删一个旧内核」是同一个 bug 的两侧）。
- 壳侧还有一次性自愈兜底：本补丁**没打上**、或者被别的触发路径打中的时候，
  页面会自己重载一次；额度用掉后再撞上才弹面板。

**尚未根治的一半**（本补丁也不解决）：内核的文件监视器**在无关目录发生大规模变化时
仍然会重组模块图**。正确做法是把监视根收窄到真正会热更的路径（profile 的
`node_modules` 等），而不是整个数据目录。补丁只保证「重组了也不会死人」。

## 出处与可逆性

- 补丁由 dsh-xlink 随包分发，用户在「设置 → 内核补丁」页自主应用，默认不生效。
- 应用前自动备份原文件，撤销时从备份还原；备份丢失时以内容哈希兜底。
- 应用要求工作台已停止（与「切换内核版本」同一规则），不会写入运行中的内核。
- 本补丁不新增文件，只对一行做精确替换。

## 验证记录

| 项 | 结果 |
| --- | --- |
| 复现 | 删除同级内核目录后 6 s 内必现（`last-incident.json` 出现同一条错误） |
| 根因定位 | 读 `dsh-client-ui-renderer/lib/client.js:1466` 的 `installScope` disposer + `:288` 的 `ScopeProvider` |
| 目标串唯一性 | 两份内核文件中各出现 1 次 |
| 版本适用 | 0.1.7-rc.2 / 0.2.0-rc.1 逐字节相同 |
| 补丁后行为 | 作用域缺席时子节点暂不渲染，`scopeRevision` 递增后自动补上；不抛错 |
