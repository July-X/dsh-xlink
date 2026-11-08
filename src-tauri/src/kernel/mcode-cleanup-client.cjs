// 凭据只从私有记录读入，不放命令行参数、界面或日志。
const fs = require('node:fs');
const net = require('node:net');
const record = JSON.parse(fs.readFileSync(process.argv[1], 'utf8'));
const action = process.argv[2];
if (record.nonce !== process.env.DSH_XLINK_CLEANUP_EXPECTED || record.endpoint !== process.env.DSH_XLINK_CLEANUP_ENDPOINT) process.exit(1);
delete process.env.DSH_XLINK_CLEANUP_EXPECTED;
delete process.env.DSH_XLINK_CLEANUP_ENDPOINT;
const socket = net.createConnection(record.endpoint);
let output = '';
const timer = setTimeout(() => { socket.destroy(); process.exit(1); }, 2500);
socket.on('error', () => process.exit(1));
socket.on('connect', () => socket.write(JSON.stringify({ nonce: record.nonce, action }) + '\n'));
socket.on('data', bytes => {
  output += bytes;
  if (output.length > 4096) { socket.destroy(); process.exit(1); }
});
socket.on('end', () => {
  clearTimeout(timer);
  process.stdout.write(output);
});
