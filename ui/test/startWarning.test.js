import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';
import { readShellSource } from '../../scripts/lib/shell-source.mjs';

const read = (path) => readFileSync(path, 'utf8');

// 启动成功但存在非致命异常的通道：`replace_child_slot` 发现句柄槽位里原来那个内核
// 还活着（同一数据目录下不该有两个内核）时，此前只 `eprintln!` 一句——日志文件在
// 用户不会去看的地方，这件事就没人处理。契约横跨三个文件，所以在这里钉住：
// commands.rs 把它放进报告 → guard.rs 的 StartReport 带 warning 字段（有值才出现）
// → store.js 把它弹成提示。
//
// 壳侧两个文件按**文件名**定位（见 scripts/lib/shell-source.mjs）：它们
// 2026-10-01 刚从 `src-tauri/src/` 平铺搬进 `diagnostics/`，写死路径的话这条
// 测试会在下一次目录改版时以 ENOENT 变红，而契约一个字都没变。
test('非致命启动异常必须从 stderr 走到面板提示', () => {
  // 2026-10-06：「把 warning 放进报告」这段搬进了 startup_run.rs。它属于
  // 「启动流程的尾部」，而看护只管进程活没活起来；留在 commands.rs 里会让那
  // 个已超反棘轮阈值的文件继续变大（2026-10-06 由 check:code-budget 逼出）。
  // 契约没变，变的只是它住在哪个文件——所以按新归属断言。
  const startup = readShellSource('startup_run.rs');
  assert.match(
    startup,
    /report\.warning = Some\(warning\)/,
    'startup_run.rs 必须把 register_child 的 warning 放进启动报告',
  );
  const commands = readShellSource('commands.rs');
  assert.match(
    commands,
    /fn register_child\([\s\S]*?\) -> Option<String>/,
    'register_child 必须把 warning 返回给调用方（只写 stderr 就回到原样了）',
  );

  const guard = readShellSource('guard.rs');
  assert.match(guard, /pub warning: Option<String>/, 'StartReport 必须带 warning 字段');
  assert.match(
    guard,
    /serde\(skip_serializing_if = "Option::is_none"\)\]\s*pub warning: Option<String>/,
    '没有异常时不该过桥一个 null（面板按真值判断，会弹空提示）',
  );

  const store = read('ui/src/store.js');
  assert.match(store, /if \(report\.warning\)/, 'store.js 必须处理启动结果里的 warning');
  assert.match(store, /toast\(report\.warning/, 'warning 必须以提示形式展示给用户');
});


// 诊断三页的入口与守卫。**源码契约测试**：这些都是「守卫」而不是行为——
// 删掉一行 `if` 不会让任何功能立刻坏掉，只会让某个场景在用户手动绕过
// UI 时失去拦截。行为测试覆盖不到「这行代码还在不在」。
test('预检与二分都必须检查实例级内核是否在运行', () => {
  const precheck = readShellSource('precheck.rs');
  assert.match(
    precheck,
    /instance_kernel_running\(family, target_instance\)/,
    '预检必须按实例 pid 判活：另一个壳可能正跑着用户自建实例（设计 §11.2）'
  );
  assert.match(
    precheck,
    /instance_kernel_running_message/,
    '预检必须复用 instance.rs 那份文案——同一句话写两遍，迟早漏掉「先关闭再试」'
  );

  const bisect = readShellSource('bisect_cmd.rs');
  assert.match(
    bisect,
    /instance_kernel_running\(&family, &instance_id\)/,
    '二分同样必须按实例判活，不能只查本壳工作台'
  );

  // 三条路径共用同一句「先停止再重试」的引导。
  for (const [name, src] of [['restore.rs', 'restore.rs'], ['precheck.rs', 'precheck.rs']]) {
    const text = readShellSource(src);
    assert.match(
      text,
      /instance_kernel_running_message/,
      `${name} 必须用统一文案，否则用户看不到下一步该做什么`
    );
  }
});
