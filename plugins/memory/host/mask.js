/**
 * 凭据形状的掩码。**与 Rust 侧 `diagnostics::run::strip_token_shapes` 同一套形状**：
 * 键名表由冒烟测试读那份 Rust 源码逐项钉住（`SENSITIVE_KEY_NEEDLES`），行为用例也刻意
 * 用同一组夹具（`sk-abcdef1234567890` / `token=ghp_secretvalue`）。
 *
 * ## 为什么插件里另有一份，而不是调壳
 *
 * 设计 §8-3 写的是「脱敏复用 `diagnostics::run::sanitize`」——那一条在**这一侧走不通**，
 * 两个原因各自都足够：
 *
 * ① **那段文本过不了桥**。输入是一整段会话（上限 64 KiB），而桥是 **GET + 查询串**
 *    （`bridge.js` 的 `MAX_PATH_BYTES` 7000，壳那边请求头缓冲固定 8 KB）。插件手里是唯一
 *    有这段文本的地方，除非把桥改成 POST 带请求体——那要先解决「body 可能读不全」那条
 *    既有约束（见 `bridge.js` 的注释），不是这一步该动的东西。
 * ② **整个 `sanitize` 会把输入毁掉**。它除了剥形状，还要把 home 折成 `~`、并把结果**截到
 *    480 字符**——这两条是给**日志行**用的（设计原文也把它的用途写在「失败日志」那一句里）。
 *    套到 64 KiB 的模型输入上，等于给模型看 480 个字符。
 *
 * 所以复用的是**形状那一半**，而且只复用形状：不匹配任意长随机串（那会把版本号、指纹、
 * 端口号一起吃掉），只认三类形状明确的。
 *
 * @module xlink-memory/mask
 */

/**
 * 键名里出现这些片段就当作敏感。
 *
 * **与 Rust 侧那份逐字一致**，由 `smoke.test.mjs` 的判据钉住：两边各写一份必然漂，
 * 而漂的那一侧不会报错——它只是**少抹掉一个**。
 */
export const SENSITIVE_KEY_NEEDLES = [
  'token',
  'secret',
  'apikey',
  'api_key',
  'password',
  'passwd',
  'credential',
  'authorization',
];

/** `sk-` 形态的最小长度：太短的（`sk-1`）多半是正文里的缩写，不是凭据。 */
const SK_MIN_CHARS = 9;

const isDigitOrLetter = (char) => /[A-Za-z0-9]/.test(char ?? '');
const isKeyChar = (char) => /[A-Za-z0-9_-]/.test(char ?? '');
/** 键名的首字符（Rust 那边是 `is_ascii_alphabetic() || '_'`）。 */
const isKeyStart = (char) => /[A-Za-z_]/.test(char ?? '');
const isSpace = (char) => /\s/.test(char ?? '');
/** 分隔符：`=`、`:` 与空格都算（Rust 那边同样是这三类）。 */
/**
 * 分隔符的**候选**字符：`=`、`:` 与空格。真正算不算「有分隔符」由 `hasSeparator` 判——
 * **必须至少有一个 `=` 或 `:`**，只隔了空格的不算（`token 这个词` 是散文）。与 Rust 侧同一条。
 */
const isSeparator = (char) => char === '=' || char === ':' || char === ' ';

/**
 * 值本身只是**认证方案名**时，真正的凭据在它后面那一段。
 *
 * `Authorization: Bearer abc.def.ghi` 里的值第一次扫出来只有 `Bearer` 这个词——只抹掉它
 * 是最坏的一种结果：输出**看起来**已经脱敏（`Authorization=***`），令牌却原样留在后面。
 * 2026-10-15 复刻 Rust 那套形状时撞见，两边一起修（Rust 侧的用例
 * `a_bearer_token_is_stripped_with_its_scheme_word` 用同一组夹具）。
 */
export const AUTH_SCHEMES = ['Bearer', 'Basic', 'Token'];

function schemeValueEnd(chars, valueStart, valueEnd) {
  const word = chars.slice(valueStart, valueEnd).join('');
  if (!AUTH_SCHEMES.includes(word)) return valueEnd;
  let cursor = valueEnd;
  while (cursor < chars.length && isSpace(chars[cursor])) cursor += 1;
  let end = cursor;
  while (end < chars.length && !isSpace(chars[end]) && chars[end] !== ',') end += 1;
  return end > cursor ? end : valueEnd;
}

function startsAt(chars, start, needle) {
  for (let offset = 0; offset < needle.length; offset += 1) {
    if (chars[start + offset] !== needle[offset]) return false;
  }
  return true;
}

/**
 * 把看起来像凭据的片段抹成 `***`。认不出的原样保留——**宁可漏抹也不要误抹**：
 * 误抹的是用户写下的正文，而正文是这条链路唯一的价值。
 *
 * @param {string} text - 待处理的文本。
 * @returns {string} 处理后的文本。
 */
export function maskSecrets(text) {
  if (typeof text !== 'string' || text.length === 0) return '';
  // 按**码点**切（`Array.from`），与 Rust 的 `Vec<char>` 对齐：按 UTF-16 码元切会在
  // emoji 与部分中文之外的字上把字符劈成两半。
  const chars = Array.from(text);
  let out = '';
  let index = 0;
  while (index < chars.length) {
    // ① `sk-` 开头的 key（DeepSeek / OpenAI 风格的最小前缀）。
    if (chars[index] === 's' && chars[index + 1] === 'k' && chars[index + 2] === '-') {
      let scan = index + 3;
      while (scan < chars.length && (isDigitOrLetter(chars[scan]) || chars[scan] === '-')) scan += 1;
      if (scan - index >= SK_MIN_CHARS) {
        out += 'sk-***';
        index = scan;
        continue;
      }
    }
    // ② `Bearer <token>`。
    if (chars[index] === 'B' && startsAt(chars, index, 'Bearer ')) {
      let scan = index + 7;
      while (scan < chars.length && !isSpace(chars[scan])) scan += 1;
      if (scan > index + 7) {
        out += 'Bearer ***';
        index = scan;
        continue;
      }
    }
    // ③ `<键>=<值>` / `<键>: <值>`，键名命中敏感片段。
    if (isKeyStart(chars[index])) {
      let end = index;
      while (end < chars.length && isKeyChar(chars[end])) end += 1;
      const key = chars.slice(index, end).join('');
      const lowered = key.toLowerCase();
      // 空格可以在分隔符**两侧**（`token = abc` 与 `token: abc` 都算），但**必须真的有一个
      // `=` 或 `:`**：`token 这个词` 是散文，没有分隔符就不该被吃（中文没有词间空格，一吃
      // 就是一整句）。与 Rust 侧同一条规则。
      let valueStart = end;
      let hasSeparator = false;
      while (valueStart < chars.length && isSeparator(chars[valueStart])) {
        if (chars[valueStart] !== ' ') hasSeparator = true;
        valueStart += 1;
      }
      const sensitive = SENSITIVE_KEY_NEEDLES.some((needle) => lowered.includes(needle));
      if (sensitive && hasSeparator) {
        let valueEnd = valueStart;
        while (valueEnd < chars.length && !isSpace(chars[valueEnd]) && chars[valueEnd] !== ',') {
          valueEnd += 1;
        }
        if (valueEnd > valueStart) {
          out += `${key}=***`;
          index = schemeValueEnd(chars, valueStart, valueEnd);
          continue;
        }
      }
    }
    out += chars[index];
    index += 1;
  }
  return out;
}
