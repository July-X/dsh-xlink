/**
 * 一次自动提取的**完整流程**：读冻结范围 → 拼切片 → 检索既有记忆 → 调模型 → 校验输出 → 提交候选。
 *
 * ## 为什么每一步都从参数进来
 *
 * 六步里有五步已经有各自的纯模块（`sessionlog.js` / `extract.js` / `bridge.js`），而真正
 * 碰内核的只有「调模型」那一步。把它们当依赖注入进来，这个编排层就能用夹具把**每条失败路径**
 * 都跑一遍——而失败路径才是它存在的理由：提取跑在**用户会话里**，任何一步出错都不许把会话
 * 带崩，也不许静默吞掉（要能说出「卡在第几步、为什么」）。
 *
 * ## 为什么模型层要读 `finish`
 *
 * 内核把 provider 侧的失败变成**终结 finish chunk**，不抛异常：失败时 `assembler.finish` 才是
 * `{kind:'error', failure:{code,message}}`，而正文是空的。只挑 text 块拼正文的话，任何 provider
 * 失败都会被压成「空正文」，下游只能误判成 parse 失败，失败原因整个丢掉。
 *
 * ## 三条不许违背的纪律
 *
 * 1. **失败不抛**：整段 try/catch，任何一步出错都回一个带 `stage` 的结果。抛出会顺着
 *    `session/event` 的监听链回到内核，而内核拿它没办法——那是把插件的问题变成用户的问题。
 * 2. **没东西可提就成功**：切片里没有一句人话、或者模型回了空数组，都算**成功**（设计 §8-5），
 *    不提交、也不报错。用户什么都没说，不是故障。
 * 3. **只提交校验过的候选**：`parse` 的拒绝清单**不进提交**，但要跟着结果回去——它是
 *    「模型提了但不算数」的账，出问题时要看得见。
 *
 * @module xlink-memory/run
 */

/** 结果里 `stage` 的取值：卡在哪一步，排障时直接看这个。 */
export const STAGES = ['read', 'slice', 'model', 'parse', 'submit', 'cancelled', 'done'];

/**
 * 结果里 `errorKind` 的取值：**模型层失败的分类**。
 *
 * 为什么要有它：内核的 provider 失败是一个终结 finish chunk，而正文是空的——不分类的话，
 * 「额度用尽」与「网络断了」在下游长得一模一样，只能靠人去猜。
 */
export const ERROR_KINDS = Object.freeze([
  'empty_stream',
  'truncated',
  'timeout',
  'transport',
  'provider_error',
  'context_overflow',
  'missing_route',
  'unknown',
]);

/**
 * 内核失败 code → errorKind。认不出的 code 一律 `unknown`——**不许猜**：猜错的话排障的人
 * 会顺着错的分类去找原因，比明说「不知道」更费时间。
 *
 * `QUOTA` / `ACCOUNT_QUOTA` 是内核自己的额度 code 常量（`QUOTA_EXCEEDED_CODE` 就是
 * `QUOTA`），与 `QUOTA_EXCEEDED` 一起收，否则真的额度用尽会掉进 unknown。
 */
const CODE_ERROR_KINDS = Object.freeze({
  TIMEOUT: 'timeout',
  TRANSPORT: 'transport',
  SERVER: 'transport',
  // 流的终结事件没来（适配器自己抛的 `STREAM_CLOSED`）——与传输层断掉是同一类。
  STREAM_CLOSED: 'transport',
  EMPTY_RESPONSE: 'empty_stream',
  // **上下文超限单列**：它的修法与别的失败都不同（输入太长，要缩切片 / 少带既有记忆），
  // 归进 provider_error 只会让人去查账号与额度，方向全错。
  CONTEXT_WINDOW_EXCEEDED: 'context_overflow',
  QUOTA_EXCEEDED: 'provider_error',
  QUOTA: 'provider_error',
  ACCOUNT_QUOTA: 'provider_error',
  INVALID_CREDENTIAL: 'provider_error',
  RATE_LIMIT: 'provider_error',
  INVALID_REQUEST: 'provider_error',
  PI_AI_ERROR: 'provider_error',
});

/** 每种分类的一句人话；内核的 code 与 message 跟在后面，不被吞掉。 */
const ERROR_KIND_TEXT = Object.freeze({
  empty_stream: '模型没有返回正文',
  truncated: '输出被 token 上限截断，没有正文',
  timeout: '模型调用被中止或超时',
  transport: '模型调用传输失败',
  provider_error: '模型调用失败',
  context_overflow: '输入超出模型上下文上限',
  missing_route: '没有可用的模型路由',
  unknown: '模型调用失败',
});

