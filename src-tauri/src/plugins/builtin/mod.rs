//! 内置插件的离线交付层：指纹、物化、接线事务与状态查询。
//!
//! 设计与验收见
//! [docs/features/extensions/openai-oauth-design.md](../../../../docs/features/extensions/openai-oauth-design.md)
//! 与开发计划同目录；接线配方的三版本实测记录见 P0 调查 §5/§6。
//!
//! 分工：
//! - [`builtins`]：**内置插件登记表**（本版本带哪几个、各自物化什么）——新增
//!   内置插件只改这一处
//! - [`materialize`]：内核依赖指纹、运行时文件集物化、peer 链接
//! - [`wiring`]：实例 `cordis.patch.yml` 自有 insert 行的精确增删
//! - [`cmd`]：`builtin_status` / `builtin_set_enabled` 命令层
//! - 本模块：启停编排与只读状态探针
//!
//! 本阶段（P1 中段）交付的是**停止状态的启停事务**：`ensure_wired` /
//! `ensure_unwired` 做到「物化 + 自有接线提交」，不碰账号与资源删除
//! （设计 §8：关闭插件保留授权与资源）。调用方（命令层）负责实例运行
//! 判据与变更前快照。

use std::fs;
use std::path::{Path, PathBuf};

pub(crate) mod bridge;
pub(crate) mod bridge_credential;
pub(crate) mod builtins;
pub(crate) mod cmd;
pub(crate) mod materialize;
pub(crate) mod runtime;
pub(crate) mod state;
pub(crate) mod upstream;
pub(crate) mod wiring;

use builtins::BuiltinSpec;

/// 桥接 env 名（开发计划 §5：地址与令牌只经子进程环境传入；与
/// plugins/openai-oauth/host/constants.js 的同名常量一一对应）。
///
/// **这一对是 openai-oauth 专属的**，不是登记表里的一项：它们是「壳把 OAuth
/// 桥接服务递给子进程」的通道，只有那个插件有桥接可递。放在本模块而不是
/// `registry`，是为了让「随应用交付的插件都有哪几项」这件事只由 registry
/// 回答——多一项无关的东西进去，下次读表的人就得判断它算不算一份登记。
pub(crate) const BRIDGE_URL_ENV: &str = "DSH_XLINK_OPENAI_BRIDGE_URL";
pub(crate) const BRIDGE_TOKEN_ENV: &str = "DSH_XLINK_OPENAI_BRIDGE_TOKEN";

/// 启用事务的结果。
#[allow(
    dead_code,
    reason = "接线记录的物化目录与「本次是否追加了行」两项：物化路径…"
)]
pub(crate) struct Wired {
    pub(crate) fingerprint: String,
    pub(crate) plugin_version: String,
    /// 物化目录（指纹专属）。
    pub(crate) dir: PathBuf,
    /// 本次是否追加了接线行（false = 已在，幂等）。
    pub(crate) row_added: bool,
}

/// 只读状态（命令层载荷的原型）。
///
/// `load_state` 是开发计划 §4.1 六态的 **P1 子集**：`disabled`（无接线行）、
/// `prepared`（已接线且当前指纹已物化）、`incompatible`（已接线但当前内核
/// 指纹没有物化产物——重跑启用即修复）。`active` 需要运行期 Host 握手、
/// `quarantined`/`failed` 需要事务与隔离记录，均属 P2+。
pub(crate) struct Status {
    pub(crate) wired: bool,
    pub(crate) requested_enabled: bool,
    pub(crate) load_state: &'static str,
    pub(crate) kernel_fingerprint: Option<String>,
    pub(crate) materialized: Vec<materialize::MaterializedEntry>,
    /// 状态文件损坏时的说明（损坏不等于关闭，如实上报）。
    pub(crate) state_error: Option<String>,
}

fn patch_path(dsh_home: &Path, profile: &str) -> PathBuf {
    dsh_home
        .join("profiles")
        .join(profile)
        .join("cordis.patch.yml")
}

fn read_patch(dsh_home: &Path, profile: &str) -> Result<String, String> {
    let path = patch_path(dsh_home, profile);
    match fs::read_to_string(&path) {
        Ok(text) => Ok(text),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(error) => Err(format!(
            "读取接线文件失败（{}）：{error}；请确认磁盘可写后重试",
            path.display()
        )),
    }
}

