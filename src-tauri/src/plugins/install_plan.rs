//! 预检与正式安装之间的不可变计划（2026-11-01 OPT-06）。
//!
//! 预检通过后把「验过的到底是什么」固化成一份计划：固定的取源 spec（npm 精确
//! 版本 / git 完整 commit）、产物树摘要、内核版本、实例、profile、端口、平台与
//! Node 版本、有效期。应用阶段只认计划缓存里的这一份，先逐项校验，再复用已保存的物料
//! 安装，装完重新算摘要与计划比对；任何一项对不上都拒绝应用，不把「可能不是
//! 验过的那个」说成安装成功。
//!
//! 计划与已验证物料保存在当前壳模式的专用目录，按有效期和数量裁剪。
//! 正式应用不再次取源或构建。

use crate::shell::settings::Settings;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

pub const SCHEMA_VERSION: u32 = 2;
/// 计划有效期：预检后 30 分钟内可应用。超过后环境可能已经变化，要求重新预检。
pub const TTL_MS: u64 = 30 * 60 * 1000;
pub(super) const MAX_PLANS: usize = 16;

/// 一次预检产出的不可变安装计划。字段全部进入 plan_id 的摘要，改动任意一项
/// 都会让计划 ID 对不上。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InstallPlan {
    pub schema_version: u32,
    pub plan_id: String,
    /// 预检请求里的原始 spec（已剥 URL 凭据）。
    pub spec: String,
    /// 应用阶段实际使用的固定 spec：npm 为 name@精确版本，git 为 url#完整 commit。
    pub pinned_spec: String,
    pub source_kind: String,
    pub requested_pin: String,
    pub resolved_version: String,
    pub resolved_commit: String,
    pub integrity: String,
    /// 预检时中央库插件树的 SHA-256（含依赖树，排除外壳 id 标记）。
    pub content_digest: String,
    pub artifact_item_digest: String,
    pub kernel_fingerprint: String,
    pub kernel_version: String,
    pub family: String,
    pub target_instance: String,
    pub profile: String,
    pub port: u16,
    pub materialize: String,
    pub platform: String,
    pub arch: String,
    pub node_version: String,
    pub created_at_ms: u64,
    pub expires_at_ms: u64,
    pub evidence_run_id: String,
}

impl InstallPlan {
    /// 计算 plan_id。先把 plan_id 清空，摘要只覆盖其余字段，避免自引用。
    pub fn finalize(mut self) -> Self {
        self.plan_id.clear();
        self.plan_id = plan_digest(&self);
        self
    }

    pub fn verify_id(&self) -> Result<(), String> {
        let mut copy = self.clone();
        copy.plan_id.clear();
        if plan_digest(&copy) == self.plan_id {
            Ok(())
        } else {
            Err("预检计划内容与计划 ID 不一致（可能被改动），已拒绝应用".to_string())
        }
    }

    pub fn pinned_spec(&self) -> Result<&str, String> {
        if self.pinned_spec.trim().is_empty() {
            Err("预检计划缺少固定安装来源，请重新预检".to_string())
        } else {
            Ok(self.pinned_spec.as_str())
        }
    }

    /// 应用前校验：计划未过期、未被改动、目标实例与环境仍与预检时一致。
    pub fn validate_for(
        &self,
        family: &str,
        target_instance: &str,
        data_dir: &Path,
        settings: &Settings,
        mode: &str,
        node_version: &str,
    ) -> Result<(), String> {
        if self.schema_version != SCHEMA_VERSION {
            return Err(format!(
                "预检计划版本 {} 与当前壳不兼容，请重新预检",
                self.schema_version
            ));
        }
        self.verify_id()?;
        let now = crate::shell::process::epoch_millis();
        if now > self.expires_at_ms {
            return Err("预检计划已过期（超过 30 分钟），请重新预检后再应用".to_string());
        }
        if self.family != family || self.target_instance != target_instance {
            return Err(format!(
                "预检计划的目标是 {}/{}，当前目标是 {family}/{target_instance}，请重新预检",
                self.family, self.target_instance
            ));
        }
        if self.profile != settings.profile {
            return Err(format!(
                "预检时的 profile 是 {}，当前是 {}，请重新预检",
                self.profile, settings.profile
            ));
        }
        if self.port != settings.port {
            return Err(format!(
                "预检时的端口是 {}，当前是 {}，请重新预检",
                self.port, settings.port
            ));
        }
        let expected_mode = if mode == "copy" { "copy" } else { "link" };
        if self.materialize != expected_mode {
            return Err(format!(
                "预检时的物化方式是 {}，当前请求是 {expected_mode}，请重新预检",
                self.materialize
            ));
        }
        if self.platform != std::env::consts::OS || self.arch != std::env::consts::ARCH {
            return Err(format!(
                "预检平台是 {}/{}，当前是 {}/{}，请重新预检",
                self.platform,
                self.arch,
                std::env::consts::OS,
                std::env::consts::ARCH
            ));
        }
        if !self.node_version.is_empty() && self.node_version != node_version {
            return Err(format!(
                "预检时的 Node 版本是 {}，当前是 {}，请重新预检",
                self.node_version, node_version
            ));
        }
        let active = crate::kernel::lifecycle::read_active(data_dir).unwrap_or_default();
        if !self.kernel_fingerprint.is_empty()
            && super::prepared::kernel_fingerprint(data_dir, &active)? != self.kernel_fingerprint
        {
            return Err("内核安装指纹已变化，请重新预检".into());
        }
        if self.kernel_version != active {
            return Err(format!(
                "预检时的内核版本是 {}，当前是 {}，请重新预检",
                self.kernel_version, active
            ));
        }
        Ok(())
    }
}

