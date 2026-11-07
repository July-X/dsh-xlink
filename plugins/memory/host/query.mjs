/**
 * 送进桥的查询串：长度上限与拼接。
 *
 * **为什么单独一个模块**：MiniMax 的 hook 只需要这一个函数，而它原来住在
 * `host/policy.js` 里——那个文件连带 `host/mask.js`、`host/extract.js` 一起
 * 有 80 KB，每回合提问都要让 node 解析一遍；而且它们是 DSH 插件的活文件，
 * 将来一旦 import `@deepseek-ai/*` 的包，MiniMax 实例里没有那个包，hook 直接
 * 加载失败（而失败是无声的）。这里只留纯函数，没有依赖。
 *
 * @module xlink-memory/query
 */

/**
 * 查询串的字符上限。
 *
 * 桥的请求头缓冲是固定 **8 KB**，而 `GET` 的查询串连同请求头都算在里面。中文经
 * `URLSearchParams` 编码后一个字符变 9 个字节（3 字节 UTF-8 × `%XX`），三倍膨胀——
 * 实测一条 1000 字的中文消息会让请求行到 9216 字节，直接越界。那之后
 * `read_request` 返回「请求头不完整」，**一个字节的响应都不写**就关连接，插件拿到
 * `socket hang up`，于是这一回合静默地没有记忆，而插件日志在这个内核里一行都不出
 * （设计稿 §9 的实测结论），用户与面板都看不见任何提示。
 *
 * **取尾部而不是头部**：最靠近当前要求的那几句才是检索要的。
 */
export const MAX_QUERY_CHARS = 500;

/**
 * 砍掉两端落单的代理。
 *
 * `slice` 按 **UTF-16 码元**切，emoji 与部分生僻汉字占 2 个码元，切点落在高/低代理
 * 之间就会留下半个。`URLSearchParams` 不会因此抛错（WHATWG 的序列化器把它替换成
 * U+FFFD，不像老的 `encodeURIComponent` 会抛 `URIError`），所以症状是**静默**的：
 * 那一个字符在检索时对不上，涉及它的关键词再也匹配不到。
 *
 * 这里取的是**尾部**，所以切出来只可能是「开头的落单低代理」（高代理被砍掉了）；
 * 末尾那一位一并检查，是因为拼接多个文本块时输入本身就可能已经带着半个。
 *
 * 砍掉那一个码元即可：少一个字词匹配，总好过多一个永远匹配不上的字符。
 *
 * @param {string} text
 * @returns {string}
 */
export function trimStraySurrogates(text) {
  if (!text) return text;
  let out = text;
  // 末尾落单只可能是**高**代理：低代理落在末尾时它的搭档高代理就在前一位，是好的。
  if (isHighSurrogate(out.charCodeAt(out.length - 1))) out = out.slice(0, -1);
  // 开头落单只可能是**低**代理，同理。
  if (isLowSurrogate(out.charCodeAt(0))) out = out.slice(1);
  return out;
}

function isHighSurrogate(code) {
  return code >= 0xd800 && code <= 0xdbff;
}

function isLowSurrogate(code) {
  return code >= 0xdc00 && code <= 0xdfff;
}

/**
 * 账本拼成送进桥的查询串：按 [`MAX_QUERY_CHARS`] 从尾部收。
 *
 * @param {string[]} texts
 * @returns {string}
 */
export function joinQueryTexts(texts, maxChars = MAX_QUERY_CHARS) {
  const list = Array.isArray(texts) ? texts.filter(Boolean) : [];
  if (list.length === 0) return '';
  return trimStraySurrogates(list.join('\n').slice(-maxChars));
}
