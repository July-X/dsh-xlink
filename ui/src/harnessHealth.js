// 工作台白屏健康探针共用的保守判定谓词。
export function shouldReportBlankHarness({ text = '', meaningful = false, childCount = 0 } = {}) {
  return !meaningful && !String(text).trim() && childCount === 0;
}
