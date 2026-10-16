// 桩模型：记下每次请求，按 OpenAI chat 形状回（**同时支持流式 SSE 与非流式**）。
//
// ## 它是干什么的
//
// G12/G13 那两个压缩实验要在**零模型额度**下重跑：真模型会让「压缩之后预算有没有被
// 清零」这件事与模型行为纠缠不清，而这里是确定性回包。
//
// ## 为什么三个桩文件都进仓（2026-10-16）
//
// 上一轮它们只在 `/tmp/p2probe`——于是清单里那条「已知缺口」是真的：G12/G13 要重跑就得
// 重新手搓一遍，而手搓的人不会重新踩那十个坑。真模型驱动器 `drive-real-model.mjs`
// 进仓时已经按同样的理由做过一遍（见那个文件的头部注释）。
//
// ## 实跑量到的一件事（2026-10-16，别照着它追）
//
// 6 回合下来压缩事件是**成功与失败交替**的：
//     compaction/end                      ← 成功
//     compaction/end error=summary is not smaller than the shadowed content
//                                 (106 estimated framed tokens >= 106)
// 两个数**相等**时压缩失败——而在这个覆盖层里 `thresholdRatio: 0.001`，于是**每回合都触发
// 压缩**，被遮蔽的内容经常就是上一轮压缩刚产出的那段短摘要。「摘要不比被遮蔽的内容更短」
// 在这种情况下是**阈值的必然结果，不是桩的缺陷**：桩这边每次摘要只回 45 字节
// （`model.log` 里那行 `REPLY summary … bytes=45` 就是证据）。
// 顺带一提：**G12（成功压缩）与 G13（失败压缩）两种形状它都能跑到**，所以那一对对照实验
// 不需要两个不同的桩——差别在观察哪一类事件。
//
// ## 环境变量
//
// - `PROBE_HOME`（默认 `/tmp/p2probe`）——**只往这里写**，不碰用户的实例。
// - `PROBE_MODEL_PORT`（默认 8791）——桩模型的端口。
// - `STUB_FAIL_SUMMARY=1` —— **G13 专用**：让压缩的**摘要那次调用**失败，于是压缩只有
//   `compaction/start` 没有 `compaction/end`。判别量正是「失败压缩之后能不能重新取得记忆」。
// - `STUB_REPLY` —— 换掉回的那段文字（要撑大上下文时用）。
import { createServer } from 'node:http';
import { appendFileSync, mkdirSync } from 'node:fs';
import { join } from 'node:path';

const PROBE_HOME = process.env.PROBE_HOME ?? '/tmp/p2probe';
const PORT = Number(process.env.PROBE_MODEL_PORT ?? 8791);
const TEXT =
  process.env.STUB_REPLY ??
  ('好的，我记住了：以后提交前都跑检查。' + '这是一段用来把上下文撑大的填充文字。'.repeat(60));

/** 提取请求数（逐回合换候选标题用，见下面那段注释）。 */
let extractionCount = 0;

mkdirSync(PROBE_HOME, { recursive: true });
const log = (line) => appendFileSync(join(PROBE_HOME, 'model.log'), `${line}\n`);

/**
 * 流式与非流式**都要回，而且回的是同一份 content**。
 *
 * **坑的第一半（实测踩过）**：适配器是流式的，只回非流式 JSON 会被解成空文本，于是插件
 * 只会说「模型没有返回可解析的 JSON」——症状指向解析，实际是传输形状错了。
 *
 * **坑的第二半（2026-10-16 实跑量到的，比第一半更阴）**：流式分支**回的是另一份内容**。
 * 那一版流式里硬编码了 `TEXT`，于是「摘要请求走流式」时拿到的是那段撑上下文的填充文字——
 * 而压缩会当场拒绝「摘要不比被遮蔽的内容更短」：
 *     compaction/end error=summary is not smaller than the shadowed content
 *     (377 estimated framed tokens >= 377)
 * **症状完全不像传输问题**（它像一个阈值配错了的参数），而真实的修法在桩模型的回包里。
 * 这也是为什么下面那行 `REPLY ... bytes=` 观测是必要的：没有它，「分支命中了吗」和
 * 「回的内容对吗」这两件事在日志里长得一模一样。
 */
