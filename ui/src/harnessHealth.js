// Shared conservative predicate for the harness blank-page health probe.
export function shouldReportBlankHarness({ text = '', meaningful = false, childCount = 0 } = {}) {
  return !meaningful && !String(text).trim() && childCount === 0;
}
