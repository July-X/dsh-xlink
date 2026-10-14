/**
 * 一次自动提取的**完整流程**：读冻结范围 → 拼切片 → 调模型 → 校验输出 → 提交候选。
 *
 * ## 为什么每一步都从参数进来
 *
 * 五步里有四步已经有各自的纯模块（`sessionlog.js` / `extract.js` / `bridge.js`），而真正
 * 碰内核的只有「调模型」那一步。把它们当依赖注入进来，这个编排层就能用夹具把**每条失败路径**
 * 都跑一遍——而失败路径才是它存在的理由：提取跑在**用户会话里**，任何一步出错都不许把会话
 * 带崩，也不许静默吞掉（要能说出「卡在第几步、为什么」）。
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
 * 跑一次提取。
 *
 * @param {object} options - 依赖与范围。
 * @param {object} options.deps - 五步的实现（全部注入）。
 * @param {object} options.key - 这次提取的身份（会话 + 游标 + 提取器版本）。
 * @param {number} options.from - 起始 seq（含）。
 * @param {number} [options.to] - 结束 seq（含）。
 * @param {AbortSignal} [options.signal] - 取消（H8 的最小一半）。
 * @returns {Promise<{ok: boolean, stage: string, candidates: object[], rejected: object[], submitted: number, reason?: string}>}
 */
export async function runExtraction({ deps, key, from, to, signal }) {
  const done = (fields) => ({
    ok: fields.ok === true,
    stage: fields.stage ?? 'done',
    candidates: fields.candidates ?? [],
    rejected: fields.rejected ?? [],
    submitted: fields.submitted ?? 0,
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

    step = 'model';
    // ③ 调模型。超时/取消在这里体现为 signal 中止。
    if (signal?.aborted) return done({ ok: false, stage: 'cancelled', reason: '调用前被取消' });
    const call = await deps.callModel({ system: deps.system, text: slice.text, key, signal });
    if (call?.ok !== true) {
      return done({ ok: false, stage: 'model', reason: call?.reason ?? '模型调用失败' });
    }

    step = 'parse';
    // ④ 校验。解析失败是失败，**空数组是成功**（设计 §8-5）。
    const parsed = deps.parse(call.text, { events: read.events });
    if (parsed?.ok !== true) {
      return done({ ok: false, stage: 'parse', reason: parsed?.reason ?? '输出不合 schema' });
    }
    if (parsed.candidates.length === 0) {
      return done({
        ok: true,
        stage: 'parse',
        rejected: parsed.rejected,
        reason: parsed.rejected.length > 0 ? '模型提的候选都被判据拒了' : '这段没有值得记的',
      });
    }

    step = 'submit';
    // ⑤ 提交。提交失败要说清「有几条没进去」——否则那几条就悄悄没了。
    if (signal?.aborted) return done({ ok: false, stage: 'cancelled', reason: '提交前被取消' });
    const submitted = await deps.submit({ key, candidates: parsed.candidates });
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
    });
  } catch (error) {
    // 纪律 1：不抛。哪一步炸的都算「跑不成」，但**要说清是哪一步**——第一版一律记成 'model'，
    // 于是「读日志时抛了」和「模型炸了」在壳那边长得一模一样（独立评审第 4 条）。
    return done({ ok: false, stage: step, reason: `提取过程抛了异常：${String(error?.message ?? error)}` });
  }
}
