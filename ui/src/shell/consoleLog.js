// 页面控制台的环形缓冲：agent 调试主壳 WebUI 时唯一的 console 通道。
//
// ## 为什么需要它
//
// 内嵌 WebDriver（tauri-plugin-wdio-webdriver，embedded provider）上没有
// console 日志端点：`browser.getLogs` 随 W3C 规范移除，在该 provider 上实测
// `TypeError: browser.getLogs is not a function`。而壳里恰恰有大量诊断信息
// **只进 console**：`errors.js` 的渲染错误留档、`main.js` 的 Vue warnHandler、
// bridge 的失败路径。没有这条通道，agent 只能看见「面板渲染出来了」，看不见
// 「面板在 console 里喊了什么」。
//
// 做法：入口最早处把 console 的常用方法包一层，按序记进定长环形缓冲，挂在
// `window.__DSH_CONSOLE__` 上；agent 用一次
// `browser.execute(() => window.__DSH_CONSOLE__.dump())` 整段取回。
//
// 三条纪律：
//  1. **只旁观不吞**：包装后必须原样调用原方法、原参数——这条通道坏掉的
//     静默形态就是「console 不再出字」，那比没有通道更糟。
//  2. **内存有界**：定长（默认 500 条），满了丢最旧并计数（`dropped`）；
//     连续重复折叠成一条带 `count`——渲染循环里刷屏的错误一行就能看懂量级。
//  3. **记录绝不抛错**：超长 payload 截断，循环引用 stringify 失败退回
//     `String()`。记录日志这件事本身不允许成为新的故障源。
//
// 边界（如实记）：挂在入口模块最早处，**早于组件挂载**，但晚于本模块之前
// 的模块求值——模块加载期的报错进不来；页面一刷新缓冲即清零（SPA 正常
// 运行不刷新，够用）。持久化到 shell_events 是另一件事，本模块刻意不做。

/** 默认容量。500 条对「一次会话里的诊断回看」足够，内存上限可预算。 */
export const CONSOLE_CAP_DEFAULT = 500;

/** 单条消息的最大字符数。更长的 payload 对定位问题没有增量价值。 */
export const ENTRY_MAX_CHARS = 2000;

/** 安全地把任意值变成有界的字符串：循环引用、Symbol、抛错的 toJSON 都不炸。 */
export function formatConsoleValue(value) {
  if (typeof value === 'string') return value.slice(0, ENTRY_MAX_CHARS);
  // Error 必须单独走 name+message：`JSON.stringify(new Error('x'))` 是 `'{}'`
  // ——message/stack 不可枚举，直接 stringify 会把最有诊断价值的那截吞掉
  // （e2e 的未处理拒绝用例真抓到过：canary 文本消失，只剩一个空壳）。
  if (value instanceof Error) {
    return `${value.name || 'Error'}: ${value.message}`.slice(0, ENTRY_MAX_CHARS);
  }
  try {
    return JSON.stringify(value)?.slice(0, ENTRY_MAX_CHARS) ?? String(value);
  } catch {
    try {
      return String(value).slice(0, ENTRY_MAX_CHARS);
    } catch {
      return '[不可字符串化]';
    }
  }
}

/** 一组 console 实参 → 一条带空格分隔的消息。 */
export function formatConsoleArgs(args) {
  return (args || [])
    .map((value) => formatConsoleValue(value))
    .join(' ')
    .slice(0, ENTRY_MAX_CHARS);
}

/**
 * 纯逻辑的记录器：条目进环形缓冲、重复折叠、溢出计数。
 * 与 console / window 解耦，node 单测直接驱动。
 */
export function createConsoleRecorder({ cap = CONSOLE_CAP_DEFAULT, now = () => Date.now() } = {}) {
  const items = [];
  let dropped = 0;

  return {
    /** 直接读也行；`dump()` 给的是副本，外部改不动内部状态。 */
    items,
    /** 因缓冲满而被丢弃的条数。非 0 时 agent 应知道看到的是「最近 N 条」。 */
    get dropped() {
      return dropped;
    },
    record(level, message, extra) {
      const last = items[items.length - 1];
      if (last && last.level === level && last.message === message) {
        last.count += 1;
        last.t = now();
        return;
      }
      if (items.length >= cap) {
        items.shift();
        dropped += 1;
      }
      items.push({ t: now(), level, message, count: 1, ...extra });
    },
    dump() {
      return { cap, dropped, items: items.map((item) => ({ ...item })) };
    },
    clear() {
      items.length = 0;
      dropped = 0;
    },
  };
}

/**
 * 把记录器装到 `target`（通常是 window）上：包装 `cons` 的五个常用方法、
 * 捕获未捕获异常与未处理的 Promise 拒绝，并暴露 `target.__DSH_CONSOLE__`。
 *
 * 幂等：同一 target 重复安装返回既有记录器（HMR / 重复入口都安全）。
 * 没有 `addEventListener` 的环境（node 单测直接 import 本模块）安全跳过。
 */
export function installConsoleCapture({ target = globalThis, cons = console } = {}) {
  if (target.__DSH_CONSOLE__) return target.__DSH_CONSOLE__;
  if (typeof target.addEventListener !== 'function' || typeof cons !== 'object') return null;

  const recorder = createConsoleRecorder();
  const passThrough = (original, level) => (...args) => {
    recorder.record(level, formatConsoleArgs(args));
    // 纪律 ①：原样透传。这里若改成 return original(...) 的返回值，会改变
    // console 方法的语义（它们本来返回 undefined），也绝不能不调。
    if (typeof original === 'function') original.apply(cons, args);
  };

  const methods = ['log', 'info', 'warn', 'error', 'debug'];
  const originals = new Map();
  for (const level of methods) {
    originals.set(level, cons[level]);
    cons[level] = passThrough(cons[level], level);
  }

  const onError = (event) => {
    const where = event.filename ? ` @ ${event.filename}:${event.lineno}:${event.colno}` : '';
    recorder.record('error', `未捕获异常：${event.message}${where}`, {
      stack: event.error && event.error.stack ? String(event.error.stack).slice(0, ENTRY_MAX_CHARS) : '',
    });
  };
  const onRejection = (event) => {
    recorder.record('error', `未处理的 Promise 拒绝：${formatConsoleValue(event.reason)}`);
  };
  target.addEventListener('error', onError);
  target.addEventListener('unhandledrejection', onRejection);

  target.__DSH_CONSOLE__ = recorder;
  return recorder;
}
