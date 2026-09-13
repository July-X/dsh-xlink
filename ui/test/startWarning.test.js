import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

const read = (path) => readFileSync(path, 'utf8');

// 启动成功但存在非致命异常的通道：`replace_child_slot` 发现句柄槽位里原来那个内核
// 还活着（同一数据目录下不该有两个内核）时，此前只 `eprintln!` 一句——日志文件在
// 用户不会去看的地方，这件事就没人处理。契约横跨三个文件，所以在这里钉住：
// commands.rs 把它放进报告 → guard.rs 的 StartReport 带 warning 字段（有值才出现）
// → store.js 把它弹成提示。
test('非致命启动异常必须从 stderr 走到面板提示', () => {
  const commands = read('src-tauri/src/commands.rs');
  assert.match(
    commands,
    /report\.warning = Some\(warning\)/,
    'commands.rs 必须把 register_child 的 warning 放进启动报告',
  );
  assert.match(
    commands,
    /fn register_child\([\s\S]*?\) -> Option<String>/,
    'register_child 必须把 warning 返回给调用方（只写 stderr 就回到原样了）',
  );

  const guard = read('src-tauri/src/guard.rs');
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