/// 启用：物化（幂等）→ 追加自有接线行（幂等）。失败时接线保持原状。
pub(crate) fn ensure_wired(
    spec: &BuiltinSpec,
    plugin_source: &Path,
    dsh_home: &Path,
    profile: &str,
    kernel_root: &Path,
) -> Result<Wired, String> {
    let version = materialize::plugin_version(plugin_source)?;
    let fingerprint = materialize::kernel_fingerprint(kernel_root)?;
    let dir = dsh_home
        .join(spec.instance_base_segment())
        .join(&version)
        .join(&fingerprint)
        .join("compat-a");
    materialize::materialize(
        spec,
        plugin_source,
        // 有上游的插件：内容从数据目录里已同步的那一版取。`None` 时
        // `materialize` 会明确报「还没同步过」——而不是物化出一个只有适配层的
        // 空壳，空壳的症状是工作台里没有技能而界面上看不出任何异常。
        crate::plugins::builtin::upstream::resolve_dir(dsh_home, spec).as_deref(),
        &dir,
        kernel_root,
        &version,
        &fingerprint,
    )?;

    let entry_rel = materialize::relative_entry(
        &dsh_home.join("profiles").join(profile),
        &dir.join(spec.entry),
    );
    let existing = read_patch(dsh_home, profile)?;
    // 已在（id + 当前路径）→ 完全不动；路径变了（指纹切换）才先摘旧行再落新行。
    let next = if wiring::row_present(&existing, spec.row_id, &entry_rel) {
        None
    } else {
        let text = wiring::remove_row(&existing, spec.row_id)?.unwrap_or(existing);
        wiring::ensure_row(&text, spec.row_id, &entry_rel)?
    };
    let row_added = next.is_some();
    if let Some(next) = next {
        let path = patch_path(dsh_home, profile);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)
                .map_err(|e| format!("创建 profile 目录失败（{}）：{e}", parent.display()))?;
        }
        crate::shell::process::atomic_write(&path, next.as_bytes()).map_err(|e| {
            format!(
                "写入接线文件失败（{}）：{e}；未写入的行不会生效，可安全重试",
                path.display()
            )
        })?;
    }
    Ok(Wired {
        fingerprint,
        plugin_version: version,
        dir,
        row_added,
    })
}

/// 停用：摘除自有接线行（幂等）。**不删**物化资源与账号（设计 §8）。
pub(crate) fn ensure_unwired(
    spec: &BuiltinSpec,
    dsh_home: &Path,
    profile: &str,
) -> Result<bool, String> {
    let existing = read_patch(dsh_home, profile)?;
    let Some(next) = wiring::remove_row(&existing, spec.row_id)? else {
        return Ok(false);
    };
    let path = patch_path(dsh_home, profile);
    crate::shell::process::atomic_write(&path, next.as_bytes()).map_err(|e| {
        format!(
            "写入接线文件失败（{}）：{e}；接线未变化，可安全重试",
            path.display()
        )
    })?;
    Ok(true)
}

/// 当前壳 mode 与给定 profile 的启用意图（`usage::subscription` 的 OpenAI
/// 分区门禁用）。状态文件读不出来（损坏）时**按开处理**：读不出意图不等于
/// 用户关了它，隐藏一个原本看得见的分区，比多显示一块带错误说明的分区更糟；
/// 损坏本身由插件面板的 `state_error` 提示，不在消费侧重复报。
pub(crate) fn requested_enabled(spec: &BuiltinSpec, dsh_home: &Path, profile: &str) -> bool {
    match state::load(dsh_home, spec) {
        Ok(entries) => entries
            .get(&state::state_key(
                crate::shell::settings::current_mode().as_str(),
                profile,
            ))
            .is_some_and(|entry| entry.requested_enabled),
        Err(_) => true,
    }
}

