// 探针的**接线形状与纪律**判据（2026-10-16）。
//
// ## 为什么需要这条
//
// 探针是手动夹具，跑它们的那一步在门禁之外——于是「探针坏了」这件事**没有任何机制会发现**，
// 直到某天有人要重跑某个实验，发现它跑不通。而它们踩过的坑（压缩阈值、流式形状、
// 覆盖层里 `headers` 的嵌套层级……）都写在各自的文件头上，**文件一改就没了**，
// 文件一丢就全没了。
//
// 上一轮那个真模型驱动器进仓时**没配判据**（本条就是来补那个缺口的），所以这里两个都管：
// · **在不在仓里**（丢文件时红）——清单里引用它们的段落会跟着红，但那是另一道门禁，
//   它只在有人去查那句话的时候才响；
// · **关键纪律还在不在**（被人改坏时红）——凭据走环境变量、只往 /tmp 写、内核路径**读
//   `active.txt` 而不是写死版本号**。
//
// ## 为什么用「读源码 + 剥注释再判」而不是「真的跑一次」
//
// 真跑一次要 2-6 分钟而且依赖本机内核安装树；这条判据只保证**形状与纪律**不变，
// 「它还能不能跑」由操作单那几节负责（都写着可复跑的命令）。两者职责不同：
// 这里挡住的是「有人顺手改坏一行、半年后才发现」。

import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import test from 'node:test';

const read = (relative) =>
  readFileSync(new URL(`../${relative}`, import.meta.url), 'utf8');

/**
 * **先剥注释再判**：三个探针文件的头部注释里原样写着所有被踩过的坑（包括
 * `did not activate`、`summary is not smaller than…`、`headers` 嵌一层）——
 * 不剥的话这些判据会命中自己写的解释，**判据命中解释 = 判据不判代码**。
 */