function reply(res, payload, stream) {
  const content = String(payload?.choices?.[0]?.message?.content ?? '');
  if (!stream) {
    const body = JSON.stringify(payload);
    res.writeHead(200, {
      'content-type': 'application/json',
      'content-length': Buffer.byteLength(body),
    });
    return res.end(body);
  }
  res.writeHead(200, { 'content-type': 'text/event-stream' });
  const chunk = (delta) => res.write(`data: ${JSON.stringify(delta)}\n\n`);
  chunk({ choices: [{ index: 0, delta: { role: 'assistant' } }] });
  // 分几段发：真实端点也是分片的，一次性发完会让「流式」这条路径退化成非流式的观测。
  for (const part of content.match(/.{1,200}/gs) ?? []) {
    chunk({ choices: [{ index: 0, delta: { content: part } }] });
  }
  chunk({ choices: [{ index: 0, delta: {}, finish_reason: 'stop' }] });
  res.write('data: [DONE]\n\n');
  res.end();
}

createServer((req, res) => {
  let body = '';
  req.on('data', (chunk) => {
    body += chunk;
  });
  req.on('end', () => {
    log(`REQ ${req.method} ${req.url}`);
    log(`BODY ${body.slice(0, 40000)}`);
    // **G13 的开关**：按请求内容判「这是不是摘要那次调用」。压缩摘要走的是模型自己的
    // summarize 路径，请求里会带 `Condense` 之类的标记——匹配不到就退化成「不失败」，
    // 那时实验变成「成功压缩」而不是「失败压缩」，而两者要验的东西不一样。
    if (process.env.STUB_FAIL_SUMMARY === '1' && /Condense|summar/i.test(body)) {
      const payload = JSON.stringify({ error: { message: 'stub refuses to summarize (G13)' } });
      res.writeHead(500, {
        'content-type': 'application/json',
        'content-length': Buffer.byteLength(payload),
      });
      log('FAIL summarization request on purpose');
      return res.end(payload);
    }
    let stream = false;
    try {
      const parsed = JSON.parse(body || '{}');
      stream = Boolean(parsed.stream);
      const messages = Array.isArray(parsed.messages) ? parsed.messages : [];
      const system = String(messages.find((m) => m?.role === 'system')?.content ?? '');
      // **提取那条请求要回合法 JSON**：回别的（哪怕也是人话）会被判「没有可解析的 JSON」。
      // 这里只认一个标记词，不是为了聪明，而是为了让「这条请求是不是提取」在日志里一眼可见。
      const isExtraction = /durable, reusable memory/i.test(system);
      // **evidence_seq 必须是切片里真的一条人类消息**（2026-10-16 实跑逮到的）。
      //
      // 这一版原先写死 `evidence_seq: 1`，而解析阶段会校验「它确实指向切片里的一条
      // `user/message` 且 `source.kind === 'user'`」——切片的第一条人类消息在 **seq 8**
      // （前面几条是内核自己注入的 runtime context 与 system-reminder），
      // 于是**每一条候选都被判「引用的序号不在这段切片里」而拒收**，一条都提交不出去。
      //
      // **为什么这值这么久没被发现**：这套桩当初是为压缩实验造的（G12/G13），
      // 那两组只关心压不压缩成，候选落不落库无关紧要——于是「提交不出去」这一整类
      // 失败在那两组里**完全没有症状**。而它一旦被拿去驱动候选（C5/C6/H7），
      // 症状是「提取请求发出去了、模型回了合法 JSON、然后库里什么都没有」，
      // 与「模型没给候选」在日志里长得一模一样。
      //
      // **从请求里把序号读出来**，而不是猜一个：`[N] 用户：…` 那几行就是切片里的原文。
      //
      // **切片原文要在同一个闭包里算一次**（2026-10-16 实跑踩到）：第一版把它塞进
      // `userSeq` 那个 IIFE 里，于是下面按内容挑标题时引用了一个作用域外的 `text`
      // ——桩在 `req.on('end')` 里抛 ReferenceError，**一次回复都没有**，
      // 而症状是「回合跑完了但一条提取请求都没进 model.log」，与「模型没被调到」一模一样。
      const sliceText = messages
        .filter((m) => m?.role !== 'system')
        .map((m) => (typeof m?.content === 'string' ? m.content : ''))
        .join('\n');
      const hit = /\[(\d+)\]\s*用户[:：]/.exec(sliceText);
      const userSeq = hit ? Number(hit[1]) : 1;
      // **每回合给一条不同的正文**（2026-10-16 加）：写死一条的话，第二次提取会被
      // `submit_candidate` 的同源指纹拒收，于是库里永远只有一条——而「第二次提取
      // 落不落得下库」正是 C6（取代）与 H7（来源变了标过期）各自要验的那一件事。
      // `STUB_CANDIDATE` 仍然是显式指定那条路。
      const candidateTitle =
        process.env.STUB_CANDIDATE ??
        `约定 ${extractionCount + 1}：${sliceText.includes('npm') ? '这个仓库改用 npm install。' : '提交前要跑检查。'}`;
      extractionCount += 1;
      // **摘要那条调用必须回一句短的**，否则压缩**永远失败**——内核会拿
      // 「摘要不比被遮蔽的内容更短」当场拒掉：
      //     compaction/end error=summary is not smaller than the shadowed content
      //     (377 estimated framed tokens >= 377)
      // 于是这套桩只能跑出「失败压缩」那一种形状，而 G12 要验的恰恰是**成功**压缩之后
      // 预算有没有被清零。（2026-10-16 实跑量出来的：/tmp 那份老桩把两种调用回同一段长文本，
      // 所以它默认也只能跑出失败压缩。）
      const isSummary = /summar|condense/i.test(body) || /summar|condense/i.test(system);
      const content = isExtraction
        ? JSON.stringify([
            {
              title: candidateTitle,
              body: `${candidateTitle}（桩模型）`,
              keywords: ['提交', '检查'],
              evidence_seq: userSeq,
              conflicts_with: [],
              supersedes: [],
            },
          ])
        : isSummary
          ? process.env.STUB_SUMMARY_REPLY ?? '摘要：用户约定提交前先跑检查。'
          : TEXT;
      // **回包观测**：这三个标记是排障的第一现场（2026-10-16 加的）。之前「摘要太长」
      // 这个错误查了好几轮，症状上完全分不清是「摘要请求没被识别」还是「被遮蔽的内容太大」
      // ——而这两种的修法在两个不同的地方。
      log(
        `REPLY ${isExtraction ? 'extraction' : isSummary ? 'summary' : 'chat'}` +
          ` stream=${stream} bytes=${Buffer.byteLength(String(content), 'utf8')}`
      );
      return reply(
        res,
        {
          id: 'stub-1',
          object: 'chat.completion',
          model: 'stub-chat',
          choices: [
            {
              index: 0,
              message: { role: 'assistant', content },
              finish_reason: 'stop',
            },
          ],
          usage: { prompt_tokens: 100, completion_tokens: 50, total_tokens: 150 },
        },
        stream
      );
    } catch (error) {
      log(`BAD REQUEST ${String(error?.message ?? error)}`);
      const payload = JSON.stringify({ error: { message: 'stub could not parse the request' } });
      res.writeHead(400, {
        'content-type': 'application/json',
        'content-length': Buffer.byteLength(payload),
      });
      res.end(payload);
      return undefined;
    }
  });
}).listen(PORT, '127.0.0.1', () => console.log(`[stub-model] 127.0.0.1:${PORT}，日志 ${PROBE_HOME}/model.log`));
