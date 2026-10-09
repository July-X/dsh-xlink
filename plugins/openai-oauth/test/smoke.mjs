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
const stubRequests = [];
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
  if (req.url === "/v1/responses" && req.method === "POST") {
    let raw = "";
    req.on("data", (chunk) => {
      raw += chunk;
    });
    req.on("end", () => {
      stubRequests.push({ url: req.url, method: req.method, body: raw });
      // 状态机：第 1 次请求发 function_call（探针工具，内核 agent loop 会
      // 执行并回传结果），第 2 次起回最终文本——验证 P4 的工具往返闭环。
      const roundtrip = stubRequests.filter((r) => r.url === "/v1/responses").length;
      // 模型名含 "error" → 服务端失败终止（§8.3 错误分类路径）。
      const wantError = raw.includes('"gpt-stub-error"');
      if (wantError) {
        const failEvents = [
          '{"type":"response.failed","response":{"error":{"code":"insufficient_quota","message":"You have exceeded your quota"}}}',
        ];
        const failTerminal = '{"type":"bridge.terminal","status":"failed","replay":null,"detail":"You have exceeded your quota"}';
        res.writeHead(200, { "content-type": "application/x-ndjson" });
        for (const event of failEvents) res.write(event + "\n");
        res.write(failTerminal + "\n");
        res.end();
        return;
      }
      const events =
        roundtrip <= 1
          ? [
              '{"type":"response.output_item.added","item":{"type":"function_call","id":"call-probe-1","name":"xlink_probe","arguments":""}}',
              '{"type":"response.function_call_arguments.delta","item_id":"call-probe-1","name":"xlink_probe","delta":"{}"}',
              '{"type":"response.completed","response":{"id":"resp-1","output":[{"type":"function_call","id":"call-probe-1","name":"xlink_probe","arguments":"{}"}]}}',
            ]
          : [
              '{"type":"response.reasoning_summary_text.delta","delta":"思考：用户要一个 pong"}',
              '{"type":"response.output_text.delta","delta":"pong-from-stub"}',
              '{"type":"response.completed","response":{"id":"resp-2","usage":{"input_tokens":3,"output_tokens":2,"total_tokens":5},"output":[]}}',
            ];
      const terminal =
        roundtrip <= 1
          ? '{"type":"bridge.terminal","status":"completed","replay":{"response":{"id":"resp-1","output":[{"type":"function_call","id":"call-probe-1","name":"xlink_probe","arguments":"{}"}]}},"detail":null}'
          : '{"type":"bridge.terminal","status":"completed","replay":{"response":{"id":"resp-2","usage":{"input_tokens":3,"output_tokens":2,"total_tokens":5},"output":[]}},"detail":null}';
      res.writeHead(200, { "content-type": "application/x-ndjson" });
      for (const event of events) res.write(event + "\n");
      res.write(terminal + "\n");
      res.end();
    });
    return;
  }
  if (req.url === "/v1/models") {
    stubRequests.push({ url: req.url, method: req.method, body: "" });
    res.writeHead(200, { "content-type": "application/json" }).end(JSON.stringify({
      revision: "smoke-rev-1",
      models: [
        { id: "gpt-stub-x", name: "GPT Stub X", contextWindow: 16384,
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
child.on("error", (error) => { kernelErr += `spawn error: ${error}`; });
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

// ── Phase C：headless 一次性会话（真实内核 agent loop → 适配器 → 桥接 →
// 桩上游）。这是 P4 的离线最强验收：整条生成链路在真实内核里跑通。 ──
const headlessProfileDir = join(home, "profiles", "headless");
mkdirSync(join(headlessProfileDir, "workspace"), { recursive: true });
writeFileSync(
  join(headlessProfileDir, "package.json"),
  `${JSON.stringify({
    name: "dsh-profile-headless",
    private: true,
    dependencies: {},
    dsh: { profile: { bundles: ["@deepseek-ai/dsh-base", "@deepseek-ai/dsh-headless"] } },
  }, null, 2)}\n`,
);
writeFileSync(
  join(headlessProfileDir, "pnpm-workspace.yaml"),
  "packages:\n  - .\n\nnodeLinker: hoisted\nautoInstallPeers: false\nminimumReleaseAge: 0\n",
);
writeFileSync(
  join(headlessProfileDir, "cordis.patch.yml"),
  [
    // 会话默认模型指向桩提供方（base 行按 id 覆写 config）。
    "- id: agent-default-model",
    "  config:",
    "    provider: xlink-openai-chatgpt",
    "    model: gpt-stub-x",
    "    reasoningEffort: low",
    // 本插件的接线行（与 web profile 同形状）。
    "- insert:",
    "    - id: xlink-openai-oauth",
    `      name: '${entryRel.replace("profiles/web/", "profiles/headless/").split("/").join("/")}'`,
    "",
  ].join("\n"),
);
const headless = spawn(
  process.execPath,
  [bin, "--profile", "headless", "Say pong"],
  {
    cwd: workspace,
    env: {
      ...process.env,
      DSH_HOME: home,
      DSH_PROFILE: "headless",
      DSH_XLINK_OPENAI_BRIDGE_URL: `http://127.0.0.1:${bridgePort}`,
      DSH_XLINK_OPENAI_BRIDGE_TOKEN: BRIDGE_TOKEN,
    },
    stdio: ["ignore", "pipe", "pipe"],
  },
);
headless.on("error", (error) => { headlessErr += `spawn error: ${error}`; });
let headlessOut = "";
headless.stdout.on("data", (c) => { headlessOut += c; });
let headlessErr = "";
headless.stderr.on("data", (c) => { headlessErr += c; });
const headlessCode = await new Promise((done) => {
  const timer = setTimeout(() => { headless.kill("SIGKILL"); done("timeout"); }, 45000);
  headless.on("exit", (code) => { clearTimeout(timer); done(code); });
});
check(
  "reasoning 增量进内核会话（stderr 的 reasoning 块）",
  headlessErr.includes("reasoning:") && headlessErr.includes("思考：用户要一个 pong"),
  headlessErr.slice(0, 120),
);
check(
  "headless 会话产出桩上游文本（经工具往返后的最终回答）",
  headlessCode === 0 && headlessOut.includes("pong-from-stub"),
  `exit=${headlessCode} stdout=${JSON.stringify(headlessOut.slice(0, 200))} stderr=${JSON.stringify(headlessErr.slice(0, 300))}`,
);
const responsesHit = stubRequests.find((r) => r.url === "/v1/responses");
const responsesAll = stubRequests.filter((r) => r.url === "/v1/responses");
const roundtripHit = responsesAll[1];
check(
  "工具往返闭环（第 2 次请求携带 function_call_output 与原 call_id）",
  responsesAll.length >= 2 &&
    roundtripHit !== undefined &&
    roundtripHit.body.includes("function_call_output") &&
    roundtripHit.body.includes("call-probe-1"),
  `hits=${responsesAll.length}`,
);
check(
  "强度端到端（agentDefaultModel.reasoningEffort → payload.reasoning.effort）",
  responsesHit !== undefined && responsesHit.body.includes('"reasoning":{"effort":"low"}'),
  JSON.stringify(responsesHit ?? {}).slice(0, 260),
);
check(
  "推理信封合规（白名单内字段；store/stream 由桥接固定参数写入）",
  responsesHit !== undefined &&
    responsesHit.body.includes("gpt-stub-x") &&
    responsesHit.body.includes("catalogRevision") &&
    responsesHit.body.includes("instructions") &&
    responsesHit.body.includes("input") &&
    !responsesHit.body.includes('"store"') &&
    !responsesHit.body.includes('"stream"') &&
    !responsesHit.body.includes("temperature") &&
    !responsesHit.body.includes("max_output_tokens"),
  JSON.stringify(responsesHit ?? {}).slice(0, 260),
);

await cleanup();
if (failures.length > 0) {
  console.error(`\n冒烟失败：${failures.join("；")}`);
  process.exit(1);
}
console.log(`\n冒烟通过（dsh ${dshVersion} / cordis ${cordisVersion} / 指纹 ${fingerprint}）`);
