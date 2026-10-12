#!/usr/bin/env node
// `/ponytail-update` 的执行体：请桌面壳**立刻**拉一次上游最新版，并把结果打出来。
//
// ## 为什么同步在壳那边做，这个脚本只发一个请求
//
// 上游内容是 npm 包，取源、完整性校验（SRI）、解包防路径穿越、系统代理探测这
// 四件事只有桌面壳那一侧有（见 src-tauri/src/plugins/builtin/upstream.rs）。在
// 这里重写一遍等于多一份「下载并信任它」的实现，而 Node 的 `fetch` 还不认
// HTTP(S)_PROXY——本仓踩过的那类坑（reqwest 同款）会在插件里原样重现一次。
//
// ## 为什么是 HTTP 而不是别的通道
//
// 工作台与桌面壳是两个进程，插件在内核里、壳在另一个进程里。壳启动内核时起了
// 一个只绑回环的桥（地址与令牌按「(族, 实例)」一代一换，只在本次内核存活期间
// 有效），这是现成的唯一通道。
//
// 凭据有两条路：环境变量，以及插件物化根下的 `.xlink-bridge.json`。为什么需要
// 第二条，见下面 `readCredentials` 的注释——**env 那条实测在真机上不通**。
//
// ## 输出是给模型读的
//
// 技能会让模型把这里的结果原样转述给用户，所以：成功/失败都要有可读的一行，
// **失败不吞**（静默退出 0 会让模型汇报「已是最新」，而实际上什么都没拉到）。

import { readFileSync, rmSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

/** 凭据文件名：与壳的 `bridge_credential.rs`、插件的 `host/bridge-credential.js` 同值。 */
const CREDENTIAL_FILE = ".xlink-bridge.json";

const url = process.env.DSH_XLINK_BUILTIN_BRIDGE_URL;
const token = process.env.DSH_XLINK_BUILTIN_BRIDGE_TOKEN;
const plugin = process.env.PONYTAIL_PLUGIN_ID ?? "ponytail";

// env 不是可靠的那条路（2026-10-12 真机实测）：壳把凭据注进**内核进程**，而这个
// 脚本是**模型**在工作台里用 shell 工具跑的，那条链看不到内核的 `DSH_*`（内核给
// 子进程的环境会把这些名字剔掉）—— 当时它稳定报「找不到同步桥」，而同一时刻内核
// pid 的环境里两个变量都在、桥也活着。
//
// 所以文件才是主路，而它有两处（**先物化根，后上一级**）：
//   · `<插件根>/.xlink-bridge.json` —— 壳启动内核时写的，0806 权限 0600。权威。
//   · `<插件根>/../.xlink-bridge.json` —— 插件自己写的（`host/bridge-credential.js`）。
//     物化目录被重建时（每次启动内核、面板点「检查更新」都会重建）前者会被带走，
//     这一份不会；跑着还没写凭据那一步的旧壳时，也只有这一份。
//
// **先 env 后文件**：env 命中时不读盘。路径全由本文件自己的位置推出
// （`adapter/../` 与 `adapter/../../`），不依赖任何环境变量去猜 DSH_HOME ——
// 而 env 正是这条路上不可靠的那一环，多穿一层就多一处会失效的假设。
function credentialFiles(here) {
  return [join(here, "..", CREDENTIAL_FILE), join(here, "..", "..", CREDENTIAL_FILE)];
}

function readCredentials() {
  if (url && token) return { url, token, via: "env", path: null };
  for (const path of credentialFiles(dirname(fileURLToPath(import.meta.url)))) {
    try {
      const raw = JSON.parse(readFileSync(path, "utf8"));
      if (typeof raw?.url === "string" && typeof raw?.token === "string" && raw.url && raw.token) {
        return { url: raw.url, token: raw.token, via: "凭据文件", path };
      }
    } catch {
      /* 这一处没有就试下一处；都没有就走下面那句人话 */
    }
  }
  return null;
}

const credentials = readCredentials();

if (credentials === null) {
  console.error(
    "找不到桌面壳的同步桥（环境变量、物化根下与物化根上一级的 .xlink-bridge.json 都没有凭据）。"
      + "这通常意味着工作台是由壳以外的途径启动的；请在插件面板里点「检查更新」。",
  );
  process.exit(2);
}

const endpoint = `${credentials.url}/v1/builtin/sync?plugin=${encodeURIComponent(plugin)}`;

try {
  const response = await fetch(endpoint, {
    headers: { authorization: `Bearer ${credentials.token}` },
    signal: AbortSignal.timeout(180_000),
  });
  const text = await response.text();
  if (!response.ok) {
    // 401 特判：**上一次工作台被强杀**（没走 Drop）时凭据文件会留下，而桥已经
    // 随上一个内核一起没了。通用报错会把它说成「同步失败」，用户会去查网络。
    if (response.status === 401 && credentials.via === "凭据文件") {
      // 把**带着这枚令牌的每一份**都删掉：落点有两处，只删读到的那一份会让下一次
      // 读到另一份同样失效的令牌，用户要多撞一次才看到这句人话。
      for (const path of credentialFiles(dirname(fileURLToPath(import.meta.url)))) {
        try {
          if (JSON.parse(readFileSync(path, "utf8"))?.token === credentials.token) {
            rmSync(path, { force: true });
          }
        } catch {
          /* 没有 / 读不出来 / 删不掉，都不影响结论 */
        }
      }
      console.error(
        "同步桥的凭据已失效（上次工作台没有正常退出，留下了上一代的令牌）。"
          + "已删除那份凭据，请重启工作台后再试一次。",
      );
      process.exit(1);
    }
    console.error(`同步失败（HTTP ${response.status}）：${text}`);
    process.exit(1);
  }
  const result = JSON.parse(text);
  if (result.error) {
    console.error(`同步失败：${result.error}（继续使用现有的 ${result.version || "无"}）`);
    process.exit(1);
  }
  console.log(
    result.version
      ? `已同步到 ${plugin} ${result.version}。重启工作台后生效。`
      : "上游没有可用版本；继续使用现有内容。",
  );
} catch (error) {
  console.error(`同步失败：${String(error?.message ?? error)}`);
  process.exit(1);
}
