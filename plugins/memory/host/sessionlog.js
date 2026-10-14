/**
 * 从会话日志里读一段**冻结的事件范围**——P2 自动提取的输入来源（设计 §8-1、§8-3）。
 *
 * ## 三步的顺序是有讲究的
 *
 * 1. `sessions.flush(session)` **先要一次落盘屏障**：刚刚 append 的事件可能还在内存里，
 *    不等它落盘就读，会读到一个比当前时刻旧的尾巴，而提取任务的游标（任务键里的 `cursor`）
 *    就此定死——**那一段内容从此没人再看它**。`flush` 的返回值是「有没有持久化监听器参与」：
 *    内核没开落盘时它是 `false`，这时**不能假装读到了什么**，如实回一个不可用的原因。
 * 2. `sessionPersistence.open(id, 'read')` 拿读句柄，再 `handle.read(offset, length)`。
 *    **不许用 `eventAt` / `snapshotEvents` / `ownEvents`**：那三个是已废弃的旧接口，新调用被禁
 *    （设计 §8-1 写明了），所以仓库里有一条判据扫这三个名字。
 * 3. 读完**必须关句柄**（`close()` 幂等）。关不上不能盖住读到的结果——那是拆房顶救地基。
 *
 * ## 为什么这个模块不 import 内核包
 *
 * 服务与会话都从参数进来，所以冒烟测试能拿假件把每条分支真的跑一遍（与 `bridge.js` 同一个理由：
 * 能脱离内核测的那一半单独放）。真机上的形状由探针验：`plugins/memory/test` 之外，
 * `docs/features/memory/memory-progress.md` 的「怎么验」那一节记了怎么在一个真会话里跑它。
 *
 * @module xlink-memory/sessionlog
 */

/** 一次读取的结果。`events` 与 `reason` 互斥：`ok` 为真时才有事件。 */
const unavailable = (reason) => ({ ok: false, events: [], reason });

/**
 * 读 `[from, to]` 这一段事件（含两端；`to` 省略表示读到末尾）。
 *
 * @param {object} options - 依赖与范围。
 * @param {{flush: Function}} options.sessions - 会话服务（`flush(session)`）。
 * @param {{open: Function}} options.sessionPersistence - 持久化服务（`open(id, 'read')`）。
 * @param {{id?: string}} options.session - 目标会话。
 * @param {number} [options.from] - 起始 seq（含）。
 * @param {number} [options.to] - 结束 seq（含）；省略则读到末尾。
 * @returns {Promise<{ok: boolean, events: object[], reason?: string}>}
 */
export async function readSessionRange({
  sessions,
  sessionPersistence,
  session,
  from = 0,
  to,
}) {
  const id = String(session?.id ?? '');
  if (!id) return unavailable('这个会话没有 id，读不了它的日志');
  if (!sessions || typeof sessions.flush !== 'function') {
    return unavailable('内核没有提供会话服务（sessions.flush）');
  }
  if (!sessionPersistence || typeof sessionPersistence.open !== 'function') {
    return unavailable('这个内核没有开落盘（sessionPersistence），拿不到冻结范围');
  }

  let flushed;
  try {
    flushed = await sessions.flush(session);
  } catch (error) {
    return unavailable(`等落盘屏障时失败：${String(error?.message ?? error)}`);
  }
  // `false` = 没有持久化监听器参与。这时候盘上那份是旧的，而「旧」比「没有」更危险：
  // 提取会基于一段不完整的历史下结论，而且游标还会往前推。
  if (flushed !== true) {
    return unavailable('没有持久化监听器参与（这次 flush 没人接），盘上那份可能不是最新的');
  }

  let handle;
  try {
    handle = await sessionPersistence.open(id, 'read');
  } catch (error) {
    return unavailable(`打不开会话日志：${String(error?.message ?? error)}`);
  }

  try {
    const length =
      typeof to === 'number' && Number.isFinite(to) ? Math.max(0, to - from + 1) : undefined;
    const result = await handle.read(from, length);
    const events = Array.isArray(result?.events) ? result.events : [];
    return { ok: true, events };
  } catch (error) {
    return unavailable(`读会话日志失败：${String(error?.message ?? error)}`);
  } finally {
    // 关句柄：关不上只吞掉，不能把已经读到的结果换成一条错误。
    try {
      await handle.close();
    } catch {
      /* 拆房顶救地基的事不做 */
    }
  }
}
