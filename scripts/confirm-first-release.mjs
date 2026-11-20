import { pathToFileURL } from 'node:url';

// 只有 latest.json 已返回 404 才走这里；资产缺失不等于没有正式 Release。
export async function confirmFirstRelease(request = fetch) {
  const response = await request('https://api.github.com/repos/July-X/dsh-xlink/releases/latest', {
    signal: AbortSignal.timeout(20_000),
  });
  if (response.status !== 404) {
    throw new Error(`无法确认首次发布（Release HTTP ${response.status}）；请检查已有 Release 的 latest.json`);
  }
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  await confirmFirstRelease();
  console.log('线上尚无正式 Release，跳过首次发布版本比较');
}
