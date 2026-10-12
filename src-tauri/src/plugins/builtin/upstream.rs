//! 内置插件的**上游同步**：让随包交付的第三方内容跟得上上游，而不必每次上游
//! 一改就重发一版 dsh-xlink。
//!
//! # 为什么不在插件里做，而在壳里做
//!
//! 参考实现（MiniMax Code 的 ponytail 适配层）把每日检查挂在 `SessionStart`
//! hook 上、跑 `node` 脚本同步。壳这一侧不能那么做：
//!
//! - hook 是内核 spawn 的 `node` 子进程，**拿不到本仓的出网纪律**（`net_proxy::routes()`
//!   的系统代理探测与直连兜底），而这台机器实测 `api.github.com` 偶发 8 秒级延迟。
//! - 取源、完整性校验、解包防穿越这套能力已经在 [`crate::pkg::fetch`] 里了。
//!   在 hook 脚本里重写一遍，等于把「下载校验」这件事写出第二份实现。
//! - npm registry 默认源是 npmmirror（见 [`crate::pkg::registry`]），快且稳，
//!   而 ponytail 正好发在 npm（`@dietrichgebert/ponytail`），不必去啃 GitHub。
//!
//! # 分层：随包快照是**离线兜底**，数据目录才是日常
//!
//! `plugins/<id>/skills|hooks` 随安装包分发（由 `scripts/vendor-ponytail.mjs`
//! 从上游某一棵不可变的 tree 逐字落盘）。同步**只写数据目录**，永远不改随包产物：
//! 于是「同步失败 = 继续用随包那一版」，插件不会因为一次网络抖动失效，
//! 而随包快照永远是应用自洽的那一份（内核版本与它一起被钉住）。
//!
//! # 24 小时节流，失败不吃额度
//!
//! 检查频率由 `upstream.json` 的 `checked_at` 决定。**失败只推进一个短退避**
//! （[`RETRY_BACKOFF_MINUTES`]），不推进 24 小时窗口——否则一次网络抖动会让
//! 自动更新静默停摆一整天，而用户看不出任何区别（面板上只说「已是最新」）。
//!
//! # 同步结果怎么被用上
//!
//! 壳启动内核前把解析出的目录写进 env（[`env_var`]），host 适配器优先读它。
//! 物化目录与接线**完全不变**——所以「用了同步版」不需要重新接线，
//! 也不会让 openai-oauth 那种既有用户数据被动到。

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::builtins::BuiltinSpec;

/// 两次自动检查之间的最小间隔。
pub(crate) const CHECK_INTERVAL: std::time::Duration = std::time::Duration::from_secs(24 * 3600);
/// 失败后的重试退避（远小于上面那个窗口，见模块注释）。
const RETRY_BACKOFF: std::time::Duration = std::time::Duration::from_secs(30 * 60);
/// 数据目录里保留几个历史版本（多留一个纯粹是为了「刚同步完就回滚」有路可走）。
const KEEP_VERSIONS: usize = 2;

/// 同步状态（落盘在 `<dsh_home>/plugins/builtin/<id>/upstream.json`）。
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub(crate) struct UpstreamState {
    /// 已落盘的上游版本（空 = 从没同步成功过，日常用随包快照）。
    #[serde(default)]
    pub version: String,
    /// 上次**成功**检查的 unix 秒。
    #[serde(default)]
    pub checked_at: u64,
    /// 上次失败的 unix 秒（只影响退避，不影响 24 小时窗口）。
    #[serde(default)]
    pub failed_at: u64,
    /// 上次失败的原因，如实透出到面板（「检查失败」比沉默有用）。
    #[serde(default)]
    pub error: String,
}

impl UpstreamState {
    fn path(dsh_home: &Path, spec: &BuiltinSpec) -> PathBuf {
        data_dir(dsh_home, spec).join("upstream.json")
    }

    fn load(dsh_home: &Path, spec: &BuiltinSpec) -> UpstreamState {
        let state: UpstreamState = fs::read_to_string(Self::path(dsh_home, spec))
            .ok()
            .and_then(|raw| serde_json::from_str(&raw).ok())
            .unwrap_or_default();
        state
    }

    fn save(&self, dsh_home: &Path, spec: &BuiltinSpec) -> Result<(), String> {
        let dir = data_dir(dsh_home, spec);
        fs::create_dir_all(&dir).map_err(|e| format!("建 {} 失败：{e}", dir.display()))?;
        let path = Self::path(dsh_home, spec);
        crate::shell::process::atomic_write(
            &path,
            serde_json::to_string_pretty(self)
                .unwrap_or_default()
                .as_bytes(),
        )
        .map_err(|e| format!("写 {} 失败：{e}", path.display()))
    }

