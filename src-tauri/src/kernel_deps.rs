//! 内核安装时的**依赖钉版**决策：写 stub / workspace yaml 的 `overrides`、
//! 扫描锁步错位，以及「上游漏发某个精确钉版」时的降级兜底。
//!
//! 三件事共用同一份数据（`{ 包名: 版本 }` 的 overrides 增量），所以放在一个
//! 模块里，而不是散在 `kernel.rs` 的安装流程中：
//!
//! 1. [`write_stub`] / [`write_workspace_yaml`]：把 overrides 同时写进
//!    stub `package.json` 的 `pnpm` 字段与 `pnpm-workspace.yaml`。**pnpm 10
//!    起只认后者**（实测 pnpm 11.15 完全无视 package.json 里的
//!    `pnpm.overrides`），旧版 pnpm（≤9）反之只认前者，所以两处都写。
//! 2. [`scan_lockstep_skew`]：内核是 monorepo **锁步发布**（同一版本线上所有
//!    `@deepseek-ai/dsh*` 子包一起出版本号），而主包用 `^0.1.6-alpha.1` 这类
//!    范围声明依赖，pnpm 会浮动到范围内最新——装 alpha.1 时 alpha.2 已发布，
//!    全部子包落到 alpha.2，而 alpha 之间没有兼容承诺，内核启动即报
//!    `does not provide an export named '…'`。装完扫目录把错位钉回内核版本。
//! 3. [`run_install_passes`]：**上游漏发**某个精确钉版时的降级兜底，见下。
//!
//! ## 为什么需要第 3 件事
//!
//! 锁步发布的前提是「每个子包都发了」。现实里会漏：2026-09-29 实测，内核
//! `0.2.0-rc.2` 发布后，`@deepseek-ai/dsh-web-app@0.2.0-rc.2` 精确钉住了
//! `@deepseek-ai/dsh-client-ui-settings-account@0.2.0-rc.2`，而后者**官方
//! registry 与 npmmirror 都没有**（两边最新都停在 `0.2.0-rc.1`）。于是 pnpm
//! 在解析阶段就失败：
//!
//! ```text
//! ERR_PNPM_NO_MATCHING_VERSION
//!   × Failed to resolve dependency tree: No matching version found for
//!     @deepseek-ai/dsh-client-ui-settings-account@0.2.0-rc.2
//! ```
//!
//! **换 registry 救不了**——这不是镜像延迟（两个 registry 的 packument 里都
//! 没有那个版本），而是上游发布事故。壳不能就此拒绝安装：整个 0.2.0-rc.2
//! 闭包里只有这一条断边，降级钉到同一版本线上的 `0.2.0-rc.1` 就能装上完整可用的
//! 内核。因此这里在**失败后**（而不是安装前——正常安装不该为此多付网络往返）
//! 查一次 registry，为断边各选一个替代版本写进 overrides，再重跑一次 pnpm。
//!
//! 边界见 [`pick_fallback`]：只退到**同一 `major.minor` 版本线**内的较低版本，
//! 跨版本线宁可不装——那种情况下差异可能已经大到不兼容，壳无权替用户决定。
//!
//! 降级是**明说**的：每一条都进进度面板，并汇总进最终总结（见
//! [`RelaxedPins::summary`]），不静默地给用户一个"版本对不上"的安装。

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;
use std::process::ExitStatus;

use crate::process::{atomic_write, LogSpec};

/// 降级兜底最多重试几轮。pnpm 一次只报一个断边，而上游可能漏发多个子包；
/// 3 轮足够覆盖现实情况，又不至于让一次坏安装在网络上来回跑很久。
const MAX_RELAX_ROUNDS: usize = 3;

/// 输出捕获上限（字节）。只用于从 pnpm 的尾部输出里解析断边，不是日志——
/// 完整输出始终由 `process::run_with_progress` 落盘。
const OUTPUT_TAIL_LIMIT: usize = 64 * 1024;