/**
 * `usage` 里那件能分开「模型没说话」与「话没落进正文」的事实。
 *
 * `empty_stream` 有两种完全不同的成因：模型一个 token 都没生成（请求形态 / 路由问题），
 * 或者生成了但没进 text 块（只回 reasoning 的模型、把正文丢掉的适配器）。
 * 两者在 `jobs.json` 里只差这一个数——少了它，下一个人还是只能猜。
 * 没有 usage 时明说「没有」，**不许用 0 假装读到了**。
 */
function outputDetail(usage) {
  const output = Number(usage?.outputTokens);
  if (!Number.isFinite(output)) return '没有 usage';
  const reasoning = Number(usage?.reasoningTokens);
  const base = `输出 ${output} tokens`;
  return Number.isFinite(reasoning) && reasoning > 0 ? `${base}（其中 reasoning ${reasoning}）` : base;
}

/**
 * 把内核的失败事实拼成一句可读的原因。
 *
 * 为什么要带 code 与 message：分类只说「哪一类」，真正要排障的是内核给的那句原文——
 * 吞掉它的话，分类再准也只是换了个猜法。
 */
function failureReason(errorKind, code, message) {
  const base = ERROR_KIND_TEXT[errorKind] ?? ERROR_KIND_TEXT.unknown;
  if (code) return `${base}（${code}）${message ? `：${message}` : ''}`;
  return message ? `${base}：${message}` : base;
}

/**
 * 这次模型调用要不要带 `sessionId`，带什么。
 *
 * **只有 OpenCode 系需要**：`x-opencode-session` 是它按会话做路由 / 缓存的头，pi-ai 的
 * `opencode`（zen）与 `opencode-go` 两套套餐都包了同一层 `withOpenCodeSessionHeader`，
 * 而那一层只在 `options.sessionId` 存在时才发头——缺了就是 400 `MissingSessionID`
 * （2026-11-05 真机：提取连试三次全撞这条，而失败原因当时还不是这个形状）。
 *
 * 普通对话由 agent loop 传 `sessionId: session.id`，不做任何转换（`session-<uuid>` 就是
 * 服务端认的那个 key）。提取这一次性调用得自己传，传**任务所属会话**的 id——与普通对话
 * 同一个值。
 *
 * 别的路由**不传**：提取是插件自己的事，不该在用户的会话里留下一条谁也看不懂的记录。
 *
 * @param {string} provider - 路由的 provider 名（`route.provider`）。
 * @param {string} sessionId - 任务所属会话 id。
 * shortcut: 只认 provider 名；自定义 provider 把 baseUrl 指向 opencode.ai 时内核认得出、
 * 这里认不出（`selection` 不带 baseUrl），那条路由的提取仍会 400。要支持得让内核把 baseUrl
 * 一并给出来，或用一条配置项。
 *
 * @returns {{sessionId?: string}} 直接展开进 `call.stream` 的选项。
 */
export function modelSessionId(provider, sessionId) {
  const id = typeof sessionId === 'string' ? sessionId : '';
  if (!id) return {};
  return provider === 'opencode' || provider === 'opencode-go' ? { sessionId: id } : {};
}

/**
 * 判一次模型调用到底是成功还是失败。
 *
 * @param {object} [finish] - 内核的终结 finish reason（`{kind, failure}`）。没有终结 chunk 时
 *   内核的 `assembler.finish` 会给 `{kind:'stop'}`。
 * @param {string} [text] - 模型正文。
 * @param {{outputTokens?: number, reasoningTokens?: number}} [usage] - 这一次的 token 账；
 *   `assembler.usage` 原样传进来，空正文时靠它分开两种成因。
 * @returns {{ok: true} | {ok: false, errorKind: string, reason: string}}
 */
export function classifyModelFinish(finish, text, usage) {
  const kind = typeof finish?.kind === 'string' ? finish.kind : 'stop';
  const failure = finish?.failure ?? null;
  const code = typeof failure?.code === 'string' ? failure.code : '';
  const message = typeof failure?.message === 'string' ? failure.message : '';
  const body = typeof text === 'string' ? text.trim() : '';

  if (kind === 'error') {
    const errorKind = CODE_ERROR_KINDS[code] ?? 'unknown';
    const reason = failureReason(errorKind, code, message);
    // 适配器的 `EMPTY_RESPONSE` 也走这条路（它把「完成但无内容」判成错误），
    // 同样是空正文里最需要 usage 的那一类——不带上就分不出「真的 0 输出」与「没读到账」。
    return {
      ok: false,
      errorKind,
      reason: errorKind === 'empty_stream' ? `${reason}（${outputDetail(usage)}）` : reason,
    };
  }
  if (kind === 'aborted') {
    return { ok: false, errorKind: 'timeout', reason: failureReason('timeout', code, message) };
  }
  if (kind === 'max-tokens') {
    return { ok: false, errorKind: 'truncated', reason: failureReason('truncated', code, message) };
  }
  if (kind === 'tool-calls') {
    // 提取不传工具，出现工具调用就是异常形状——归 unknown，别装作成功。
    return {
      ok: false,
      errorKind: 'unknown',
      reason: failureReason('unknown', 'tool-calls', '提取不传工具，收到工具调用属于异常形状'),
    };
  }
  // **认不出的终结原因不许当成功**：内核的 FinishReason 是 merge-extensible，出现没见过的
  // kind 时不猜它是哪一类失败（正文也可能只是被截了一半），把原值写进原因，下次照它补判定表。
  if (kind !== 'stop') {
    return {
      ok: false,
      errorKind: 'unknown',
      reason: `${failureReason('unknown', '', message)}（finish=${kind}，${outputDetail(usage)}）`,
    };
  }
  // stop（以及没有终结 chunk 的默认）：**有正文才算成功**，空正文是「模型没说话」。
  if (body.length === 0) {
    return {
      ok: false,
      errorKind: 'empty_stream',
      reason: `${failureReason('empty_stream', code, message)}（finish=stop，${outputDetail(usage)}）`,
    };
  }
  return { ok: true };
}

