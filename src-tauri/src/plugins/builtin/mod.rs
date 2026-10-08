//! 内嵌 openai-oauth 插件的离线交付层：指纹、物化、接线事务与状态查询。
//!
//! 设计与验收见
//! [docs/features/extensions/openai-oauth-design.md](../../../../docs/features/extensions/openai-oauth-design.md)
//! 与开发计划同目录；接线配方的三版本实测记录见 P0 调查 §5/§6。
//!
//! 分工：
//! - [`materialize`]：内核依赖指纹、运行时文件集物化、peer 链接
//! - [`wiring`]：实例 `cordis.patch.yml` 自有 insert 行的精确增删
//! - [`cmd`]：`builtin_openai_status` 命令层（只读）
//! - 本模块：稳定标识、启停编排与只读状态探针
//!
//! 本阶段（P1 中段）交付的是**停止状态的启停事务**：`ensure_wired` /
//! `ensure_unwired` 做到「物化 + 自有接线提交」，不碰账号与资源删除
//! （设计 §8：关闭插件保留授权与资源）。调用方（命令层）负责实例运行
//! 判据与变更前快照。

use std::fs;
use std::path::{Path, PathBuf};

pub(crate) mod cmd;
pub(crate) mod materialize;
pub(crate) mod wiring;

/// 接线行 id = settingsNs = client 卡片 key（P0 调查 §6 实测）。
pub(crate) const ENTRY_ID: &str = wiring::ROW_ID;
/// 实例内物化根：`<DSH_HOME>/extensions/builtin/openai-oauth/`（开发计划 §3）。
pub(crate) const INSTANCE_BASE_SEGMENT: &str = "extensions/builtin/openai-oauth";

/// 启用事务的结果。
pub(crate) struct Wired {
    pub(crate) fingerprint: String,
    pub(crate) plugin_version: String,
    /// 物化目录（指纹专属）。
    pub(crate) dir: PathBuf,
    /// 本次是否追加了接线行（false = 已在，幂等）。
    pub(crate) row_added: bool,
}

/// 只读状态（命令层载荷的原型）。
pub(crate) struct Status {
    pub(crate) wired: bool,
    pub(crate) kernel_fingerprint: Option<String>,
    pub(crate) materialized: Vec<materialize::MaterializedEntry>,
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
    plugin_source: &Path,
    dsh_home: &Path,
    profile: &str,
    kernel_root: &Path,
) -> Result<Wired, String> {
    let version = materialize::plugin_version(plugin_source)?;
    let fingerprint = materialize::kernel_fingerprint(kernel_root)?;
    let dir = dsh_home
        .join(INSTANCE_BASE_SEGMENT)
        .join(&version)
        .join(&fingerprint)
        .join("compat-a");
    materialize::materialize(plugin_source, &dir, kernel_root, &version, &fingerprint)?;

    let entry_rel = materialize::relative_entry(
        &dsh_home.join("profiles").join(profile),
        &dir.join("host").join("index.js"),
    );
    let existing = read_patch(dsh_home, profile)?;
    // 已在（id + 当前路径）→ 完全不动；路径变了（指纹切换）才先摘旧行再落新行。
    let next = if wiring::row_present(&existing, &entry_rel) {
        None
    } else {
        let text = wiring::remove_row(&existing)?.unwrap_or(existing);
        wiring::ensure_row(&text, &entry_rel)?
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
pub(crate) fn ensure_unwired(dsh_home: &Path, profile: &str) -> Result<bool, String> {
    let existing = read_patch(dsh_home, profile)?;
    let Some(next) = wiring::remove_row(&existing)? else {
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

/// 只读状态探针（命令层用；不做任何写入）。
pub(crate) fn probe_status(dsh_home: &Path, profile: &str, kernel_root: Option<&Path>) -> Status {
    let materialized = materialize::find_materialized(&dsh_home.join(INSTANCE_BASE_SEGMENT));
    let wired = read_patch(dsh_home, profile)
        .map(|text| text.contains(wiring::ROW_ID))
        .unwrap_or(false);
    Status {
        wired,
        kernel_fingerprint: kernel_root.and_then(|root| materialize::kernel_fingerprint(root).ok()),
        materialized,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

        fn source(&self) -> PathBuf {
            let dir = self.0.join("source");
            for sub in ["host", "client", "locales"] {
                fs::create_dir_all(dir.join(sub)).unwrap();
            }
            fs::write(
                dir.join("package.json"),
                "{\"name\":\"xlink-openai-oauth\",\"version\":\"0.1.0\"}",
            )
            .unwrap();
            fs::write(dir.join("host/index.js"), "export {}").unwrap();
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
            fs::create_dir_all(dir.join("node_modules/@deepseek-ai/dsh-llm")).unwrap();
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
        let source = scratch.source();
        let kernel = scratch.kernel();
        let home = scratch.home();

        let wired = ensure_wired(&source, &home, "web", &kernel).unwrap();
        assert!(wired.row_added);
        assert!(wired.dir.join("host/index.js").is_file());
        let patch = fs::read_to_string(patch_path(&home, "web")).unwrap();
        assert!(patch.contains("id: xlink-openai-oauth"));
        assert_eq!(wired.fingerprint, "dsh-0.2.1-alpha.1-cordis-4.0.5-alpha.1");

        // 幂等：再来一次不重复追加。
        let again = ensure_wired(&source, &home, "web", &kernel).unwrap();
        assert!(!again.row_added);
        assert_eq!(patch, fs::read_to_string(patch_path(&home, "web")).unwrap());

        // 探针：已接线 + 已物化 + 指纹。
        let status = probe_status(&home, "web", Some(&kernel));
        assert!(status.wired);
        assert_eq!(status.materialized.len(), 1);
        assert_eq!(
            status.kernel_fingerprint.as_deref(),
            Some("dsh-0.2.1-alpha.1-cordis-4.0.5-alpha.1")
        );

        // 停用：行消失，资源保留。
        assert!(ensure_unwired(&home, "web").unwrap());
        let after = fs::read_to_string(patch_path(&home, "web")).unwrap();
        assert!(!after.contains("xlink-openai-oauth"));
        assert!(wired.dir.join("host/index.js").is_file());
        assert!(!ensure_unwired(&home, "web").unwrap());
        assert!(!probe_status(&home, "web", None).wired);
    }

    #[test]
    fn wire_preserves_user_rows() {
        let scratch = Scratch::new("user");
        let source = scratch.source();
        let kernel = scratch.kernel();
        let home = scratch.home();
        let user = "# 用户\n- insert:\n    - id: mine\n      name: 'mine'\n";
        fs::write(patch_path(&home, "web"), user).unwrap();

        ensure_wired(&source, &home, "web", &kernel).unwrap();
        ensure_unwired(&home, "web").unwrap();

        assert_eq!(fs::read_to_string(patch_path(&home, "web")).unwrap(), user);
    }
}
