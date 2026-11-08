// 只控制持有本轮随机凭据的启动器，绝不根据外部传入的 PID 发信号。
function xlinkCleanupControl(stop, service) {
  const net = require('node:net');
  const fs = require('node:fs');
  const endpoint = process.env.DSH_XLINK_MCODE_CLEANUP_ENDPOINT;
  const nonce = process.env.DSH_XLINK_MCODE_CLEANUP_NONCE;
  delete process.env.DSH_XLINK_MCODE_CLEANUP_ENDPOINT;
  delete process.env.DSH_XLINK_MCODE_CLEANUP_NONCE;
  // Windows 的管道 ACL 尚未显式限制和验证，暂不开放残留清理入口。
  // 仍先移除环境里的凭据，不能传给服务子进程。
  if (process.platform === 'win32' || !endpoint || !nonce) return;
  const ports = [Number(process.env.WEBUI_SERVER_PORT), Number(process.env.WEBUI_MINIAPP_PORT)];
  const server = net.createServer(socket => {
    socket.setTimeout(2000, () => socket.destroy());
    let buffer = '';
    socket.on('error', () => {});
    socket.on('data', bytes => {
      buffer += bytes;
      if (buffer.length > 4096) { socket.destroy(); return; }
      if (!buffer.includes('\n')) return;
      socket.removeAllListeners('data');
      let request;
      try { request = JSON.parse(buffer.slice(0, buffer.indexOf('\n'))); }
      catch { socket.destroy(); return; }
      // JSON.parse 可返回 null、数组和标量；认证前先拒绝非对象帧。
      if (request === null || typeof request !== 'object' || Array.isArray(request)
          || typeof request.nonce !== 'string' || typeof request.action !== 'string'
          || request.nonce !== nonce || !['probe', 'stop'].includes(request.action)) {
        socket.destroy(); return;
      }
      const child = service();
      const live = child && child.exitCode === null && child.signalCode === null;
      // 写端仍连着意味着另一壳还在管理它，不属于可清理的孤儿。
      const parent_connected = !process.stdin.readableEnded;
      const accepted = request.action === 'probe' || !parent_connected;
      const reply = { nonce, pid: process.pid, service_pid: live ? child.pid : null, ports, parent_connected, accepted };
      socket.end(JSON.stringify(reply) + '\n', () => {
        if (request.action === 'stop' && accepted) stop();
      });
    });
  });
  let ownedEndpoint = false;
  let closing = false;
  function releaseEndpoint() {
    closing = true;
    try { server.close(); } catch {}
    if (!ownedEndpoint) return;
    ownedEndpoint = false;
    // 只移除本次成功监听的精确路径；失败的 bind 不能删除别人的文件。
    try { fs.unlinkSync(endpoint); } catch {}
  }
  // Unix 启动器最终会 SIGKILL 自己，不会触发 exit；必须在 TERM 阶段收尾。
  process.on('SIGTERM', releaseEndpoint);
  process.on('SIGINT', releaseEndpoint);
  process.once('exit', releaseEndpoint);
  server.on('error', stop);
  server.listen(endpoint, () => {
    ownedEndpoint = true;
    if (closing) { releaseEndpoint(); return; }
    try { fs.chmodSync(endpoint, 0o600); } catch { stop(); }
  });
}