/**
 * 结果里多带的那一个数：**这一次的候选到底对着几条既有记忆判过冲突**（C5）。
 *
 * 为什么值得单独报出来：`conflicts_with` 为空有两种完全不同的原因——「真没有冲突」与
 * 「一条都没对照上」（桥不通、检索失败）。只报空数组的话，排障的人会以为模型没发现冲突，
 * 而真相是它压根没看到过库里的东西。
 */

/**
 * 跑一次提取。
 *
 * @param {object} options - 依赖与范围。
 * @param {object} options.deps - 六步的实现（全部注入）。
 * @param {object} options.key - 这次提取的身份（会话 + 游标 + 提取器版本）。
 * @param {number} options.from - 起始 seq（含）。
 * @param {number} [options.to] - 结束 seq（含）。
 * @param {AbortSignal} [options.signal] - 取消（H8 的最小一半）。
 * @returns {Promise<{ok: boolean, stage: string, candidates: object[], rejected: object[], submitted: number, errorKind: string, reason?: string}>}
 */
export async function runExtraction({ deps, key, from, to, signal }) {
  const done = (fields) => ({
    ok: fields.ok === true,
    stage: fields.stage ?? 'done',
    candidates: fields.candidates ?? [],
    rejected: fields.rejected ?? [],
    submitted: fields.submitted ?? 0,
    refused: fields.refused ?? 0,
    compared: fields.compared ?? 0,
    againstPending: fields.againstPending ?? 0,
    contentHash: fields.contentHash ?? '',
    // **只有模型层会填**：别的阶段失败时留空串——分类是给「模型这一层」用的，
    // 读日志失败也说成「传输失败」只会把排障的人带偏。
    errorKind: fields.errorKind ?? '',
    ...(fields.refusedReason === undefined ? {} : { refusedReason: fields.refusedReason }),
    ...(fields.reason === undefined ? {} : { reason: fields.reason }),
  });

  let step = 'read';
  try {
    if (signal?.aborted) return done({ ok: false, stage: 'cancelled', reason: '开始前就被取消了' });

    // ① 读冻结范围。读不到**绝不**继续——拿一段旧历史下结论比不提取更糟（见 sessionlog.js）。
    const read = await deps.readRange({ session: key.session, from, to });
    if (read?.ok !== true) {
      return done({ ok: false, stage: 'read', reason: read?.reason ?? '读取失败' });
    }

    step = 'slice';
    // ② 拼切片。没有一句人话就当成功：用户没说什么值得记的。
    const slice = deps.buildSlice(read.events);
    if (!slice.text) {
      return done({ ok: true, stage: 'slice', reason: '这一段里没有一句人话' });
    }

    // ③ **提取前先检索一次既有记忆**（C5，设计 §7-2「提取前用候选关键词走现成的 /context
    // 拿命中」）：没有这份对照，模型就标不出 `conflicts_with`，而「没标冲突」会被用户
    // 当成「与库里的都不冲突」。
    //
    // **这一步失败不算提取失败**，只是这次没有可对照的既有记忆（`compared: 0` 会一起回去）：
    // 桥抖一下就把这一段内容整个丢掉，比「少标几个冲突」坏得多。它也不改 `step`——
    // 失败与成功都不该让「卡在第几步」这句话指向别处。
    let existing = [];
    let pending = [];
    try {
      const recalled = await deps.retrieveForExtract?.({ text: slice.text, key, signal });
      if (recalled?.ok === true) {
        existing = Array.isArray(recalled.items) ? recalled.items : [];
        pending = Array.isArray(recalled.pending) ? recalled.pending : [];
      }
    } catch {
      existing = [];
      pending = [];
    }

    step = 'model';
    // ④ 调模型。超时/取消在这里体现为 signal 中止。
    if (signal?.aborted) return done({ ok: false, stage: 'cancelled', reason: '调用前被取消' });
    // **那一段内容的指纹**（H7）：只有插件读得到那段原文，所以壳算不出来，只能这里给。
    // 它进的是**来源**而不是候选正文——「来源变了」是关于「这条是从哪句话来的」，不是关于
    // 「模型提了什么」。
    const contentHash = deps.fingerprint(slice.text);
    const call = await deps.callModel({
      system: deps.system,
      text: slice.text,
      existing,
      pending,
      key,
      signal,
    });
    if (call?.ok !== true) {
      // **分类跟着结果回去**：失败停在 model 这一步，阶段不变，但「哪一类失败」不能丢——
      // 少了它，额度用尽与网络断了在壳那边长得一模一样。
      return done({
        ok: false,
        stage: 'model',
        reason: call?.reason ?? '模型调用失败',
        errorKind: call?.errorKind ?? '',
      });
    }

    step = 'parse';
    // ⑤ 校验。解析失败是失败，**空数组是成功**（设计 §8-5）。
    // **把「模型看到的那一段」也交给校验**：`evidence_seq` 只许指向切片里的行（`slice.seqs`）。
    // 只给整份日志的话，判据会覆盖整个会话——历史里总能找到一个人类序号，
    // 「工具输出里的指令不能升级成用户偏好」就白写了（独立评审第 7 条）。
    const parsed = deps.parse(call.text, {
      events: read.events,
      allowedSeqs: new Set(slice.seqs),
      // **冲突标记的校验集合 = 这一批真的给模型看过的 id**（不是全库）：模型没见过的 id
      // 标出来就是编的，而放它过去等于把 C5 的机械判据关掉。
      allowedConflictIds: new Set(existing.map((row) => String(row?.id ?? '')).filter(Boolean)),
      // **取代标记的校验集合是另一份**（C6）：它只许引用这次真的给模型看过的**待确认候选**
      // id。两份集合分开而不是合成一份——合成之后模型就能拿一条记忆的 id 去标「取代」，
      // 而候选与记忆根本不是一回事，混了之后那道校验就等于没有。
      allowedSupersedeIds: new Set(pending.map((row) => String(row?.id ?? '')).filter(Boolean)),
    });
    if (parsed?.ok !== true) {
      return done({ ok: false, stage: 'parse', reason: parsed?.reason ?? '输出不合 schema' });
    }
    if (parsed.candidates.length === 0) {
      return done({
        ok: true,
        stage: 'parse',
        rejected: parsed.rejected,
        compared: existing.length,
        againstPending: pending.length,
        reason: parsed.rejected.length > 0 ? '模型提的候选都被判据拒了' : '这段没有值得记的',
      });
    }

    step = 'submit';
    // ⑥ 提交。提交失败要说清「有几条没进去」——否则那几条就悄悄没了。
    if (signal?.aborted) return done({ ok: false, stage: 'cancelled', reason: '提交前被取消' });
    const submitted = await deps.submit({ key, candidates: parsed.candidates, contentHash });
    if (submitted?.ok !== true) {
      return done({
        ok: false,
        stage: 'submit',
        candidates: parsed.candidates,
        rejected: parsed.rejected,
        reason: submitted?.reason ?? '提交失败',
      });
    }
    return done({
      ok: true,
      stage: 'done',
      candidates: parsed.candidates,
      rejected: parsed.rejected,
      submitted: Number(submitted.submitted ?? parsed.candidates.length),
      compared: existing.length,
      againstPending: pending.length,
      // **指纹要跟着结果回去**：它是这次提取的「来源身份」，而 `index.js` 那边的日志与
      // 排查都要靠它对上「这一条是从哪一段来的」。少这一行的话指纹在结果里永远是空串，
      // 而症状是「stale 偶尔不触发」——没有任何地方会报错。
      contentHash,
      // **提交阶段被拒的条数要跟着回去**（独立评审第 5 条）：否则「落库 3 条」看起来就是全部，
      // 而另外 7 条的去向只存在于提交那一步的局部变量里。
      refused: Number(submitted.refused ?? 0),
      ...(submitted.refusedReason ? { refusedReason: submitted.refusedReason } : {}),
    });
  } catch (error) {
    // 纪律 1：不抛。哪一步炸的都算「跑不成」，但**要说清是哪一步**——第一版一律记成 'model'，
    // 于是「读日志时抛了」和「模型炸了」在壳那边长得一模一样（独立评审第 4 条）。
    return done({ ok: false, stage: step, reason: `提取过程抛了异常：${String(error?.message ?? error)}` });
  }
}