fn plan_digest(plan: &InstallPlan) -> String {
    match serde_json::to_vec(plan) {
        Ok(bytes) => hex_sha256(&bytes),
        Err(_) => String::new(),
    }
}

pub(super) fn hex_sha256(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

pub fn load(plan_id: &str) -> Option<InstallPlan> {
    super::prepared::load(plan_id)
}
pub fn forget(plan_id: &str) {
    if let Err(error) = super::prepared::forget(plan_id) {
        crate::shell::shell_events::record(
            "precheck-cache",
            &format!("计划已应用，但缓存清理失败：{error}"),
        );
    }
}

/// 计算插件树的内容摘要：按相对路径排序，逐文件哈希路径与字节；符号链接哈希
/// 链接目标，不再跟随进去。排除外壳自己写的元数据：`.dsh-id`（内容随中央库
/// id 变化）、`.dsh-source.json`（`fetchedAt` 每次取源都变）、`.dsh-build.log`
/// （每次构建的 pnpm 输出不同）。这些都不是插件产物，参与摘要会让「预检通过、
/// 应用重新取源」必然对不上（FIX-01）。来源身份/版本/commit 由应用阶段单独
/// 核对（`precheck::verify_applied_artifact`），没有因为摘要范围缩小而丢掉。
/// 读不了任何文件都算失败——摘要不完整就不能用于比对。
pub fn tree_digest(root: &Path) -> Result<String, String> {
    let mut files: Vec<(String, PathBuf)> = Vec::new();
    collect_files(root, root, &mut files)?;
    files.sort_by(|left, right| left.0.cmp(&right.0));
    let mut hasher = Sha256::new();
    for (relative, path) in files {
        hasher.update(relative.as_bytes());
        hasher.update([0]);
        let metadata = std::fs::symlink_metadata(&path)
            .map_err(|e| format!("读取 {} 失败：{e}", path.display()))?;
        if metadata.file_type().is_symlink() {
            let target = std::fs::read_link(&path)
                .map_err(|e| format!("读取链接 {} 失败：{e}", path.display()))?;
            hasher.update(b"link:");
            hasher.update(target.to_string_lossy().as_bytes());
        } else {
            hasher.update(b"file:");
            let bytes =
                std::fs::read(&path).map_err(|e| format!("读取 {} 失败：{e}", path.display()))?;
            hasher.update((bytes.len() as u64).to_le_bytes());
            hasher.update(&bytes);
        }
        hasher.update([0]);
    }
    let digest = hasher.finalize();
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        out.push_str(&format!("{byte:02x}"));
    }
    Ok(out)
}

