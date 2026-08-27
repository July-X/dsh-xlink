# dsh-desktop 插件机制实现

`plugins.rs` 内部的 pnpm 调用、`.npmrc` 规则、lockfile 处理与 symlink 修复。设计层（用户可见的目录布局、双模式、接线、目录浏览）见 [plugin-management.md](plugin-management.md)；约定性约束（必须照做）见 [AGENTS.md](../AGENTS.md)。

## 完整流程（`install(spec)`）

1. **fetch**：git clone（深度 1）或 npm tarball 解压到 `~/.dsh/plugins/<id>/`
2. **ensure_store_npmrc**：写入 `~/.dsh/plugins/.npmrc`（`minimumReleaseAge=0`、固定 npm registry）—— pnpm v11 的 `minimumReleaseAgeExclude` 不支持通配符，必须直接关掉年龄检查
3. **install_store_deps**：`pnpm install --ignore-workspace --config.node-linker=hoisted --reporter=append-only`
   - 装依赖链 → 若有 `prepare` 脚本（`tsdown` / `tsc`）→ 触发构建 → `lib/` 就位
   - 装前**先删旧 `pnpm-lock.yaml`**：避开历史 lockfile 的 `minimumReleaseAge` 失效条目
4. **upsert_item**：写入 `~/.dsh/plugins/store.json`
5. **sync_kernels**：每个已装内核调用 `materialize_one`：
   - 解析 `resolved_source`（store 若本身是 symlink，展开到真实路径）
   - 校验现有 `target` symlink 是否等于 `resolved_source`——不等就重建（修复历史 double-symlink 链）
   - 调 `refresh_store_peers`：把内核 `node_modules/@deepseek-ai/*` 链接进 store 的 peer deps 解析路径
6. **ensure_wiring**：写 profile 的 `package.json` + `pnpm install` 把 `link:` 依赖铺到 `profiles/<name>/node_modules/`

## 坑

| 现象 | 根因 | 修复 |
| --- | --- | --- |
| `minimumReleaseAgeExclude` 通配符不生效 | pnpm 不支持通配符，必须 `package@version` 全列 | 改用 `minimumReleaseAge=0` |
| `Cannot find module .../lib/index.js`（TS 插件） | `pnpm install` 没触发 `prepare` 构建 | 修好 `.npmrc` + 删除旧 lockfile 后重装 |
| 内核 `node_modules/<id>` 看着对但解析失败 | store 若本身是 symlink，套一层形成 double-symlink | `materialize_one` `read_link` store，链上一个跳转 |
| 重启后 `pnpm install` 报 `ERR_PNPM_MINIMUM_RELEASE_AGE_VIOLATION` | 旧 lockfile 过期条目 | `install_store_deps` 先删 lockfile 再 re-resolve |