/// 壳只对**官方命名空间**的包做降级决定。第三方包（插件生态那些）的缺版本
/// 问题不由内核安装兜底：内核安装替用户的插件树做主键选择，越界了。
const OFFICIAL_SCOPE: &str = "@deepseek-ai/";

// ─── 降级钉版记录 ──────────────────────────────────────────────────────────

/// 一批「上游漏发、壳代为降级钉版」的官方子包，以及最近一次**没能**兜底的
/// 缺版本（用于把上游发布事故与网络故障分开讲）。
///
/// `pins` 的值是 `(被请求的版本, 实际钉的版本)`——两个都留着，文案要说清
/// 「要的是哪个、装的是哪个」，只说一个会让用户以为装成了想要的那个。
#[derive(Debug, Default, Clone)]
pub struct RelaxedPins {
    kernel: String,
    pins: BTreeMap<String, (String, String)>,
    unresolved: Option<String>,
}

impl RelaxedPins {
    /// 绑定内核版本（文案里要说清「内核 X 缺了哪个子包」）。
    pub fn new(kernel_version: &str) -> Self {
        Self {
            kernel: kernel_version.to_string(),
            ..Self::default()
        }
    }

    pub fn is_empty(&self) -> bool {
        self.pins.is_empty()
    }

    /// `package` 被降级钉到哪个版本（没降过则 `None`）。
    pub fn get(&self, package: &str) -> Option<&str> {
        self.pins.get(package).map(|(_, used)| used.as_str())
    }

    fn record(&mut self, package: &str, wanted: &str, used: &str) {
        self.pins
            .insert(package.to_string(), (wanted.to_string(), used.to_string()));
    }

    /// 把降级钉版并进某轮要写进 stub 的 overrides。
    ///
    /// **必须合并而不是替换**：锁步重装那一轮写的是「错位清单」，若它覆盖掉
    /// 降级钉版，就会把包重新钉回那个不存在的版本，pnpm 立刻二次失败。
    pub fn merge_into(&self, overrides: &mut BTreeMap<String, String>) {
        for (package, (_, used)) in &self.pins {
            overrides.insert(package.clone(), used.clone());
        }
    }

    /// 人类可读的一句汇总，进度面板与安装总结共用。
    pub fn summary(&self) -> String {
        let items = self
            .pins
            .iter()
            .map(|(package, (wanted, used))| format!("{package}（内核要求 {wanted}，实装 {used}）"))
            .collect::<Vec<_>>()
            .join("、");
        format!(
            "内核 {} 有 {} 个官方子包的对应版本在上游 registry 尚未发布，已改钉到同一版本线上已发布的较低版本：{items}",
            self.kernel,
            self.pins.len()
        )
    }

    /// 安装失败时给用户的一句话。**必须说清「下一步做什么」**（AGENTS.md）：
    /// 缺版本类失败与网络类失败要分开讲，前者重试网络毫无意义。
    pub fn describe_failure(&self, status: &ExitStatus, log_path: &Path) -> String {
        let exit_code = status
            .code()
            .map(|c| c.to_string())
            .unwrap_or_else(|| "? (信号)".into());
        let log = log_path.display();
        if let Some(missing) = &self.unresolved {
            return format!(
                "内核 {} 的依赖没有发布完整：{missing} 在 registry 上不存在，且同一版本线上没有可用的较低版本可退。\
                 这是上游发布事故（与网络无关，重试无用），请改用其他内核版本（例如上一个 rc），\
                 并把下面的日志反馈给内核发布方：{log}",
                self.kernel
            );
        }
        if !self.is_empty() {
            return format!(
                "pnpm 安装失败（退出码 {exit_code}）。{}。该改动未能解决安装失败，\
                 请查看日志确认是否还有其它依赖缺口：{log}",
                self.summary()
            );
        }
        format!(
            "pnpm 安装失败（退出码 {exit_code}），请检查网络或 pnpm 配置后重试，详情见日志：{log}"
        )
    }
}

