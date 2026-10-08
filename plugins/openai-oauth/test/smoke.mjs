/**
 * 端到端冒烟：把本包按 P1 配方物化进临时 DSH home，起桩桥接，拉起真实
 * 内核，断言接线链路（目录行 / settingsNs / 握手 / 目录 / 解析 / boot 图）。
 *
 * 用法：node test/smoke.mjs --kernel <内核安装根> [--port 3130] [--keep]
 *
 * 纪律（AGENTS.md）：一切写入只落在 os.tmpdir() 的临时目录；不触碰用户
 * 数据目录与内核安装树（对内核树只读：物化目录里的 peer 是指向它的
 * 符号链接，不写入）。退出码 0 = 全部断言通过。
 */
import { spawn } from "node:child_process";
import { cpSync, mkdirSync, mkdtempSync, readFileSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { createServer } from "node:http";
import { tmpdir } from "node:os";
import { join, relative } from "node:path";
import { setTimeout as delay } from "node:timers/promises";

const args = process.argv.slice(2);
const flag = (name) => {
  const i = args.indexOf(`--${name}`);
  return i >= 0 ? args[i + 1] : undefined;
};
const kernelRoot = flag("kernel");
const keep = args.includes("--keep");
if (!kernelRoot) {
  console.error("用法: node test/smoke.mjs --kernel <内核安装根> [--port 3130] [--keep]");
  process.exit(2);
}

const here = new URL("..", import.meta.url).pathname; // plugins/openai-oauth/
const failures = [];
const check = (label, ok, detail = "") => {
  console.log(`${ok ? "✅" : "❌"} ${label}${detail ? `（${detail}）` : ""}`);
  if (!ok) failures.push(label);
};

// ── 内核指纹（dsh + cordis 版本；正式指纹由 Rust 接线器负责，此处同形） ──
const pkgVersion = (root, name) => {
  try {
    return JSON.parse(readFileSync(join(root, "node_modules", "@deepseek-ai", name, "package.json"), "utf8")).version;
  } catch {
    return "unknown";
  }
};
const dshVersion = pkgVersion(kernelRoot, "dsh");
const cordisVersion = pkgVersion(kernelRoot, "cordis");
const fingerprint = `dsh-${dshVersion}-cordis-${cordisVersion}`.replace(/[^\w.-]+/g, "-");

// ── 临时 home 与物化 ──
const scratch = mkdtempSync(join(tmpdir(), "oop-smoke-"));
const home = join(scratch, "home");
const pluginDir = join(home, "extensions", "builtin", "xlink-openai-oauth", "0.1.0", fingerprint, "compat-a");
const profileDir = join(home, "profiles", "web");
const workspace = join(scratch, "workspace");
cpSync(join(here, "host"), join(pluginDir, "host"), { recursive: true });
cpSync(join(here, "client"), join(pluginDir, "client"), { recursive: true });
cpSync(join(here, "locales"), join(pluginDir, "locales"), { recursive: true });
cpSync(join(here, "package.json"), join(pluginDir, "package.json"));
mkdirSync(join(pluginDir, "node_modules", "@deepseek-ai"), { recursive: true });
mkdirSync(profileDir, { recursive: true });
symlinkSync(
  join(kernelRoot, "node_modules", "@deepseek-ai", "dsh-llm"),
  join(pluginDir, "node_modules", "@deepseek-ai", "dsh-llm"),
);
writeFileSync(join(profileDir, "package.json"), `${JSON.stringify({
  name: "dsh-profile-web",
  private: true,
  dependencies: {},
  dsh: { profile: { bundles: ["@deepseek-ai/dsh-base", "@deepseek-ai/dsh-web-app"] } },
}, null, 2)}\n`);
writeFileSync(join(profileDir, "pnpm-workspace.yaml"),
  "packages:\n  - .\n\nnodeLinker: hoisted\nautoInstallPeers: false\nminimumReleaseAge: 0\n");
// 接线行：name 必须直指入口 JS（ESM 无目录导入）；相对路径锚定在 patch 所在目录。
const entryRel = relative(profileDir, join(pluginDir, "host", "index.js")).split("\\").join("/");
writeFileSync(join(profileDir, "cordis.patch.yml"),
  `- insert:\n    - id: xlink-openai-oauth\n      name: '${entryRel}'\n`);
mkdirSync(workspace, { recursive: true });

// ── 桩桥接：握手 + 固定目录，校验 Bearer ──
const BRIDGE_TOKEN = `smoke-${Date.now()}-${Math.random().toString(36).slice(2)}`;
const bridgeHits = [];
const bridgeServer = createServer((req, res) => {
  bridgeHits.push(req.url);
  if (req.headers.authorization !== `Bearer ${BRIDGE_TOKEN}`) {
    res.writeHead(401).end();
    return;
  }
  if (req.url === "/v1/handshake") {
    res.writeHead(200, { "content-type": "application/json" })
      .end(JSON.stringify({ protocol: 1, pluginVersion: "stub", service: "xlink-openai-oauth" }));
    return;
  }
  if (req.url === "/v1/models") {
    res.writeHead(200, { "content-type": "application/json" }).end(JSON.stringify({
      revision: "smoke-rev-1",
      models: [
        { id: "gpt-stub-mini", name: "GPT Stub Mini", contextWindow: 16384,
          efforts: [{ id: "low", name: "Low" }, { id: "high", name: "High" }] },
        { id: "gpt-stub-max", name: "GPT Stub Max" },
      ],
    }));
    return;
  }
  res.writeHead(404).end();
});
const bridgePort = await new Promise((resolvePort) => {
  bridgeServer.listen(0, "127.0.0.1", () => resolvePort(bridgeServer.address().port));
});

const wantedPort = flag("port");
const kernelPort = wantedPort !== undefined ? Number(wantedPort) : await new Promise((resolvePort) => {
  const probe = createServer();
  probe.listen(0, "127.0.0.1", () => { const p = probe.address().port; probe.close(() => resolvePort(p)); });
});
const markerPath = join(scratch, "marker.json");

// ── 拉起内核 ──
const bin = join(kernelRoot, "node_modules", "@deepseek-ai", "dsh", "lib", "bin.js");
const child = spawn(process.execPath, [bin, "web", "--no-open", "--port", String(kernelPort)], {
  cwd: workspace,
  env: {
    ...process.env,
    DSH_HOME: home,
    DSH_PROFILE: "web",
    DSH_XLINK_OPENAI_BRIDGE_URL: `http://127.0.0.1:${bridgePort}`,
    DSH_XLINK_OPENAI_BRIDGE_TOKEN: BRIDGE_TOKEN,
    DSH_XLINK_OPENAI_TEST_MARKER: markerPath,
  },
  stdio: ["ignore", "pipe", "pipe"],
});
let kernelOut = "";
let kernelErr = "";
child.stdout.on("data", (chunk) => { kernelOut += chunk; });
child.stderr.on("data", (chunk) => { kernelErr += chunk; });

let cleaned = false;
const cleanup = async () => {
  if (cleaned) return;
  cleaned = true;
  child.kill("SIGTERM");
  await new Promise((done) => {
    child.on("exit", done);
    const timer = setTimeout(done, 3000);
    timer.unref?.();
  });
  bridgeServer.close();
  if (!keep) rmSync(scratch, { recursive: true, force: true });
  else console.log(`保留现场：${scratch}`);
};
process.on("SIGINT", async () => { await cleanup(); process.exit(130); });

// ── 等待标记并断言 ──
let marker;
for (let i = 0; i < 40 && marker === undefined; i++) {
  await delay(500);
  try { marker = JSON.parse(readFileSync(markerPath, "utf8")); } catch { /* 尚未生成 */ }
}
if (marker === undefined) {
  console.error(`内核未在 20s 内写出标记。stderr 尾部：\n${kernelErr.slice(-1500)}`);
  await cleanup();
  process.exit(1);
}
check("目录行注册（settingsNs 取接线行 id）", marker.settingsNs === "xlink-openai-oauth", marker.settingsNs);
check("桥接握手成功", marker.bridgeHandshakeOk === true, marker.bridgeError?.message ?? "");
check("桩桥接收到握手请求", bridgeHits.includes("/v1/handshake"), bridgeHits.join(","));
check("目录经真实适配器拉取", marker.catalog?.count === 2, JSON.stringify(marker.catalog));
check("能力解析（contextWindow/efforts）",
  marker.catalog?.resolvedContext === 16384 && JSON.stringify(marker.catalog?.resolvedEfforts) === '["low","high"]');
check("注册句柄带 replace", marker.replaceIsFunction === true);
check("内核无插件加载告警", !/did not activate|failed to import/.test(kernelErr));

// ── boot 图含 client 条目（token → cookie → 首页 HTML） ──
let bootOk = false;
for (let i = 0; i < 20 && !kernelOut.includes("token="); i++) await delay(500);
const token = /token=([^\s]+)/.exec(kernelOut)?.[1];
if (token !== undefined) {
  try {
    const first = await fetch(`http://127.0.0.1:${kernelPort}/?token=${token}`, { redirect: "manual" });
    const cookies = (first.headers.getSetCookie?.() ?? []).map((c) => c.split(";")[0]);
    const headers = cookies.length > 0 ? { cookie: cookies.join("; ") } : {};
    const html = await (await fetch(`http://127.0.0.1:${kernelPort}/`, { headers })).text();
    bootOk = html.includes("xlink-openai-oauth/client.js");
  } catch { /* 内核可能在退出中 */ }
}
check("boot 模块图包含 client 条目", bootOk);

await cleanup();
if (failures.length > 0) {
  console.error(`\n冒烟失败：${failures.join("；")}`);
  process.exit(1);
}
console.log(`\n冒烟通过（dsh ${dshVersion} / cordis ${cordisVersion} / 指纹 ${fingerprint}）`);