fn collect_files(
    root: &Path,
    current: &Path,
    out: &mut Vec<(String, PathBuf)>,
) -> Result<(), String> {
    let entries = std::fs::read_dir(current)
        .map_err(|e| format!("读取目录 {} 失败：{e}", current.display()))?;
    for entry in entries {
        let entry = entry.map_err(|e| format!("读取目录项失败：{e}"))?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|e| format!("读取文件类型 {} 失败：{e}", path.display()))?;
        let name = entry.file_name().to_string_lossy().to_string();
        if file_type.is_dir() {
            // .git 是取源方式留下的元数据，不是插件产物：同一 commit 用 clone
            // 与 init+fetch 取回，.git 里的文件布局并不相同，纳入摘要会让 git
            // 来源的计划永远对不上。
            if name != ".git" {
                collect_files(root, &path, out)?;
            }
        } else if name != ".dsh-id" && !METADATA_FILES.contains(&name.as_str()) {
            let relative = path
                .strip_prefix(root)
                .map(|value| {
                    value
                        .to_string_lossy()
                        .replace(std::path::MAIN_SEPARATOR, "/")
                })
                .unwrap_or_else(|_| name);
            out.push((relative, path));
        }
    }
    Ok(())
}

/// 中央库目录里由外壳写入、但不代表插件内容的元数据文件（见 [`tree_digest`]）。
const METADATA_FILES: [&str; 2] = [crate::pkg::fetch::SOURCE_MARKER, ".dsh-build.log"];

/// 从预检结果生成计划里的固定 spec。
pub fn build_pinned_spec(
    spec: &crate::plugins::center::PluginSpec,
    resolved_version: &str,
    resolved_commit: &str,
) -> Result<String, String> {
    match spec.origin.as_str() {
        "npm" => {
            let (name, _) =
                crate::pkg::fetch::split_npm_spec(&spec.source).map_err(|e| e.to_string())?;
            if resolved_version.trim().is_empty() {
                return Err("npm 来源没有解析出精确版本".to_string());
            }
            Ok(format!("{}@{}", name, resolved_version.trim()))
        }
        "git" => {
            let base = spec.repo_url.clone().unwrap_or_else(|| spec.source.clone());
            let pin = if !resolved_commit.trim().is_empty() {
                resolved_commit.trim()
            } else if !resolved_version.trim().is_empty() {
                resolved_version.trim()
            } else {
                return Err("git 来源没有解析出 tag 或 commit".to_string());
            };
            Ok(format!("{}#{}", base, pin))
        }
        other => Err(format!("未知来源 {other:?}，无法生成固定安装计划")),
    }
}