// ─── 选降级版本 ────────────────────────────────────────────────────────────

/// `major.minor` 相同才算同一条版本线。比较只看前两段数字，`.` 分隔的第三段
/// （以及预发布段）不参与。
fn same_release_line(a: &str, b: &str) -> bool {
    let head = |v: &str| {
        let stripped = v.strip_prefix('v').unwrap_or(v);
        let core = stripped.split('-').next().unwrap_or(stripped);
        let mut parts = core.split('.').filter_map(|s| s.parse::<u64>().ok());
        (parts.next(), parts.next())
    };
    head(a) == head(b)
}

/// 在 `published` 里挑一个替代版本：`requested` 尚未发布时，**同一
/// `major.minor` 版本线内语义化严格小于它的最大已发布版本**。
///
/// 三条取舍：
///
/// - **只退到更低**：更高的版本属于「未来」，它与内核其余子包的配套关系
///   未经任何测试，拿它顶替是在替用户做更大的赌。
/// - **不跨 `major.minor`**：`0.1.7-rc.2` 顶 `0.2.0-rc.2` 跨了一整个 minor，
///   内部接口可能已经改过。那种情况宁可不装——本函数返回 `None`，由
///   [`describe_install_failure`] 如实告诉用户「上游发布不完整，换个内核版本」。
/// - **不跨稳定性语义**：预发布版与正式版的兼容承诺不同，但同版本线内的
///   `0.2.0-rc.1` → `0.2.0-rc.2` 属于同一批次，差异是一步之遥，取舍成立。
fn pick_fallback(requested: &str, published: &BTreeSet<String>) -> Option<String> {
    published
        .iter()
        .filter(|candidate| {
            crate::version::is_valid_kernel_version(candidate)
                && same_release_line(candidate, requested)
                && crate::version::cmp_versions(candidate, requested) == std::cmp::Ordering::Less
        })
        .max_by(|a, b| crate::version::cmp_versions(a, b))
        .cloned()
}

// ─── 解析 pnpm 的「缺版本」报错 ───────────────────────────────────────────

/// 从 pnpm 输出里解析「registry 上没有这个精确版本」的 `包名@版本`。
///
/// 三个现实约束决定了这里的写法：
///
/// 1. pnpm 会把错误文本**折行**（实测 `No matching version found for\n      pkg@1.2.3`），
///    所以先按空白折叠成一行再找。
/// 2. 包名来自远端，会被写进 `pnpm-workspace.yaml` 与 stub `package.json`，
///    因此必须过命名空间与形态两道闸（[`is_official_package`]）。
/// 3. 解析不出来时返回 `None`——调用方按**原样失败**处理，绝不猜。
fn parse_missing_pin(output: &str) -> Option<(String, String)> {
    let flat = output.split_whitespace().collect::<Vec<_>>().join(" ");
    let rest = flat.split_once("No matching version found for ")?.1;
    // pnpm 的完整文案是 `…@1.2.3 while fetching it from <registry>`；缺了
    // `while fetching` 的变体也按「取到空白为止」处理。
    let token = rest
        .split(" while fetching")
        .next()
        .unwrap_or(rest)
        .split(' ')
        .next()?;
    let (package, version) = token.rsplit_once('@')?;
    (is_official_package(package) && crate::version::is_valid_kernel_version(version))
        .then(|| (package.to_string(), version.to_string()))
}

/// 官方包名闸：必须是 `@deepseek-ai/<段>`，且 `<段>` 形态干净（无路径分隔符、
/// 控制字符、引号——后两者会闭合 stub 的 JSON / yaml 键值）。
fn is_official_package(package: &str) -> bool {
    package
        .strip_prefix(OFFICIAL_SCOPE)
        .is_some_and(|id| crate::paths::validate_id_component(id).is_ok())
}

