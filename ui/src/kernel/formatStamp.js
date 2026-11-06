/**
 * 磁盘用量「统计时刻」的展示口径：`今天 14:20` / `昨天 18:05` /
 * `3 天前（10 月 1 日 14:20）`。
 *
 * **按日历日判今天/昨天，不按 24 小时差**：昨天 18:00 的扫描在今天 09:00
 * 打开时只差 15 小时，按 24 小时差会显示成「今天 18:00」——用户靠这句话判断
 * 数字新旧，读起来像刚统计过。未来时间戳（系统时钟回拨、缓存时间异常）单独
 * 给完整日期，不落「昨天」。
 *
 * 只显示日期 + 时刻、不显示秒：这张表的精度是「天」，给到秒会让人误以为数字
 * 在秒级变化。
 *
 * @param {number|string} ms 扫描时刻（毫秒时间戳）。
 * @param {number} [now] 当前时刻（测试注入用）。
 * @returns {string} 展示文本；时间戳无效或为 0 时返回空串。
 */
export function formatStamp(ms, now = Date.now()) {
  const value = Number(ms);
  if (!ms || Number.isNaN(new Date(value).getTime())) return '';

  const at = new Date(value);
  const current = new Date(Number(now));
  const hm = `${String(at.getHours()).padStart(2, '0')}:${String(at.getMinutes()).padStart(2, '0')}`;
  const dayOf = (date) => new Date(date.getFullYear(), date.getMonth(), date.getDate()).getTime();
  // 本地日历日的差值；夏令时当天 23/25 小时，四舍五入到整天。
  const days = Math.round((dayOf(current) - dayOf(at)) / 86400000);

  if (days === 0) return `今天 ${hm}`;
  if (days === 1) return `昨天 ${hm}`;
  if (days > 1) {
    return `${days} 天前（${at.getMonth() + 1} 月 ${at.getDate()} 日 ${hm}）`;
  }
  // 未来时间戳：给完整日期，不落「昨天」。
  return `${at.getFullYear()} 年 ${at.getMonth() + 1} 月 ${at.getDate()} 日 ${hm}`;
}
