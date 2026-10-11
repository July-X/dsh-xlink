<script setup>
// 磁盘用量的下钻树：点某个条目（「正式版 666.5 MB」这一类小计行）就地展开
// 它**下一层**子项的占用，展开过的行还能继续往下点。统计按需发起——每层
// 展开才调一次 `disk_usage_children`，展开一节扫一节，前端按路径缓存结果；
// 比一次把整棵树展平便宜得多（`node_modules` 一层就是几万个文件）。
//
// 只读：树里没有任何删除入口，与瓦片同一纪律（2026-10-02 拍板）。文件行
// 是叶子，渲染成不可点的文本——`package.json` / `pnpm-lock.yaml` 常常就是
// 「这层为什么这么大」的答案，但没有「里面」。
import { reactive, watch } from 'vue';
import { invoke } from '../shell/bridge.js';
import { formatBytes } from '../shell/labels.js';

const props = defineProps({
  // 本层要列的条目。根层来自报表分组，下层来自 `disk_usage_children`。
  entries: { type: Array, default: () => [] },
  // 报表的 measuredAt。报表一换（手动刷新 / 后台重扫回填），子项缓存全部
  // 作废重取——旧数字配新报表会让「展开后的加总」与瓦片对不上。
  stamp: { type: Number, default: 0 },
  // 嵌套深度。0 是瓦片里的根层（带顶部分隔线与滚动约束），下层只缩进。
  depth: { type: Number, default: 0 },
});

// path → { loading, error, loaded, entries }。键用 path：同一条目在树里唯一。
const children = reactive({});
// path → 是否展开。与缓存分开记：收起再展开不重扫，刷新后展开状态保留。
const expanded = reactive({});

watch(
  () => props.stamp,
  () => {
    // 缓存清掉，展开状态保留：用户点开的路径没变，重扫后数字自己跟上。
    for (const key of Object.keys(children)) delete children[key];
    for (const key of Object.keys(expanded)) {
      if (expanded[key]) loadChildren(key);
    }
  },
);

async function loadChildren(path) {
  const cached = children[path];
  // 在途或已取到就不再发；出错时 loaded 仍是 false，「重试」走得进来。
  if (cached && (cached.loading || cached.loaded)) return;
  children[path] = { loading: true, error: null, loaded: false, entries: [] };
  try {
    const entries = await invoke('disk_usage_children', { path });
    children[path] = {
      loading: false,
      error: null,
      loaded: true,
      entries: Array.isArray(entries) ? entries : [],
    };
  } catch (e) {
    children[path] = {
      loading: false,
      error: e && e.message ? e.message : String(e),
      loaded: false,
      entries: [],
    };
  }
}

function toggle(entry) {
  if (!entry || !entry.isDir) return;
  const path = entry.path;
  expanded[path] = !expanded[path];
  if (expanded[path]) loadChildren(path);
}

// 悬停先给人话全名，再给路径：缩写（正式版）在同一张瓦片里可能重名，
// 真正能区分它们的是内核族与目录。与瓦片条目行同一约定。
function rowTitle(entry) {
  return entry.detail ? entry.detail + '\n' + entry.path : entry.path;
}
</script>