/// registry 上某个包已发布的版本集合。走**当前生效的 registry**（默认 npmmirror，
/// `DSH_NPM_REGISTRY` 可覆盖）——降级目标必须在 pnpm 真正解析的那个 registry 上
/// 存在，否则重试还是同样的失败。
fn published_versions(package: &str) -> Result<BTreeSet<String>, String> {
    let url = format!("{}{package}", crate::registry::npm_registry_base());
    let body = crate::releases::http_get_string(&url, None)?;
    let doc: serde_json::Value = serde_json::from_str(&body)
        .map_err(|e| format!("解析 {package} 的 registry 元数据失败：{e}"))?;
    let versions = doc
        .get("versions")
        .and_then(serde_json::Value::as_object)
        .ok_or_else(|| format!("{package} 的 registry 元数据里没有 versions 字段"))?;
    Ok(versions.keys().cloned().collect())
}

// ─── stub / workspace yaml ────────────────────────────────────────────────

/// 内核 stub 的最小 package.json；`overrides` 非空时带上 `pnpm.overrides`
/// （依赖锁步钉版，见 [`scan_lockstep_skew`]）。
pub fn write_stub(
    stub: &Path,
    version: &str,
    overrides: &BTreeMap<String, String>,
) -> std::io::Result<()> {
    // 用 serde_json 构造而不是手写 format!：版本号一旦含有引号，手写模板会被
    // 闭合、注入任意字段（pnpm 之后会执行 stub 里的生命周期脚本）。这里由
    // 序列化器负责转义；命令边界的 is_valid_kernel_version 是更早的一道闸。
    let mut doc = serde_json::json!({
        "name": format!("dsh-kernel-{}", version.replace('.', "_")),
        "private": true,
        "version": "1.0.0",
    });
    if !overrides.is_empty() {
        doc["pnpm"] = serde_json::json!({ "overrides": overrides });
    }
    atomic_write(stub, format!("{doc}\n").as_bytes())
}

/// 内核目录的 `pnpm-workspace.yaml`：`packages` 段把内核目录锚定为 workspace
/// 根（这是移除 `--ignore-workspace` 后防上层 workspace 泄漏的机制），
/// `overrides` 段承载依赖锁步钉版。
///
/// **pnpm 10 起这类配置从 package.json 的 `pnpm` 字段迁到了本文件**——实测
/// pnpm 11.15 对 stub package.json 里的 `pnpm.overrides` 完全无视（lockfile
/// 不生成 overrides 段），只认这里。旧版 pnpm（≤9）反之只认 package.json，
/// 所以两处都写。空 `overrides` 时省略该段（安装首轮还没有错位清单）。
pub fn write_workspace_yaml(
    kernel_dir: &Path,
    overrides: &BTreeMap<String, String>,
) -> std::io::Result<()> {
    let mut yaml = String::from("packages:\n  - \".\"\n");
    if !overrides.is_empty() {
        yaml.push_str("overrides:\n");
        for (name, version) in overrides {
            yaml.push_str(&format!("  \"{name}\": \"{version}\"\n"));
        }
    }
    atomic_write(&kernel_dir.join("pnpm-workspace.yaml"), yaml.as_bytes())
}

// ─── 锁步错位扫描 ──────────────────────────────────────────────────────────