/// git 计划使用的完整 commit。旧 API 没有暴露这一点时返回空串。
pub fn git_head_commit(plugin_dir: &Path) -> String {
    let dir = plugin_dir.to_string_lossy().to_string();
    match crate::shell::process::run_capture("git", &["-C", &dir, "rev-parse", "HEAD"]) {
        Ok((true, out)) => out.trim().to_string(),
        _ => String::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(settings: &Settings) -> InstallPlan {
        let now = crate::shell::process::epoch_millis();
        InstallPlan {
            schema_version: SCHEMA_VERSION,
            plan_id: String::new(),
            spec: "@scope/pkg@1.2.3".to_string(),
            pinned_spec: "@scope/pkg@1.2.3".to_string(),
            source_kind: "npm".to_string(),
            requested_pin: "1.2.3".to_string(),
            resolved_version: "1.2.3".to_string(),
            resolved_commit: String::new(),
            integrity: "none".to_string(),
            artifact_item_digest: String::new(),
            kernel_fingerprint: String::new(),
            content_digest: "digest".to_string(),
            kernel_version: String::new(),
            family: "dsh".to_string(),
            target_instance: "default".to_string(),
            profile: settings.profile.clone(),
            port: settings.port,
            materialize: "link".to_string(),
            platform: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
            node_version: "v20.0.0".to_string(),
            created_at_ms: now,
            expires_at_ms: now + TTL_MS,
            evidence_run_id: "run".to_string(),
        }
        .finalize()
    }

    #[test]
    fn plan_id_detects_tampering() {
        let plan = sample(&Settings::default());
        plan.verify_id().expect("刚生成的计划必须通过校验");
        // 来源身份 / 版本 / commit 都在 plan_id 覆盖范围内：摘要不再覆盖
        // `.dsh-source.json`（FIX-01）之后，这些字段的防篡改只能靠计划本身。
        let mutations: [fn(&mut InstallPlan); 5] = [
            |plan| plan.resolved_version = "9.9.9".to_string(),
            |plan| plan.resolved_commit = "deadbeef".to_string(),
            |plan| plan.pinned_spec = "@scope/pkg@9.9.9".to_string(),
            |plan| plan.source_kind = "git".to_string(),
            |plan| plan.spec = "@scope/pkg@9.9.9".to_string(),
        ];
        for mutate in mutations {
            let mut tampered = plan.clone();
            mutate(&mut tampered);
            assert!(
                tampered.verify_id().is_err(),
                "改了字段却没有改 plan_id 时必须被拒绝"
            );
        }
    }

    #[test]
    fn expired_plan_is_refused() {
        let settings = Settings::default();
        let mut plan = sample(&settings);
        plan.expires_at_ms = crate::shell::process::epoch_millis().saturating_sub(1);
        plan = plan.finalize();
        let error = plan
            .validate_for(
                "dsh",
                "default",
                Path::new("/tmp/none"),
                &settings,
                "link",
                "v20.0.0",
            )
            .expect_err("过期计划必须被拒绝");
        assert!(error.contains("过期"), "{error}");
    }

    #[test]
    fn environment_drift_is_refused() {
        let settings = Settings::default();
        let plan = sample(&settings);
        let data_dir = Path::new("/tmp/dsh-plan-no-kernels");
        plan.validate_for("dsh", "default", data_dir, &settings, "link", "v20.0.0")
            .expect("基线应通过");
        let mut other_port = settings.clone();
        other_port.port = settings.port.wrapping_add(1);
        assert!(plan
            .validate_for("dsh", "default", data_dir, &other_port, "link", "v20.0.0")
            .is_err());
        assert!(plan
            .validate_for("dsh", "other", data_dir, &settings, "link", "v20.0.0")
            .is_err());
        assert!(plan
            .validate_for("dsh", "default", data_dir, &settings, "copy", "v20.0.0")
            .is_err());
        assert!(plan
            .validate_for("dsh", "default", data_dir, &settings, "link", "v22.0.0")
            .is_err());
    }

    #[test]
    fn pinned_spec_uses_exact_identity() {
        let mut spec = crate::plugins::center::PluginSpec {
            origin: "npm".to_string(),
            source: "@scope/pkg".to_string(),
            pin: Some("1.2.3".to_string()),
            id: "scope__pkg".to_string(),
            name: "@scope/pkg".to_string(),
            repo_url: None,
        };
        assert_eq!(
            build_pinned_spec(&spec, "1.2.3", "").unwrap(),
            "@scope/pkg@1.2.3"
        );
        spec.origin = "git".to_string();
        spec.source = "https://github.com/o/r.git".to_string();
        spec.repo_url = Some("https://github.com/o/r.git".to_string());
        assert_eq!(
            build_pinned_spec(&spec, "v1.0.0", "abcdefabcdefabcdefabcdefabcdefabcdefabcd").unwrap(),
            "https://github.com/o/r.git#abcdefabcdefabcdefabcdefabcdefabcdefabcd",
            "有完整 commit 时优先用 commit，tag 可能被移动"
        );
        assert_eq!(
            build_pinned_spec(&spec, "v1.0.0", "").unwrap(),
            "https://github.com/o/r.git#v1.0.0"
        );
    }

    #[test]
    fn tree_digest_ignores_shell_marker_and_tracks_content() {
        let dir = std::env::temp_dir().join(format!(
            "dsh-plan-digest-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("index.js"), b"one").unwrap();
        std::fs::write(dir.join(".dsh-id"), b"first-id").unwrap();
        // 动态来源元数据：只有 fetchedAt 不同时摘要必须相同（FIX-01）。
        std::fs::write(
            dir.join(crate::pkg::fetch::SOURCE_MARKER),
            br#"{"id":"pkg","origin":"npm","source":"pkg","version":"1.0.0","fetchedAt":"1"}"#,
        )
        .unwrap();
        std::fs::write(dir.join(".dsh-build.log"), b"first build output").unwrap();
        let before = tree_digest(&dir).unwrap();
        std::fs::write(dir.join(".dsh-id"), b"another-id").unwrap();
        std::fs::write(
            dir.join(crate::pkg::fetch::SOURCE_MARKER),
            br#"{"id":"pkg","origin":"npm","source":"pkg","version":"1.0.0","fetchedAt":"999999"}"#,
        )
        .unwrap();
        std::fs::write(dir.join(".dsh-build.log"), b"second build output").unwrap();
        assert_eq!(
            tree_digest(&dir).unwrap(),
            before,
            "外壳 id / fetchedAt / 构建日志不参与摘要"
        );
        std::fs::write(dir.join("index.js"), b"two").unwrap();
        assert_ne!(tree_digest(&dir).unwrap(), before, "内容变化必须改变摘要");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