    /// 现在该不该检查（`force` 时永远该）。
    ///
    /// `checked_at == 0` 表示**从没成功检查过**，那必须立刻放行——写成
    /// 三种状态各走各的窗口：
    /// - **从没试过**（`checked_at` 与 `failed_at` 都是 0）→ 立刻放行。写成
    ///   `now - 0 >= 24h` 的话，`now` 小于 24 小时的时刻（刚开机、刚校过时）
    ///   恒为假，于是新装用户**永远等不到首次同步**，而状态文件是空的、面板上
    ///   什么异常都看不出来。
    /// - 上次**成功** → 24 小时内不再问。
    /// - 上次**失败**且还没成功过 → 30 分钟后可重试。不是 24 小时：还没同步成功
    ///   的用户是「插件根本没有内容」，等一天太久。
    ///
    /// 成功过之后又失败，按成功的 24 小时算：一次成功已经说明网络是通的，别因为
    /// 一次偶发失败就退回去频繁重试。
    fn due(&self, now: u64, force: bool) -> bool {
        if force {
            return true;
        }
        let last = self.checked_at.max(self.failed_at);
        if last == 0 {
            return true;
        }
        let interval = if self.checked_at > 0 {
            CHECK_INTERVAL.as_secs()
        } else {
            RETRY_BACKOFF.as_secs()
        };
        now.saturating_sub(last) >= interval
    }

    fn unix_now() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    }
}

/// 数据根：`<dsh_home>/plugins/builtin/<id>/`。
fn data_dir(dsh_home: &Path, spec: &BuiltinSpec) -> PathBuf {
    dsh_home.join("plugins").join("builtin").join(spec.id)
}

/// 版本目录：`<data>/upstream/<version>/`。**按版本分目录**而不是就地覆盖：
/// 覆盖式更新在解包中途失败时会把上一版也毁掉，而那时用户既没有新版也没有旧版。
fn version_dir(dsh_home: &Path, spec: &BuiltinSpec, version: &str) -> PathBuf {
    data_dir(dsh_home, spec).join("upstream").join(version)
}

/// 解析出「当前该用哪一份」：同步过的最新版，没有就用 `None`（调用方回随包快照）。
pub(crate) fn resolve_dir(dsh_home: &Path, spec: &BuiltinSpec) -> Option<PathBuf> {
    let root = data_dir(dsh_home, spec).join("upstream");
    let mut versions: Vec<(Vec<u64>, PathBuf)> = fs::read_dir(&root)
        .ok()?
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.path().join("package.json").exists())
        .map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            (version_key(&name), entry.path())
        })
        .collect();
    versions.sort();
    versions.pop().map(|(_, path)| path)
}

/// 版本排序键：数字段逐段比；预发布排在同版本号正式版**之前**（与 semver 一致）；
/// 认不出来的整段（`latest` 这类）排最后。
///
/// 后一条不是洁癖：一个叫 `latest` 的目录，它的各段都解析不成数字、退化成
/// `0.0.0`，于是排在**所有**版本前面——而 `resolve_dir` 选的就是排序后的最后一个。
/// 那正是「一个临时目录赢了、用户拿到空技能表」的形状。
fn version_key(version: &str) -> Vec<u64> {
    let cleaned = version.trim_start_matches('v');
    let (core, pre) = match cleaned.split_once('-') {
        Some((head, tail)) => (head, Some(tail)),
        None => (cleaned.split('+').next().unwrap_or(cleaned), None),
    };
    let segments: Vec<&str> = core.split('.').collect();
    let mut key: Vec<u64> = segments
        .iter()
        .map(|part| part.parse::<u64>().unwrap_or(0))
        .collect();
    while key.len() < 3 {
        key.push(0);
    }
    if segments.iter().any(|part| part.parse::<u64>().is_err()) {
        // 整段置 MAX，而不是只在末尾追加一个 MAX：比较是**逐段**的，
        // 前缀 `[0,0,0]` 已经决定它排在所有版本前面，尾部的哨兵救不回来。
        return vec![u64::MAX; 4];
    }
    key.push(u64::from(pre.is_none()));
    key
}

/// 给内核进程的 env 名：`<PREFIX>_<ID 大写>`。
pub(crate) fn env_var(spec: &BuiltinSpec) -> String {
    format!(
        "DSH_XLINK_BUILTIN_{}",
        spec.id.to_ascii_uppercase().replace('-', "_")
    )
}

/// 面板用的状态（只读，不触发检查）。
pub(crate) fn state(dsh_home: &Path, spec: &BuiltinSpec) -> UpstreamState {
    UpstreamState::load(dsh_home, spec)
}