/// 只读状态探针（命令层用；不做任何写入）。`mode` 用于读分键的启用意图。
pub(crate) fn probe_status(
    spec: &BuiltinSpec,
    dsh_home: &Path,
    profile: &str,
    kernel_root: Option<&Path>,
    mode: &str,
) -> Status {
    let materialized = materialize::find_materialized(&dsh_home.join(spec.instance_base_segment()));
    let kernel_fingerprint =
        kernel_root.and_then(|root| materialize::kernel_fingerprint(root).ok());
    let wired = read_patch(dsh_home, profile)
        .map(|text| text.contains(spec.row_id))
        .unwrap_or(false);
    let load_state = if !wired {
        "disabled"
    } else if kernel_fingerprint
        .as_ref()
        .is_some_and(|fp| !materialized.iter().any(|entry| &entry.fingerprint == fp))
    {
        "incompatible"
    } else {
        "prepared"
    };
    let (requested_enabled, state_error) = match state::load(dsh_home, spec) {
        Ok(entries) => (
            entries
                .get(&state::state_key(mode, profile))
                .is_some_and(|entry| entry.requested_enabled),
            None,
        ),
        Err(error) => (false, Some(error)),
    };
    Status {
        wired,
        requested_enabled,
        load_state,
        kernel_fingerprint,
        materialized,
        state_error,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// openai-oauth 的 spec。整组测试都是**围绕这一个插件**写的（它有 client
    /// 与 locales，是文件集最"胖"的那个），ponytail 那条路径由
    /// `materialize::tests::a_builtin_without_client_dirs_materializes_its_own_entry_set`
    /// 与本模块的 `two_builtins_independent_rows_and_state` 各守一半。
    fn spec() -> &'static BuiltinSpec {
        builtins::find(builtins::OPENAI_OAUTH).expect("openai-oauth 必须在登记表里")
    }

    /// 单一根目录下摆 home/source/kernel 三个子树，结束时整根删掉。
    struct Scratch(PathBuf);

    impl Scratch {
        fn new(tag: &str) -> Self {
            let root =
                std::env::temp_dir().join(format!("oop-builtin-{}-{}", std::process::id(), tag));
            let _ = fs::remove_dir_all(&root);
            let home = root.join("home");
            fs::create_dir_all(home.join("profiles/web")).unwrap();
            fs::create_dir_all(root.join("workspace")).unwrap();
            Self(root)
        }

        fn home(&self) -> PathBuf {
            self.0.join("home")
        }

        /// 夹具源目录**按 spec 的文件集生成**，不再写死 `host/client/locales`：
        /// 写死的话登记表加一个文件集不同的插件时，这个夹具会替它造出并不存在
        /// 的目录，于是「物化只复制自己那份」永远验不到。
        fn source(&self, spec: &BuiltinSpec) -> PathBuf {
            let dir = self.0.join(format!("source-{}", spec.id));
            for entry in spec.entries {
                if *entry == "package.json" {
                    fs::write(
                        dir.join("package.json"),
                        format!("{{\"name\":\"{}\",\"version\":\"0.1.0\"}}", spec.row_id),
                    )
                    .unwrap();
                } else {
                    fs::create_dir_all(dir.join(entry)).unwrap();
                    fs::write(dir.join(entry).join("keep.js"), "// keep").unwrap();
                }
            }
            fs::create_dir_all(dir.join("host")).unwrap();
            fs::write(dir.join("host/index.js"), "export {}").unwrap();
            // 有上游的插件需要一份「已同步」的内容，否则物化会（正确地）拒绝：
            // 那份拒绝是本次修掉的洞——空壳在界面上看不出任何异常。
            if spec.upstream.is_some() {
                crate::plugins::builtin::upstream::seed_fixture(&self.home(), spec);
            }
            dir
        }

        fn kernel(&self) -> PathBuf {
            let dir = self.0.join("kernel");
            for (pkg, version) in [("dsh", "0.2.1-alpha.1"), ("cordis", "4.0.5-alpha.1")] {
                let manifest_dir = dir.join("node_modules/@deepseek-ai").join(pkg);
                fs::create_dir_all(&manifest_dir).unwrap();
                fs::write(
                    manifest_dir.join("package.json"),
                    format!("{{\"version\":\"{version}\"}}"),
                )
                .unwrap();
            }
            // 全部内置插件的 peer 都摆出来：夹具不该因为登记表多了一个插件
            // 就失败——那会让「加第二个内置插件」这件事先撞上一个无关的红。
            for builtin in builtins::BUILTINS {
                for pkg in builtin.peers {
                    fs::create_dir_all(dir.join("node_modules/@deepseek-ai").join(pkg)).unwrap();
                }
            }
            dir
        }
    }

    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn wire_unwire_roundtrip_and_idempotence() {
        let scratch = Scratch::new("rt");
        let spec = spec();
        let source = scratch.source(spec);
        let kernel = scratch.kernel();
        let home = scratch.home();

        let wired = ensure_wired(spec, &source, &home, "web", &kernel).unwrap();
        assert!(wired.row_added);
        assert!(wired.dir.join("host/index.js").is_file());
        let patch = fs::read_to_string(patch_path(&home, "web")).unwrap();
        assert!(patch.contains("id: xlink-openai-oauth"));
        assert_eq!(wired.fingerprint, "dsh-0.2.1-alpha.1-cordis-4.0.5-alpha.1");

        // 幂等：再来一次不重复追加。
        let again = ensure_wired(spec, &source, &home, "web", &kernel).unwrap();
        assert!(!again.row_added);
        assert_eq!(patch, fs::read_to_string(patch_path(&home, "web")).unwrap());

        // 探针：已接线 + 已物化 + 指纹 + 加载子集状态（意图未写 → false/disabled 语义由 wiring 决定）。
        let status = probe_status(spec, &home, "web", Some(&kernel), "release");
        assert!(status.wired);
        assert!(!status.requested_enabled);
        assert_eq!(status.load_state, "prepared");
        assert_eq!(status.materialized.len(), 1);
        assert_eq!(
            status.kernel_fingerprint.as_deref(),
            Some("dsh-0.2.1-alpha.1-cordis-4.0.5-alpha.1")
        );

        // 意图落盘后：requestedEnabled = true。
        let mut entries = std::collections::HashMap::new();
        entries.insert(
            state::state_key("release", "web"),
            state::StateEntry {
                requested_enabled: true,
                updated_at_ms: 1,
                plugin_version: Some(wired.plugin_version.clone()),
                fingerprint: Some(wired.fingerprint.clone()),
            },
        );
        state::save(&home, spec, &entries).unwrap();
        assert!(probe_status(spec, &home, "web", Some(&kernel), "release").requested_enabled);
        // 分键：dev 壳读不到 release 的意图。
        assert!(!probe_status(spec, &home, "web", Some(&kernel), "dev").requested_enabled);

        // 停用：行消失，资源保留。
        assert!(ensure_unwired(spec, &home, "web").unwrap());
        let after = fs::read_to_string(patch_path(&home, "web")).unwrap();
        assert!(!after.contains("xlink-openai-oauth"));
        assert!(wired.dir.join("host/index.js").is_file());
        assert!(!ensure_unwired(spec, &home, "web").unwrap());
        let stopped = probe_status(spec, &home, "web", None, "release");
        assert!(!stopped.wired);
        assert_eq!(stopped.load_state, "disabled");
    }

    /// 两个内置插件同时开着时：各接各的行、各记各的物化目录、意图各存各的
    /// 文件，停掉一个不影响另一个。
    ///
    /// 这是整条通道从「一家写死」改成 N 家的**接线形状**判据。三个症状在
    /// 写死时代里是同一段代码的必然结果，所以这三条断言各自都曾经没有对应的
    /// 代码路径可测——现在有了，就必须钉住：任何一个插件 id 被写死回来
    /// （例如 `probe_status` 又去读固定的那份 state.json），立刻转红。
    #[test]
    fn two_builtins_independent_rows_and_state() {
        let scratch = Scratch::new("two");
        let openai = spec();
        let ponytail = builtins::find(builtins::PONYTAIL).expect("ponytail 必须在登记表里");
        let kernel = scratch.kernel();
        let home = scratch.home();

        ensure_wired(openai, &scratch.source(openai), &home, "web", &kernel).unwrap();
        ensure_wired(ponytail, &scratch.source(ponytail), &home, "web", &kernel).unwrap();

        // 两行都在。
        let patch = fs::read_to_string(patch_path(&home, "web")).unwrap();
        assert!(patch.contains("id: xlink-openai-oauth"), "{patch}");
        assert!(patch.contains("id: xlink-ponytail"), "{patch}");

        // 物化目录分家：各自的 base segment 下各有一份。
        let openai_dir = home.join(openai.instance_base_segment());
        let ponytail_dir = home.join(ponytail.instance_base_segment());
        assert!(openai_dir.is_dir() && ponytail_dir.is_dir());
        assert!(openai_dir.join("0.1.0").is_dir());
        assert!(ponytail_dir.join("0.1.0").is_dir());

        // 探针互不串：停用 ponytail 后 openai 仍是 prepared。
        // 键按**当前壳 mode** 分（`requested_enabled` 内部也读 current_mode）：
        // 这里写死 "release" 而在 debug 构型下 current_mode 是 "dev"，
        // 断言会红，而红的原因与「两个插件是否独立」无关。
        let mode = crate::shell::settings::current_mode().as_str();
        let mut entries = std::collections::HashMap::new();
        for builtin in [openai, ponytail] {
            entries.insert(
                state::state_key(mode, "web"),
                state::StateEntry {
                    requested_enabled: true,
                    updated_at_ms: 1,
                    plugin_version: Some("0.1.0".into()),
                    fingerprint: Some("dsh-0.2.1-alpha.1-cordis-4.0.5-alpha.1".into()),
                },
            );
            state::save(&home, builtin, &entries).unwrap();
        }
        assert!(ensure_unwired(ponytail, &home, "web").unwrap());
        let after = fs::read_to_string(patch_path(&home, "web")).unwrap();
        assert!(
            after.contains("id: xlink-openai-oauth"),
            "停 ponytail 不该摘 openai 的行"
        );
        assert!(!after.contains("id: xlink-ponytail"));
        assert_eq!(
            probe_status(openai, &home, "web", Some(&kernel), mode).load_state,
            "prepared"
        );
        assert_eq!(
            probe_status(ponytail, &home, "web", Some(&kernel), mode).load_state,
            "disabled"
        );
        assert!(requested_enabled(openai, &home, "web"));
    }

    /// 门禁判据：没写过意图 → 关；写过（按当前壳 mode / profile 分键）→ 开；
    /// 状态文件损坏 → fail-open（按开，见 `requested_enabled` 文档）。
    #[test]
    fn requested_enabled_is_keyed_and_fails_open() {
        let scratch = Scratch::new("gate");
        let spec = spec();
        let home = scratch.home();
        let mode = crate::shell::settings::current_mode().as_str().to_string();

        assert!(!requested_enabled(spec, &home, "web"));
        let mut entries = std::collections::HashMap::new();
        entries.insert(
            state::state_key(&mode, "web"),
            state::StateEntry {
                requested_enabled: true,
                updated_at_ms: 1,
                plugin_version: None,
                fingerprint: None,
            },
        );
        state::save(&home, spec, &entries).unwrap();
        assert!(requested_enabled(spec, &home, "web"));

        fs::write(state::state_path(&home, spec), "{ not json").unwrap();
        assert!(
            requested_enabled(spec, &home, "web"),
            "损坏必须按开处理，而不是把分区藏掉"
        );
    }

    #[test]
    fn probe_reports_incompatible_when_fingerprint_drifts() {
        let scratch = Scratch::new("drift");
        let spec = spec();
        let source = scratch.source(spec);
        let kernel = scratch.kernel();
        let home = scratch.home();
        ensure_wired(spec, &source, &home, "web", &kernel).unwrap();

        // 内核树换成新指纹（模拟版本切换），物化产物还停在旧指纹上。
        let manifest_dir = kernel.join("node_modules/@deepseek-ai/cordis");
        fs::write(manifest_dir.join("package.json"), "{\"version\":\"4.1.0\"}").unwrap();
        let status = probe_status(spec, &home, "web", Some(&kernel), "release");
        assert!(status.wired);
        assert_eq!(status.load_state, "incompatible");

        // 重跑启用即修复：按新指纹重新物化并改写接线行。
        ensure_wired(spec, &source, &home, "web", &kernel).unwrap();
        assert_eq!(
            probe_status(spec, &home, "web", Some(&kernel), "release").load_state,
            "prepared"
        );
    }

    #[test]
    fn wire_preserves_user_rows() {
        let scratch = Scratch::new("user");
        let spec = spec();
        let source = scratch.source(spec);
        let kernel = scratch.kernel();
        let home = scratch.home();
        let user = "# 用户\n- insert:\n    - id: mine\n      name: 'mine'\n";
        fs::write(patch_path(&home, "web"), user).unwrap();

        ensure_wired(spec, &source, &home, "web", &kernel).unwrap();
        ensure_unwired(spec, &home, "web").unwrap();

        assert_eq!(fs::read_to_string(patch_path(&home, "web")).unwrap(), user);
    }
}
