import { computed, reactive, ref } from 'vue';
import { mcodeExperience } from '../kernel/experience.js';
import { hasBridge, invoke } from './bridge.js';
import { withLoading } from './loading.js';
import { formatActionError, toastError } from './notify.js';
import { createSessionSlotWriter, HIDDEN_SESSION_SLOT, sessionSlotRequest } from './sessionSurfaceSlot.js';

const SURFACES = [
  { kind: 'harness', label: 'DSH 工作台' },
  { kind: 'mcode-workbench', label: 'MiniMax 工作台' },
  { kind: 'official-chat', label: '官网网页版' },
];

const opened = reactive(new Set());
const placements = reactive(new Map());
const selected = ref(null);

export const visibleSessions = computed(() => SURFACES.filter(({ kind }) =>
  opened.has(kind) && (kind !== 'mcode-workbench' || mcodeExperience.value),
));
export const activeSession = computed(() =>
  visibleSessions.value.find(({ kind }) => kind === selected.value) ?? visibleSessions.value[0],
);
export const activeSessionPoppedOut = computed(() => placements.get(activeSession.value?.kind) === true);

function eventKind(payload) {
  const kind = typeof payload === 'string' ? payload : payload?.kind;
  return SURFACES.some((surface) => surface.kind === kind) ? kind : null;
}

export function openSessionSurface(payload) {
  const kind = eventKind(payload);
  if (!kind) return false;
  opened.add(kind);
  if (kind === 'mcode-workbench' && !mcodeExperience.value) return false;
  selectSessionSurface(kind);
  return true;
}

export function applySessionPlacement(payload) {
  const kind = eventKind(payload);
  if (kind && typeof payload.poppedOut === 'boolean') placements.set(kind, payload.poppedOut);
}

export function closeSessionSurface(payload) {
  const kind = eventKind(payload);
  if (!kind) return;
  opened.delete(kind);
  placements.delete(kind);
  if (selected.value === kind) selected.value = activeSession.value?.kind ?? null;
}

export function selectSessionSurface(kind) {
  selected.value = kind;
}

export const SESSION_PLACEMENT_LOADING = 'sessionSurfacePlacement';

export function toggleSessionPlacement() {
  const kind = activeSession.value?.kind;
  if (!kind) return;
  const poppedOut = !activeSessionPoppedOut.value;
  return withLoading(SESSION_PLACEMENT_LOADING, async () => {
    try {
      await invoke('set_session_surface_popped_out', { kind, poppedOut });
    } catch (error) {
      toastError(formatActionError(poppedOut ? '弹出会话失败' : '归位会话失败', error));
    }
  });
}

const writeSlot = createSessionSlotWriter((slot) => invoke('set_session_surface_slot', slot));

export function hideSessionSlot() {
  if (hasBridge()) writeSlot(HIDDEN_SESSION_SLOT);
}

export function syncSessionSlot(slot) {
  if (!hasBridge()) return;
  const kind = !activeSessionPoppedOut.value && !document.hidden && activeSession.value?.kind;
  if (!kind || !slot?.isConnected) return hideSessionSlot();
  const rect = slot.getBoundingClientRect();
  // 原生子 webview 不受 DOM z-index 限制。主窗模态层与插槽相交的浮层出现时，
  // 先藏原生层；弹层退场后再恢复，不必重开内容。
  const overlays = document.querySelectorAll(
    '.progress-overlay, .diagnosis, .render-error-fallback, [role="dialog"], .el-overlay, .el-popper, .el-message',
  );
  for (const overlay of overlays) {
    const bounds = overlay.getBoundingClientRect();
    if (bounds.width && bounds.height && getComputedStyle(overlay).visibility === 'visible' &&
        bounds.left < rect.right && bounds.right > rect.left &&
        bounds.top < rect.bottom && bounds.bottom > rect.top) return hideSessionSlot();
  }
  writeSlot(sessionSlotRequest(kind, rect, { width: window.innerWidth, height: window.innerHeight }));
}