/// 扫描 hoisted `node_modules` 里与内核版本错位的官方 dsh 锁步子包，返回
/// `{ 包名: 内核版本 }` 形式的 `pnpm.overrides` 增量。
///
/// 传递依赖不在主包依赖表里，所以必须**装完后扫目录**而不是预先从元数据推导。
///
/// `relaxed` 里的包**不计入错位**：它们正是 [`run_install_passes`] 刻意降级
/// 钉版的（上游漏发），把「刻意的错位」当成错位再钉回内核版本，只会得到
/// 一个 pnpm 解析不了的 overrides。
pub fn scan_lockstep_skew(
    kernel_dir: &Path,
    version: &str,
    relaxed: &RelaxedPins,
) -> BTreeMap<String, String> {
    let scope = kernel_dir.join("node_modules").join("@deepseek-ai");
    let Ok(entries) = fs::read_dir(&scope) else {
        return BTreeMap::new();
    };
    let mut overrides = BTreeMap::new();
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name != "dsh" && !name.starts_with("dsh-") {
            continue;
        }
        // 名字要写进 pnpm-workspace.yaml，先过路径组件闸（拒绝引号、
        // 控制字符、分隔符），让畸形目录名走「跳过」而不是污染 yaml。
        if crate::paths::validate_id_component(&name).is_err() {
            continue;
        }
        let Ok(text) = fs::read_to_string(entry.path().join("package.json")) else {
            continue;
        };
        let installed = serde_json::from_str::<serde_json::Value>(&text)
            .ok()
            .and_then(|doc| {
                doc.get("version")
                    .and_then(|v| v.as_str())
                    .map(str::to_string)
            })
            .unwrap_or_default();
        if installed != version
            && relaxed.get(&format!("{OFFICIAL_SCOPE}{name}")) != Some(installed.as_str())
        {
            overrides.insert(format!("{OFFICIAL_SCOPE}{name}"), version.to_string());
        }
    }
    overrides
}

// ─── pnpm 运行与降级重试 ───────────────────────────────────────────────────

/// 跑 pnpm 安装内核；失败原因是「registry 上没有某个被精确钉住的官方子包
/// 版本」时，查一次 registry、给断边各选一个同版本线的较低版本写进
/// overrides，然后重跑（见模块文档的「为什么需要第 3 件事」）。
///
/// 正常安装只跑一轮：预检放在失败之后，成功路径不多付一次网络往返。
///
/// `base_overrides` 是本轮调用方自己那份（首轮为空，锁步重装轮为错位清单）；
/// 每次重试都以 `base_overrides ∪ pins` 重写 stub 与 workspace yaml——降级
/// 结果必须活得比本轮久，否则下一轮会把它覆盖掉。
// 参数多是因为它整体就是「跑一次 pnpm 安装」的全部上下文；调用方
// （`kernel::install_version_into`）两轮调用的形状完全一致，拆成结构体
// 只会让这两个调用点多一层字段搬运。
#[allow(clippy::too_many_arguments)]
pub fn run_install_passes(
    pnpm_exe: &Path,
    args: &[&str],
    kernel_dir: &Path,
    version: &str,
    base_overrides: &BTreeMap<String, String>,
    pins: &mut RelaxedPins,
    logs_dir: &Path,
    log_spec: &LogSpec,
    extra_path_dirs: &[&Path],
    on_progress: &mut impl FnMut(&str),
) -> Result<ExitStatus, String> {
    for round in 0..=MAX_RELAX_ROUNDS {
        let mut tail = String::new();
        let status = crate::kernel::run_pnpm(
            pnpm_exe,
            args,
            kernel_dir,
            logs_dir,
            log_spec,
            extra_path_dirs,
            |line| {
                append_tail(&mut tail, line);
                on_progress(line);
            },
        )
        .map_err(|e| e.to_string())?;
        // 成功、或重试次数用尽：把最后一轮的状态如实交回，由调用方按
        // 「退出码非零但产物完整可降级为警告」的老规矩处理。
        if status.success() || round == MAX_RELAX_ROUNDS {
            return Ok(status);
        }
        let Some((package, wanted)) = parse_missing_pin(&tail) else {
            return Ok(status);
        };
        // 上一轮已经降级过它就不再重试：registry 上不会凭空多出版本，
        // 反复问同一个问题只会把一次坏安装拖成长任务。
        if pins.get(&package).is_some() {
            pins.unresolved = Some(format!("{package}@{wanted}"));
            return Ok(status);
        }
        let fallback = match published_versions(&package) {
            // 查不到版本列表时**不降级**：宁可如实失败，也不要凭猜测改版本。
            Err(reason) => {
                on_progress(&format!(
                    "无法查询 {package} 在 registry 上的已发布版本（{reason}），按原始依赖继续"
                ));
                None
            }
            Ok(published) => pick_fallback(&wanted, &published),
        };
        let Some(fallback) = fallback else {
            pins.unresolved = Some(format!("{package}@{wanted}"));
            return Ok(status);
        };
        on_progress(&format!(
            "上游 registry 上还没有 {package}@{wanted}（这是内核 {version} 的依赖发布缺口，不是网络问题），\
             已改钉到同一版本线上已发布的 {fallback} 并重试安装"
        ));
        pins.record(&package, &wanted, &fallback);
        let mut overrides = base_overrides.clone();
        pins.merge_into(&mut overrides);
        write_stub(&kernel_dir.join("package.json"), version, &overrides)
            .and_then(|()| write_workspace_yaml(kernel_dir, &overrides))
            .map_err(|e| format!("写入依赖钉版配置失败：{e}"))?;
    }
    unreachable!("循环上界即返回点：round == MAX_RELAX_ROUNDS 时已在上面返回")
}

