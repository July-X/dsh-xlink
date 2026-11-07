const STATES = new Set(['pass', 'fail', 'not_run']);

// Accept only predefined results: no free text, account identifiers or URLs.
export function parseLoginAcknowledgment(raw, fields) {
  if (typeof raw !== 'string' || raw.length > 256) throw new Error('验收确认格式无效');
  let value;
  try { value = JSON.parse(raw.replace(/^\uFEFF/, '')); } catch { throw new Error('验收确认格式无效'); }
  if (!value || typeof value !== 'object' || Array.isArray(value) ||
      Object.keys(value).length !== fields.length ||
      fields.some(field => !Object.hasOwn(value, field) || !STATES.has(value[field]))) {
    throw new Error('验收确认只能包含规定的状态');
  }
  return Object.fromEntries(fields.map(field => [field, value[field]]));
}

export function classifyWorkbench(status, handles) {
  if (!status || typeof status.running !== 'boolean' || typeof status.window_open !== 'boolean' || !Array.isArray(handles)) {
    throw new Error('工作台状态不可用');
  }
  if (!status.running) return 'service_stopped';
  const listed = handles.includes('mcode-workbench');
  if (status.window_open && listed) return 'ready';
  if (status.window_open) return 'driver_missing';
  return listed ? 'inconsistent' : 'closed';
}
