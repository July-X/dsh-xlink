// 只替换仓库固定脚本的配置块。绝不接收令牌，也不执行用户填写的 Groovy。
export function jenkinsScript(source, { url, credentialsId, message, files, log }) {
  let endpoint;
  try { endpoint = new URL(url); }
  catch { throw new Error('请先启用可信局域网接收服务'); }
  const parts = endpoint.hostname.split('.');
  const ipv4 = parts.length === 4 && parts.every(part => /^(0|[1-9][0-9]{0,2})$/.test(part) && Number(part) <= 255);
  const privateIp = ipv4 && (['10', '127'].includes(parts[0]) || (parts[0] === '192' && parts[1] === '168') || (parts[0] === '172' && Number(parts[1]) >= 16 && Number(parts[1]) <= 31));
  if (!privateIp || endpoint.port === '0' || !['http:', 'https:'].includes(endpoint.protocol) || endpoint.username || endpoint.password || endpoint.search || endpoint.hash || !/^\/hooks\/v1\/endpoints\/[A-Za-z0-9_-]+$/.test(endpoint.pathname)) throw new Error('请先启用可信局域网接收服务');
  if (!credentialsId?.trim() || !['off', 'on_failure', 'always'].includes(log)) throw new Error('请填写凭据 ID 和有效日志策略');
  const config = { url: endpoint.href, credentialsId: credentialsId.trim(), message: String(message || ''), files: String(files || '').split(/\r?\n/).map(value => value.trim()).filter(Boolean), log };
  // Groovy 单引号字符串不插值；JSON 中的换行、反斜杠和引号再安全编码一层。
  const literal = JSON.stringify(config).replace(/\\/g, '\\\\').replace(/'/g, "\\'");
  const block = /^Map HOOK = \[[\s\S]*?^\]/m;
  if ([...source.matchAll(new RegExp(block.source, 'gm'))].length !== 1) throw new Error('固定脚本配置格式已变化，请更新应用');
  return source.replace(block, () => `Map HOOK = new groovy.json.JsonSlurper().parseText('${literal}')`);
}
