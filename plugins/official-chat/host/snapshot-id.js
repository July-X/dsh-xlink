/**
 * 快照 id 的形状与**路径安全**闸（协议 S-1）。
 *
 * **形状是安全边界，不只是格式**：`snapshotId` 会直接拼进文件路径
 * （`refs/`、`snapshots/`、临时文件），所以这里只认小写字母数字与两个
 * `~`——路径分隔符、`.`、空格、相对路径片段一律不认。
 *
 * 独立成模块的原因：`host/index.js` 的快照存储与 `host/commands.js` 的
 * `release` 入口都要过同一道闸；从 index 反向 import 会形成
 * index → commands → index 的循环。
 */

/**
 * `ref` / `snapshotId` 的长度上限（协议 S-1：整串 ≤ 24 个可见字符）。
 *
 * 上限不是审美问题：内核自带 source 的 `clipboardText: (ref) => ref` 加上
 * chip 宽度按 ref 长度计 ⇒ **ref 的长度就是用户编辑器里那枚 chip 的宽度**。
 */
export const MAX_SNAPSHOT_ID_LEN = 24;

/** `<providerShort>~<sessionShort>~<seq>`；providerShort ≤ 4、sessionShort ≤ 12、seq 4 位。 */
const SNAPSHOT_ID_RE = /^[a-z0-9]{1,4}~[a-z0-9]{1,12}~\d{4}$/;

/**
 * 这个 id 是不是合法的快照 id（**路径安全**那一道闸）。
 *
 * @param {unknown} id - 待校验的 id。
 * @returns {boolean} 合法即 true。
 */
export function isValidSnapshotId(id) {
  const text = String(id ?? '');
  return text.length <= MAX_SNAPSHOT_ID_LEN && SNAPSHOT_ID_RE.test(text);
}