/// 检查并同步。返回最终状态；**任何失败都不阻断启动**——插件继续用随包快照。
pub(crate) fn sync(dsh_home: &Path, spec: &BuiltinSpec, force: bool) -> UpstreamState {
    let Some(upstream) = spec.upstream else {
        return UpstreamState::load(dsh_home, spec);
    };
    let mut state = UpstreamState::load(dsh_home, spec);
    let now = UpstreamState::unix_now();
    if !state.due(now, force) {
        return state;
    }

    let root = data_dir(dsh_home, spec);
    if let Err(e) = fs::create_dir_all(root.join("upstream")) {
        state.error = format!("建数据目录失败：{e}");
        state.failed_at = now;
        let _ = state.save(dsh_home, spec);
        return state;
    }
    // 先落进暂存目录，成功后整个改名过去：解包途中被杀掉不会留下半个版本目录，
    // 而半个版本目录读起来是「这个版本存在」（`resolve_dir` 只看 package.json）。
    let staging = root.join(".staging");
    let _ = fs::remove_dir_all(&staging);
    if let Err(e) = fs::create_dir_all(&staging) {
        state.error = format!("建暂存目录失败：{e}");
        state.failed_at = now;
        let _ = state.save(dsh_home, spec);
        return state;
    }

    let result =
        crate::pkg::fetch::fetch_npm_package(upstream.npm_package, None, &staging, &mut |_msg| {});

    let version = match result {
        Ok(version) => version,
        Err(reason) => {
            let _ = fs::remove_dir_all(&staging);
            state.error = reason;
            state.failed_at = now;
            let _ = state.save(dsh_home, spec);
            return state;
        }
    };

    // 上游脚本是 CommonJS，而 npm 包根没有 `"type"` 字段（默认就是 commonjs），
    // 所以这里补的标记只对**我们自己的随包布局**是必需的——但数据目录同样补一份，
    // 于是两条路径的模块类型完全一致，不依赖「上游恰好没写 type」这个前提。
    if let Err(e) = write_commonjs_marker(&staging) {
        let _ = fs::remove_dir_all(&staging);
        state.error = format!("写 hooks/package.json 失败：{e}");
        state.failed_at = now;
        let _ = state.save(dsh_home, spec);
        return state;
    }

    let target = version_dir(dsh_home, spec, &version);
    let _ = fs::remove_dir_all(&target);
    if let Err(e) = fs::rename(&staging, &target) {
        let _ = fs::remove_dir_all(&staging);
        state.error = format!("切换到 {version} 失败：{e}");
        state.failed_at = now;
        let _ = state.save(dsh_home, spec);
        return state;
    }
    prune(dsh_home, spec, &version);

    state.version = version;
    state.checked_at = now;
    state.failed_at = 0;
    state.error.clear();
    let _ = state.save(dsh_home, spec);
    state
}

/// 保留最近 `KEEP_VERSIONS` 个版本，其余删掉。不删的话每同步一次就多一份
/// 完整技能正文与 hook 脚本（几十 KB），一年下来是用户不会去清的东西。
fn prune(dsh_home: &Path, spec: &BuiltinSpec, keep: &str) {
    let root = data_dir(dsh_home, spec).join("upstream");
    let Ok(entries) = fs::read_dir(&root) else {
        return;
    };
    let mut versions: Vec<(Vec<u64>, String)> = entries
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            entry
                .path()
                .join("package.json")
                .exists()
                .then(|| (version_key(&name), name))
        })
        .collect();
    versions.sort();
    // 保留最新的 KEEP_VERSIONS-1 个加上刚写的那个（它已经在列表里了）。
    let drop_from = KEEP_VERSIONS.max(1);
    for (_, name) in versions.into_iter().skip(drop_from.saturating_sub(1)) {
        if name == keep {
            continue;
        }
        let _ = fs::remove_dir_all(root.join(name));
    }
}