const code = (relative) =>
  read(relative)
    .replace(/\/\*[\s\S]*?\*\//g, '')
    .replace(/^\s*\/\/.*$/gm, '');

const FILES = [
  'test/probe/drive-real-model.mjs',
  'test/probe/drive-stub.mjs',
  'test/probe/stub-bridge.mjs',
  'test/probe/stub-model.mjs',
];

test('四个探针文件都在仓里：丢一个就红', () => {
  for (const file of FILES) {
    const text = read(file);
    assert.ok(text.length > 400, `${file} 看着不像完整的探针（${text.length} 字节）`);
  }
});

test('探针不许写死内核版本——升级后那个目录还在，于是它会静默跑错内核', () => {
  // 判的是**没有任何版本号形状的字面量**：真模型驱动器第一版写的是
  // `kernels/0.2.1-alpha.2`，升级之后那个目录依然存在，实验照跑、结论照出，
  // 只是那个结论属于旧内核。
  for (const file of FILES.filter((name) => name.endsWith('drive-real-model.mjs') || name.endsWith('drive-stub.mjs'))) {
    const source = code(file);
    assert.ok(
      !/kernels\/[\d.]+-(alpha|beta|rc)/.test(source),
      `${file} 里出现了写死的内核版本目录——那会在升级之后静默跑错内核`
    );
    assert.match(source, /active\.txt/, `${file} 要读 active.txt 才知道该用哪一版内核`);
  }
});

test('探针只往临时目录写，不碰用户的实例', () => {
  for (const file of FILES) {
    const source = code(file);
    // **`~/.dsh-xlink` 只允许出现在「读 active.txt」那一处**——那是只读路径解析，
    // 出现在别处就意味着它可能往用户的目录里写东西。
    const homeHits = source.match(/~\/\.dsh-xlink/g) ?? [];
    assert.equal(
      homeHits.length,
      0,
      `${file} 里出现了硬编码的 ~ 路径（探针只该走 PROBE_HOME）`
    );
    if (file.startsWith('test/probe/drive')) {
      assert.match(source, /PROBE_HOME/, `${file} 的写目录要能覆盖（默认 /tmp/p2probe）`);
    }
  }
});

test('凭据与端点一律走环境变量，仓里不写死任何密钥', () => {
  for (const file of FILES) {
    const source = code(file);
    // 长串十六进制/base64 形状 = 密钥。记忆插件自己生成的 id 也是这个形状，
    // 但它不出现在这三个文件里。
    assert.ok(
      !/[0-9a-f]{32,}/.test(source),
      `${file} 里出现了长十六进制串——那可能是被写死的密钥或令牌`
    );
    assert.ok(
      !/sk-[A-Za-z0-9]{16,}/.test(source),
      `${file} 里出现了 sk- 开头的串——那是被写死的密钥`
    );
  }
  // 真模型驱动器那两个是**必须**存在的（它们是唯一把真端点接进来的地方）。
  const real = code('test/probe/drive-real-model.mjs');
  assert.match(real, /PROBE_API_KEY/, '真模型驱动器要从环境变量拿凭据');
  assert.match(real, /PROBE_MODEL_BASE/, '真模型驱动器要从环境变量拿端点');
});

test('覆盖层里 headers 必须嵌一层：pi-ai 会静默忽略平铺的未知键', () => {
  const real = read('test/probe/drive-real-model.mjs');
  // **判注释之前先看形状**：这条纪律的说明在文件头注释里，而注释里正好写着
  // 「平铺到 provider 层会被当成未知键」——所以判的是 `code()` 之后仍然存在的形状。
  const source = code('test/probe/drive-real-model.mjs');
  assert.match(source, /headers:/, '覆盖层里要有 headers 那一层');
  // 平铺的形状是 `key: value` 直接挂在 provider 下——这里判的是**缩进层级**：
  // headers 的子项比 headers 自己深两级。
  // **按真实形状判，不要凭想象写正则**：覆盖层是「一个字符串数组」，`headers` 那一行与
  // 它的子项模板串在**同一行**里（`['        headers:', ...map(...) => \`          ${key}: …\`]`）。
  // 第一版写成「子项在下一行」，于是判据在代码正确时也是红的——**判据写错形状比没有判据更坏**。
  assert.match(source, /' {8}headers:'/, 'headers 那一层挂在 provider 下面（8 空格）');
  assert.match(
    source,
    /` {10}\$\{key\}: /,
    '它的子项缩进要更深（10 空格）——平铺到 provider 层会被 pi-ai 当未知键忽略掉'
  );
  assert.match(real, /端点 400/, '这个坑要留在文件里当解释（判据剥注释，它只查代码形状）');
});

test('压缩那三条参数还在覆盖层里：删掉任何一条压缩都不触发或不激活', () => {
  const source = code('test/probe/drive-stub.mjs');
  assert.match(source, /thresholdRatio/, '阈值必须压到极低，否则两个回合触发不了一次压缩');
  assert.match(source, /headroomTokens: 0/, 'headroomTokens 默认 65536，两万窗口下数学上不可能触发');
  assert.match(source, /maxTokens: 512/, 'headroomTokens 归零会让 maxTokens 默认取 0，构造期断言要求它为正');
  assert.match(source, /retainTokens: 10/, 'retainTokens 必须小于阈值');
});

test('桩桥的 /context 必须回真命中——空命中会让注入预算恒为 0，任何 A/B 都假绿', () => {
  const source = code('test/probe/stub-bridge.mjs');
  const start = source.indexOf("/v1/memory/context");
  assert.ok(start > 0, '桩桥要处理 /context');
  const body = source.slice(start, start + 900);
  assert.match(body, /hits:\s*\[/, '/context 要回 hits 数组');
  assert.match(body, /mem-stub-1/, 'hits 里要有至少一条真命中——空命中会让 injections 恒为 0');
});

/**
 * 桩模型的候选必须**带一个切片里真存在的 `evidence_seq`**，且**逐回合换一条正文**。
 *
 * 两条各自防一类「整套桩在某个场景下完全不可用」的缺陷，而它们在桩模型**原本的用途**里
 * 都没有症状（G12/G13 只关心压不压缩成，候选落不落库无关紧要）：
 * ① `evidence_seq` 写死成 1 ——而解析阶段要校验它指向切片里一条真的人类消息，
 *    切片的第一条在 seq 8（前面几条是内核注入的 runtime context 与 system-reminder），
 *    于是**每一条候选都被判「引用的序号不在这段切片里」而拒收**。症状是
 *    「提取请求发出去、模型回了合法 JSON、然后库里什么都没有」。
 * ② 每次提取都回同一条正文 ——第二次会被 `submit_candidate` 的同源指纹拒收，
 *    于是库里永远只有一条，而「第二次提取落不落得下库」正是 C6 与 H7 要验的那件事。
 */
test('桩模型的候选带真 evidence_seq、且逐回合换正文（否则它驱动不了任何候选场景）', () => {
  const source = code('test/probe/stub-model.mjs');
  // **序号要从请求里读出来**，不是写死的常量——写死的那个值在真实切片里不存在。
  //
  // **判源码字面时别自己踩转义**：第一版把 `/\[(\d+)\]/` 写进正则字面量时多转了一层
  // （heredoc → 文件 → regex 三层），于是判据在代码**正确**时也是红的——
  // 那条红还长得像「桩模型没有解析序号」。**判据写错形状比没有判据更坏**
  // （本仓栽过，见覆盖层缩进那条）。这里改成断言不含转义陷阱的**两段可读片段**。
  assert.ok(
    source.includes('用户[:：]') && source.includes('.exec(sliceText)'),
    'evidence_seq 要从请求里的 `[N] 用户：` 那几行解析出来（先按原文拼 sliceText，再 exec）'
  );
  assert.ok(!/evidence_seq:\s*1\b/.test(source), '不许把 evidence_seq 写死成 1');
  assert.match(source, /evidence_seq:\s*userSeq/, '回包要带解析出来的那个序号');
  // **反向验逮到过一条漏网的**：只判上面那句的话，把 `userSeq` 的**定义**改成写死的 1
  // （`const userSeq = 1;`）判据照样全绿——而那正是这个缺陷本来的形状。
  // **判据要杀的是「序号没有被解析出来」，不是「某一行没写着 1」。**
  assert.match(
    source,
    /const userSeq = hit \? Number\(hit\[1\]\) : 1;?/,
    'userSeq 的定义要从解析结果来——写死成常量时上面那条判据是绿的（实测）'
  );
  // 逐回合变化的正文：靠一个提取计数，且 `STUB_CANDIDATE` 仍能显式指定。
  assert.match(source, /let extractionCount = 0/, '要有一个提取请求的计数器');
  assert.match(source, /extractionCount \+= 1/, '每收到一次提取请求就 +1');
  assert.match(source, /process\.env\.STUB_CANDIDATE/, '显式指定那条路要留着');
});

/**
 * 「同会话、两次独立提取」是**增量提取唯一能量出来的形状**（2026-10-16 真机实测量到的）。
 *
 * 两条能力各自对应一次撞墙：
 * · `PROBE_LINGER_BETWEEN` ——默认把回合连着说完，而壳那边有 30 秒去抖，
 *   **同一会话的多个回合会被合并成一次提取**（C6 验收踩过）。一次运行里只发生一次提取时，
 *   `from_cursor` 恒是 0，于是「第二次从哪儿读」根本量不出来。
 * · `PROBE_SESSION_ID` ——跨两次驱动器跑同一个会话本来是另一条路，但 `session/prompt`
 *   **只创建不续接**，同一个 id 第二次跑会撞 `session "…" already exists`。
 *   实测就是撞了这句才轮到回合之间等待那条路，所以两个都要有。
 */
test('驱动器能造出「同会话、两次独立提取」：回合之间等待 + 会话 id 可钉住', () => {
  const source = code('test/probe/drive-real-model.mjs');
  assert.match(
    source,
    /const BETWEEN_MS = process\.env\.PROBE_LINGER_BETWEEN === '1'/,
    '回合之间要能等（默认连着说会被去抖合并成一次提取）'
  );
  assert.match(source, /if \(index < TURNS\.length - 1 && BETWEEN_MS > 0\)/, '最后一个回合之后不重复等');
  assert.match(
    source,
    /const sessionId =\s*process\.env\.PROBE_SESSION_ID\?\.trim\(\) \|\|/,
    '会话 id 要能被 PROBE_SESSION_ID 钉住（默认带时间戳，两次跑一定不是同一个会话）'
  );
});
