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
// 一个只绑回环的桥（地址与令牌在 env 里），这是现成的唯一通道。令牌按
// 「(族, 实例)」一代一换，只在本次内核存活期间有效。
//
// ## 输出是给模型读的
//
// 技能会让模型把这里的结果原样转述给用户，所以：成功/失败都要有可读的一行，
// **失败不吞**（静默退出 0 会让模型汇报「已是最新」，而实际上什么都没拉到）。

const url = process.env.DSH_XLINK_BUILTIN_BRIDGE_URL;
const token = process.env.DSH_XLINK_BUILTIN_BRIDGE_TOKEN;
const plugin = process.env.PONYTAIL_PLUGIN_ID ?? "ponytail";

if (!url || !token) {
  console.error(
    "找不到桌面壳的同步桥（DSH_XLINK_BUILTIN_BRIDGE_URL / _TOKEN 未设置）。"
      + "这通常意味着工作台是由壳以外的途径启动的；请在插件面板里点「检查更新」。",
  );
  process.exit(2);
}

const endpoint = `${url}/v1/builtin/sync?plugin=${encodeURIComponent(plugin)}`;

try {
  const response = await fetch(endpoint, {
    headers: { authorization: `Bearer ${token}` },
    signal: AbortSignal.timeout(180_000),
  });
  const text = await response.text();
  if (!response.ok) {
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
