// stdin 由壳独占：开发重启、崩溃或强制退出都会关闭管道。
// 启动器自身保持为会话/进程组组长，直到收尾结束，避免 PGID 被复用。
const { spawn } = require('node:child_process');
const options = process.env.DSH_XLINK_MCODE_NODE_OPTIONS;
delete process.env.DSH_XLINK_MCODE_NODE_OPTIONS;
if (options !== undefined) process.env.NODE_OPTIONS = options;
let started = false;
let stopping = false;
let child;

function stop() {
  if (stopping) return;
  stopping = true;
  // 先安装兜底，再通知服务；即便服务忽略 TERM，两处监听也必须释放。
  setTimeout(() => process.kill(-process.pid, 'SIGKILL'), 1000);
  process.kill(-process.pid, 'SIGTERM');
}
process.on('SIGTERM', stop);
process.on('SIGINT', stop);
process.stdin.on('error', stop);
process.stdin.on('end', stop);
process.stdin.on('data', bytes => {
  for (const byte of bytes) {
    if (byte !== 1 || started || stopping) {
      stop();
      return;
    }
    started = true;
    // 子服务不继承控制管道；否则壳退出后 EOF 会被子服务自己延迟。
    child = spawn(process.execPath, [process.argv[1]], {
      stdio: 'ignore',
      env: process.env,
    });
    child.once('error', stop);
    child.once('exit', stop);
  }
});
process.stdin.resume();

if (typeof xlinkCleanupControl === 'function') xlinkCleanupControl(stop, () => child);
