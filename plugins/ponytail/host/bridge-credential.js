/**
 * 桥凭据落盘：把内核环境里的地址与令牌写一份到**物化目录的上一级**，让
 * `/ponytail-update` 那个脚本（跑在模型的 shell 里，看不到内核的环境）能找到它。
 *
 * ## 为什么插件自己也要写一份
 *
 * 壳启动内核时把凭据注进**内核进程的环境**，同时在物化根下写一份
 * `.xlink-bridge.json`（`src-tauri/src/plugins/builtin/bridge_credential.rs`）。
 * 那份文件本身是对的，但有两条路走不到：
 *
 * · **跑着旧壳**：环境里有凭据、盘上没有文件。2026-10-12 用户实测：桥活着、
 *   两个变量都在内核环境里、物化根下却没有 `.xlink-bridge.json`，于是
 *   `/ponytail-update` 恒报退出码 2 —— 脚本只剩文件那条路，而文件不存在。
 * · **物化目录被重建**：我们改了 `skills/`，壳的物化摘要必然对不上，所以每次
 *   启动内核、面板每次「检查更新」都会 `remove_dir_all` 重建整棵插件目录；
 *   壳写的那份文件当场消失，而插件**不会**跟着重新加载。写在上一级
 *   （`<fingerprint>/`）就不会被那次重建带走。
 *
 * 插件与内核同进程，`process.env` 里本来就有那两个变量，写一份即可。
 *
 * ## 威胁模型没有变化
 *
 * 令牌是壳写给这个用户的（0600），而它此刻就躺在本进程的环境里：这次只是换个
 * 地方放，读得到它的人还是同一个用户。前端 `adapter/update.mjs` 仍然
 * **env 优先、文件兜底**，路径与这里逐字对应。
 *
 * 先写临时文件再 rename、权限 0600：半截 JSON 会让脚本报「凭据格式不对」，
 * 比真实原因更难查；权限松了则等于给同机别的账号看（Windows 上 `mode` 不生效，
 * 与壳那边的 `#[cfg(unix)]` 是同一条限制）。
 *
 * 本文件不 import 任何 `@deepseek-ai/*` 包，所以 `npm run check` 能直接跑它。
 *
 * @module xlink-ponytail/bridge-credential
 */
import { readFileSync, renameSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";

/** 文件名与两个环境变量名：与壳的 `bridge_credential.rs` / `bridge.rs` 逐字同值。 */
export const CREDENTIAL_FILE = ".xlink-bridge.json";
export const URL_ENV = "DSH_XLINK_BUILTIN_BRIDGE_URL";
export const TOKEN_ENV = "DSH_XLINK_BUILTIN_BRIDGE_TOKEN";

/**
 * 凭据的落点：物化根（`…/<fingerprint>/compat-a`）的**上一级**。
 *
 * 不写进物化根：`materialize()` 会在重建时 `remove_dir_all(target)`，而重建
 * 恰恰发生在我们加载之后（面板「检查更新」）—— 写进去等于写完就等着被删。
 *
 * @param {string} root 插件根（物化目录）。
 * @returns {string}
 */
export function credentialPath(root) {
  return join(root, "..", CREDENTIAL_FILE);
}

/** 读盘上那份；读不出来、或形状不对，一律当「没有」。 */
function readCredential(path) {
  try {
    const raw = JSON.parse(readFileSync(path, "utf8"));
    if (typeof raw?.url !== "string" || typeof raw?.token !== "string") return null;
    return raw.token.length > 0 && raw.url.length > 0 ? raw : null;
  } catch {
    return null;
  }
}

/**
 * 把环境里的凭据写一份到盘上。
 *
 * 盘上已经是**同一代**凭据时不动它（不重写、不刷新时间戳）：每次内核启动都
 * 写一遍没有意义，而「同一个令牌反复落盘」只增加被读到的窗口。
 *
 * **不抛异常**：写不下去只是 `/ponytail-update` 少一条路，不该让技能注册跟着
 * 失败。所以失败以 `failed` 标出来，由调用方决定记一行日志。
 *
 * @param {string} root 插件根（物化目录）。
 * @param {Record<string, string|undefined>} [env] 默认 `process.env`，测试注入用。
 * @returns {{written: boolean, failed?: boolean, path: string|null, reason: string}}
 */
export function writeBridgeCredential(root, env = process.env) {
  const url = env?.[URL_ENV];
  const token = env?.[TOKEN_ENV];
  // 没有凭据是**正常**的：不是由桌面壳启动的内核（或这一代壳没起桥）就没有这两个
  // 变量，此时没什么可写，也不该报错。
  if (!url || !token) return { written: false, path: null, reason: "内核环境里没有桥凭据" };

  const path = credentialPath(root);
  const existing = readCredential(path);
  if (existing !== null && existing.url === url && existing.token === token) {
    return { written: false, path, reason: "盘上那份就是这一代凭据" };
  }
  try {
    const tmp = `${path}.tmp-${process.pid}`;
    writeFileSync(tmp, `${JSON.stringify({ url, token, pid: process.pid }, null, 2)}\n`, {
      encoding: "utf8",
      mode: 0o600,
    });
    renameSync(tmp, path);
    return { written: true, path, reason: `桥凭据已落盘（${path}）` };
  } catch (error) {
    return {
      written: false,
      failed: true,
      path,
      reason: `写桥凭据失败（${path}）：${String(error?.message ?? error)}`,
    };
  }
}

/**
 * 收走这一代凭据（内核正常停止时）。
 *
 * **只删自己这一代**：文件里的令牌与本次环境对不上，说明它是更新的一代
 * （换了壳、或内核刚重启），留着。
 *
 * @param {string} root 插件根（物化目录）。
 * @param {Record<string, string|undefined>} [env] 默认 `process.env`。
 * @returns {boolean} 真的删掉了才为 `true`。
 */
export function removeBridgeCredential(root, env = process.env) {
  const token = env?.[TOKEN_ENV];
  const path = credentialPath(root);
  const existing = readCredential(path);
  if (token === undefined || existing === null || existing.token !== token) return false;
  try {
    rmSync(path, { force: true });
    return true;
  } catch {
    return false;
  }
}
