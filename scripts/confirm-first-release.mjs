import { pathToFileURL } from 'node:url';

const RELEASES_URL = 'https://api.github.com/repos/July-X/dsh-xlink/releases/latest';

// 只有 latest.json 已返回 404 才走这里；资产缺失不等于没有正式 Release。
//
// **必须带 GITHUB_TOKEN**：未认证调 api.github.com 的配额是 60 次/小时/IP，
// 而 CI runner 的出口 IP 是共享的——配额耗尽时 API 回 403。那不是「确认不了
// 有没有正式 Release」，是限流，排查方向完全两回事（2026-11-21 代码 review
// 查实）。带 token 后配额是 5000 次/小时。
export async function confirmFirstRelease(request = fetch, env = process.env) {
  const token = env.GITHUB_TOKEN;
  const response = await request(RELEASES_URL, {
    signal: AbortSignal.timeout(20_000),
    headers: {
      Accept: 'application/vnd.github+json',
      'X-GitHub-Api-Version': '2022-11-28',
      ...(token ? { Authorization: `Bearer ${token}` } : {}),
    },
  });
  if (response.status === 404) {
    return;
  }
  if (response.status === 403) {
    throw new Error(
      'GitHub API 返回 403，疑似 API 限流或令牌权限不足；请确认 GITHUB_TOKEN 已注入后重试',
    );
  }
  throw new Error(
    `无法确认首次发布（Release HTTP ${response.status}）；请检查已有 Release 的 latest.json`,
  );
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  await confirmFirstRelease();
  console.log('线上尚无正式 Release，跳过首次发布版本比较');
}