<template>
  <ul :class="depth === 0 ? 'usage-tree usage-tree--root' : 'usage-tree'">
    <li v-for="entry in entries" :key="entry.id" class="usage-tree__item">
      <!-- 目录行：整行可点（原生 button，键盘可达），行首箭头随状态翻转。 -->
      <button
        v-if="entry.isDir"
        type="button"
        class="usage-tree__row"
        :aria-expanded="expanded[entry.path] ? 'true' : 'false'"
        :title="rowTitle(entry)"
        @click="toggle(entry)"
      >
        <span class="usage-tree__caret" aria-hidden="true">{{
          expanded[entry.path] ? '▾' : '▸'
        }}</span>
        <span class="usage-tree__name">{{ entry.label }}</span>
        <span class="usage-tree__bytes">{{ formatBytes(entry.bytes) }}</span>
      </button>
      <!-- 文件行：叶子，不可点也不给箭头占位——缩进对齐目录名。 -->
      <div v-else class="usage-tree__row usage-tree__row--leaf" :title="rowTitle(entry)">
        <span class="usage-tree__caret" aria-hidden="true"></span>
        <span class="usage-tree__name">{{ entry.label }}</span>
        <span class="usage-tree__bytes">{{ formatBytes(entry.bytes) }}</span>
      </div>

      <template v-if="entry.isDir && expanded[entry.path]">
        <p v-if="children[entry.path] && children[entry.path].error" class="usage-tree__note">
          {{ children[entry.path].error }}
          <button
            type="button"
            class="usage-tree__retry"
            title="重新统计这一层"
            @click="loadChildren(entry.path)"
          >
            重试
          </button>
        </p>
        <DiskUsageTree
          v-else-if="children[entry.path] && children[entry.path].loaded"
          :entries="children[entry.path].entries"
          :stamp="stamp"
          :depth="depth + 1"
        />
        <p v-else class="usage-tree__note">正在统计这一层…</p>
      </template>
    </li>
    <!-- 展开到的空目录要给一句话，不然「点了没反应」和「真的是空的」
         分不清。根层的空态由瓦片自己说（「这一类当前没有内容。」）。 -->
    <li v-if="depth > 0 && entries.length === 0" class="usage-tree__item">
      <p class="usage-tree__note">空目录。</p>
    </li>
  </ul>
</template>

<style scoped>
/* 根层接住瓦片里原来 `.usage-entries` 的位置：顶部一道细线把「分类自己的
   数」与下钻列表分开；限高 + 内部滚动——下钻可以很深，页面高度不该跟着
   树一起长（与发布列表同一条纪律：滚动交给内层容器）。 */
.usage-tree {
  list-style: none;
  margin: 4px 0 0;
  padding: 0;
}

.usage-tree--root {
  border-top: 1px solid var(--border);
  padding-top: 4px;
  max-height: 220px;
  overflow-y: auto;
  overscroll-behavior: contain;
}

/* 下层只缩进：嵌套 ul 自带层叠，缩进随深度自然加深，不必数层数。 */
.usage-tree--root .usage-tree {
  margin-top: 0;
  padding-left: 12px;
  max-height: none;
  overflow: visible;
}

.usage-tree__item {
  margin: 0;
}

.usage-tree__row {
  display: flex;
  align-items: baseline;
  gap: 4px;
  width: 100%;
  padding: 1px 0;
  font-size: 11.5px;
  line-height: 1.5;
  text-align: left;
  background: none;
  border: 0;
  font: inherit;
  font-size: 11.5px;
  color: inherit;
  cursor: pointer;
  border-radius: 4px;
  min-width: 0;
}

.usage-tree__row:hover {
  background: var(--overlay-faint);
}

.usage-tree__row:focus-visible {
  outline: 1px solid var(--accent);
  outline-offset: -1px;
}

.usage-tree__row--leaf {
  cursor: default;
}

.usage-tree__row--leaf:hover {
  background: none;
}

.usage-tree__caret {
  flex: none;
  width: 10px;
  color: var(--usage-color, var(--accent));
  font-size: 10px;
}

.usage-tree__name {
  flex: 1 1 auto;
  min-width: 0;
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
  color: var(--text-secondary);
}

.usage-tree__bytes {
  flex: none;
  color: var(--text);
  white-space: nowrap;
  font-variant-numeric: tabular-nums;
}

/* 加载中 / 出错 / 空目录共用的注脚行。出错要能重试——读不到的目录不该
   把整棵树卡死在「正在统计」。 */
.usage-tree__note {
  margin: 1px 0 2px;
  padding-left: 14px;
  font-size: 11px;
  color: var(--text-muted);
  line-height: 1.5;
  display: flex;
  align-items: baseline;
  gap: 6px;
  min-width: 0;
}

.usage-tree__retry {
  flex: none;
  background: none;
  border: 0;
  padding: 0;
  font: inherit;
  font-size: 11px;
  color: var(--accent);
  cursor: pointer;
}

.usage-tree__retry:hover {
  color: var(--accent-strong);
}
</style>