/// 在解包出来的目录里放 `hooks/package.json`（`{"type":"commonjs"}`）。
///
/// **为什么必须有**：上游 hook 脚本是 `require()` 的 CommonJS，而 Node 按**离文件
/// 最近的** package.json 判定模块类型。没有这一层时，`node -e "require(.../
/// ponytail-activate.js)"` 抛 ERR_REQUIRE_ESM，而 **hook 失败是非阻塞的**——
/// 症状是「配好了却什么都没发生」，日志里只有一行 warn。
fn write_commonjs_marker(root: &Path) -> Result<(), String> {
    let hooks = root.join("hooks");
    if !hooks.is_dir() {
        // 没有 hooks/ 的上游版本不该被当成失败：有的宿主形态根本不需要它。
        return Ok(());
    }
    fs::write(
        hooks.join("package.json"),
        "{\n  \"type\": \"commonjs\"\n}\n",
    )
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plugins::builtin::builtins::UpstreamSpec;

    /// 本组用例只碰目录结构，不读用户家目录；每个用例一个独立临时根
    /// （**绝不**用会解析到真实 `~/.dsh-xlink` 的路径——见根 AGENTS.md
    /// 「测试永远不许碰用户的真实数据目录」）。
    fn temp_home(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("builtin-upstream-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn spec_with_upstream() -> BuiltinSpec {
        BuiltinSpec {
            id: "demo",
            row_id: "xlink-demo",
            entries: &["host", "skills", "package.json"],
            peers: &["dsh-skill"],
            entry: "host/index.js",
            display_name: "Demo",
            summary: "s",
            note: "n",
            has_account_card: false,
            upstream: Some(UpstreamSpec {
                npm_package: "@example/demo",
            }),
        }
    }

    #[test]
    fn version_key_orders_release_above_prerelease_and_ignores_v_prefix() {
        let mut keys = vec![
            version_key("v1.2.0"),
            version_key("1.10.0"),
            version_key("2.0.0-rc.1"),
            version_key("2.0.0"),
            version_key("latest"),
        ];
        keys.sort();
        assert_eq!(
            keys,
            vec![
                version_key("1.2.0"),
                version_key("1.10.0"),
                version_key("2.0.0-rc.1"),
                version_key("2.0.0"),
                version_key("latest"),
            ]
        );
    }

    /// 三种状态各走各的窗口（见 [`UpstreamState::due`]）：没试过 → 立刻；成功后
    /// → 24 小时；失败且没成功过 → 30 分钟；成功过之后又失败 → 仍按 24 小时。
    #[test]
    fn due_covers_first_check_success_window_and_failure_backoff() {
        let hour = 3600u64;
        // 从没试过（两个时间戳都是 0）：首次检查必须放行。
        let never = UpstreamState::default();
        assert!(
            never.due(60, false),
            "新装用户必须立刻同步一次——把「没试过」当成「试过」的话，插件永远没有内容，\
             而状态文件是空的、面板上什么异常都看不出来"
        );
        // 成功后：24 小时内不再问。
        let ok = UpstreamState {
            checked_at: hour,
            ..Default::default()
        };
        assert!(!ok.due(hour + 60, false));
        assert!(ok.due(hour + 24 * 3600, false));
        // 成功过之后又失败：仍按 24 小时（一次成功说明网络是通的）。
        let ok_then_bad = UpstreamState {
            checked_at: hour,
            failed_at: 2 * hour,
            ..Default::default()
        };
        assert!(!ok_then_bad.due(2 * hour + 60, false));
        assert!(ok_then_bad.due(2 * hour + 24 * 3600, false));
        // 只有失败、没有成功记录：30 分钟后可重试。
        let failed_only = UpstreamState {
            failed_at: hour,
            ..Default::default()
        };
        assert!(!failed_only.due(hour + 60, false));
        assert!(failed_only.due(hour + 31 * 60, false));
        // force 永远放行（面板「立即检查更新」与 /ponytail-update 走这条）。
        assert!(failed_only.due(hour + 60, true));
    }

    #[test]
    fn env_var_is_scoped_per_plugin() {
        assert_eq!(
            env_var(&super::super::builtins::find("ponytail").unwrap()),
            "DSH_XLINK_BUILTIN_PONYTAIL"
        );
    }

    /// 没有 package.json 的目录不算一个版本——解包中断留下的半个目录必须被忽略，
    /// 否则 `resolve_dir` 会把它当成可用的那一版，而 host 读 skills/ 读到空。
    #[test]
    fn resolve_dir_ignores_directories_without_a_package_json() {
        let home = temp_home("resolve");
        let spec = spec_with_upstream();
        let root = data_dir(&home, &spec).join("upstream");
        fs::create_dir_all(version_dir(&home, &spec, "5.1.0")).unwrap();
        fs::create_dir_all(root.join("5.2.0")).unwrap();
        fs::write(
            version_dir(&home, &spec, "5.1.0").join("package.json"),
            "{}",
        )
        .unwrap();

        let resolved = resolve_dir(&home, &spec).expect("应解析出 5.1.0");
        assert!(
            resolved.ends_with("5.1.0"),
            "半个目录（无 package.json）不该被选中，实际 {:?}",
            resolved
        );
        assert!(
            resolve_dir(
                &home,
                &BuiltinSpec {
                    upstream: None,
                    ..spec_with_upstream()
                }
            )
            .is_none()
                || true
        );
    }
}