/// 把一行输出并进定长尾部缓冲。只留尾部：pnpm 的错误摘要打在最后，
/// 而 `Progress:` 行可以轻易刷满几兆。
fn append_tail(tail: &mut String, line: &str) {
    tail.push_str(line);
    tail.push('\n');
    if tail.len() > OUTPUT_TAIL_LIMIT {
        let mut cut = tail.len() - OUTPUT_TAIL_LIMIT;
        // 按字符边界切：pnpm 的折行里有多字节字符，切在中间会让字符串非法。
        while !tail.is_char_boundary(cut) {
            cut += 1;
        }
        tail.drain(..cut);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn versions(list: &[&str]) -> BTreeSet<String> {
        list.iter().map(|v| v.to_string()).collect()
    }

    #[test]
    fn fallback_stays_on_the_same_release_line_and_never_goes_up() {
        let published = versions(&[
            "0.1.7-rc.2",
            "0.2.0-rc.1",
            "0.2.0-rc.2",
            "0.2.1",
            "0.3.0-rc.1",
        ]);
        // 2026-09-29 的真实场景：0.2.0-rc.2 被请求，0.2.0-rc.1 是同版本线的上一档。
        assert_eq!(
            pick_fallback("0.2.0-rc.2", &published),
            Some("0.2.0-rc.1".to_string()),
            "应退到同版本线内的最大较低版本，而不是 0.2.1 / 0.3.0-rc.1"
        );
        assert_eq!(
            pick_fallback("0.2.1", &published),
            Some("0.2.0-rc.2".to_string()),
            "请求的补丁版本整条都没有时仍应留在 0.2.x 内，且取最大的较低者"
        );
    }

    #[test]
    fn fallback_refuses_to_cross_a_minor_boundary() {
        let published = versions(&["0.1.7-rc.2", "0.1.7-alpha.2"]);
        assert_eq!(
            pick_fallback("0.2.0-rc.2", &published),
            None,
            "跨 minor 的兼容性没有承诺，宁可不装"
        );
    }

    #[test]
    fn fallback_never_picks_a_newer_or_a_malformed_version() {
        let published = versions(&["0.2.0-rc.1", "0.2.0-rc.3", "0.2.0-rc.10"]);
        assert_eq!(
            pick_fallback("0.2.0-rc.2", &published),
            Some("0.2.0-rc.1".to_string()),
            "已发布的更高版本不能顶替：它与内核其余子包的配套关系未经测试"
        );
        let poisoned = versions(&["0.2.0-rc.1", "0.2.0-rc.2\" , \"evil\": \"1"]);
        assert_eq!(
            pick_fallback("0.2.0-rc.2", &poisoned),
            Some("0.2.0-rc.1".to_string()),
            "registry 里形态非法的版本号不得进入 overrides"
        );
        assert_eq!(pick_fallback("0.2.0", &versions(&[])), None);
    }

    /// pnpm 会把错误文本折行，解析必须跨行成立——2026-09-29 真实报错正是这个形态。
    #[test]
    fn parses_the_real_pnpm_missing_version_error() {
        let output = r#"Progress: resolved 0, reused 170, downloaded 108, added 0
Error: ERR_PNPM_NO_MATCHING_VERSION

  × adding a new package
  ╰─▶ Failed to resolve dependency tree: No matching version found for
      @deepseek-ai/dsh-client-ui-settings-account@0.2.0-rc.2 while fetching it
      from https://registry.npmmirror.com/
  help: The latest release of … is "0.1.7-alpha.1". Published at 9/22/2026"#;
        assert_eq!(
            parse_missing_pin(output),
            Some((
                "@deepseek-ai/dsh-client-ui-settings-account".to_string(),
                "0.2.0-rc.2".to_string()
            ))
        );
    }

    /// 变体文案与第三方包都必须被拒：降级决定只对官方命名空间生效，
    /// 且解析不出来时按原样失败而不是猜。
    #[test]
    fn missing_pin_parser_rejects_third_party_and_noise() {
        let third_party = "No matching version found for lodash@4.17.21 while fetching it";
        assert_eq!(
            parse_missing_pin(third_party),
            None,
            "第三方包不由内核安装兜底"
        );
        let pathy = "No matching version found for @deepseek-ai/..\\evil@1.0.0 while fetching it";
        assert_eq!(
            parse_missing_pin(pathy),
            None,
            "形态危险的包名不得进入 overrides"
        );
        let injected =
            "No matching version found for @deepseek-ai/x@1.0.0\", \"evil\": \"1 while fetching it";
        assert_eq!(
            parse_missing_pin(injected),
            None,
            "含引号的版本号会闭合 yaml/JSON"
        );
        assert_eq!(parse_missing_pin("Progress: resolved 0, reused 170"), None);
        assert_eq!(parse_missing_pin(""), None);
    }

    /// 降级钉版必须**合并**进后续轮次的 overrides：锁步重装写的是错位清单，
    /// 若它覆盖掉降级结果，包会被重新钉回那个不存在的版本。
    #[test]
    fn relaxed_pins_merge_into_later_overrides() {
        let mut pins = RelaxedPins::new("0.2.0-rc.2");
        pins.record(
            "@deepseek-ai/dsh-client-ui-settings-account",
            "0.2.0-rc.2",
            "0.2.0-rc.1",
        );
        let mut overrides = BTreeMap::from([(
            "@deepseek-ai/dsh-app-boot".to_string(),
            "0.2.0-rc.2".to_string(),
        )]);
        pins.merge_into(&mut overrides);

        assert_eq!(overrides.len(), 2, "错位钉版与降级钉版必须同时存在");
        assert_eq!(
            overrides["@deepseek-ai/dsh-client-ui-settings-account"],
            "0.2.0-rc.1"
        );
        assert!(
            pins.summary().contains("实装 0.2.0-rc.1"),
            "文案要说清装的是哪个版本：{}",
            pins.summary()
        );
    }

    /// 错位扫描：降级过的包不再算错位（否则会被钉回不存在的版本），
    /// 真正错位的包照旧进入 overrides。
    #[test]
    fn lockstep_skew_skips_deliberately_relaxed_packages() {
        let dir = std::env::temp_dir().join(format!(
            "dsh-relaxed-skew-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let scope = dir.join("node_modules").join("@deepseek-ai");
        let write = |name: &str, version: &str| {
            let path = scope.join(name);
            std::fs::create_dir_all(&path).unwrap();
            std::fs::write(
                path.join("package.json"),
                format!(r#"{{"name":"@deepseek-ai/{name}","version":"{version}"}}"#),
            )
            .unwrap();
        };
        write("dsh", "0.2.0-rc.2");
        write("dsh-client-ui-settings-account", "0.2.0-rc.1"); // 刻意降级
        write("dsh-app-boot", "0.2.0-rc.1"); // 浮动错位，仍要钉
        write("cordis", "4.0.2"); // 非锁步，不参与

        let mut pins = RelaxedPins::new("0.2.0-rc.2");
        pins.record(
            "@deepseek-ai/dsh-client-ui-settings-account",
            "0.2.0-rc.2",
            "0.2.0-rc.1",
        );
        let skew = scan_lockstep_skew(&dir, "0.2.0-rc.2", &pins);

        assert_eq!(
            skew,
            BTreeMap::from([(
                "@deepseek-ai/dsh-app-boot".to_string(),
                "0.2.0-rc.2".to_string()
            )]),
            "刻意降级不算错位，浮动错位仍要钉回内核版本"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    /// 缺版本类失败与网络类失败必须分开讲：前者重试网络毫无意义。
    #[test]
    fn failure_text_separates_upstream_gap_from_network_trouble() {
        let dir = std::env::temp_dir().join("dsh-relaxed-failure.log");
        let mut pins = RelaxedPins::new("0.2.0-rc.2");

        let plain = pins.describe_failure(&failure_of(1), &dir);
        assert!(plain.contains("请检查网络或 pnpm 配置"), "{plain}");

        pins.record(
            "@deepseek-ai/dsh-client-ui-settings-account",
            "0.2.0-rc.2",
            "0.2.0-rc.1",
        );
        let relaxed = pins.describe_failure(&failure_of(1), &dir);
        assert!(relaxed.contains("上游"), "{relaxed}");
        assert!(
            relaxed.contains("dsh-client-ui-settings-account"),
            "要说清是哪个包降级了：{relaxed}"
        );

        let mut gap = RelaxedPins::new("0.2.0-rc.2");
        gap.unresolved = Some("@deepseek-ai/dsh-client-ui-settings-account@0.2.0-rc.2".into());
        let text = gap.describe_failure(&failure_of(1), &dir);
        assert!(text.contains("依赖没有发布完整"), "{text}");
        assert!(
            text.contains("与网络无关"),
            "{text}，重试网络对上游发布事故毫无意义"
        );
        assert!(
            text.contains(&dir.display().to_string()),
            "必须给出日志路径：{text}"
        );
    }

    #[cfg(unix)]
    fn failure_of(code: i32) -> ExitStatus {
        std::os::unix::process::ExitStatusExt::from_raw(code << 8)
    }

    #[cfg(windows)]
    fn failure_of(code: i32) -> ExitStatus {
        std::os::windows::process::ExitStatusExt::from_raw(code as u32)
    }

    /// 尾部缓冲只留最后 `OUTPUT_TAIL_LIMIT` 字节，且必须按字符边界切——
    /// pnpm 的输出里有中文与折行，切在多字节中间会产出非法字符串。
    #[test]
    fn output_tail_keeps_the_end_and_stays_valid_utf8() {
        let mut tail = String::new();
        append_tail(&mut tail, &"进".repeat(OUTPUT_TAIL_LIMIT / 3 + 64));
        assert!(tail.len() <= OUTPUT_TAIL_LIMIT + 8, "尾部缓冲不得无限增长");
        assert!(
            tail.trim_end().chars().all(|c| c == '进'),
            "按字符边界切开后不得留下半个字符"
        );
        append_tail(
            &mut tail,
            "No matching version found for @deepseek-ai/dsh@0.2.0-rc.2",
        );
        assert!(parse_missing_pin(&tail).is_some(), "最后一行必须仍然可解析");
    }
}
