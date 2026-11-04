// 侧栏菜单与页头共用的图标表（2026-11-02）。
//
// 用户要求「页头不要再显示 01/02 那种编号，换成和左侧菜单一致的图标」——
// 两边各画一份必然漂，所以图标只在这里登记一次：侧栏 MENU_GROUPS 从
// PAGE_ICONS 取，页头由 PageHeadIcon.vue 按同一个 id 取。
import {
  Odometer,
  TrendCharts,
  Box,
  Connection,
  MagicStick,
  Collection,
  SetUp,
} from '@element-plus/icons-vue';

export const PAGE_ICONS = {
  overview: Odometer,
  usage: TrendCharts,
  versions: Box,
  plugins: Connection,
  skills: MagicStick,
  memory: Collection,
  settings: SetUp,
};
