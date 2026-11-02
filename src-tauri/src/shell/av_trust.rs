//! 杀软排除项：把内核的「第三方包代码」目录交给杀软免检。
//!
//! ## 为什么是「排除项」而不是「防火墙例外」
//!
//! 防火墙管的是网络，文件被扫描是杀软的**实时保护**管的事——加防火墙例外对
//! 「打开文件慢」一点用都没有。真正管用的是杀软的**信任区 / 排除项**。
//!
//! ## 为什么值得做
//!
//! 本机实测：打开一个从没读过的文件（open + close）约 7.7ms，再打开一次
//! 0.07ms；内核冷启动 55 秒 ≈ 7,700 次这样的 open × 7.7ms。杀软的扫描代价
//! **付在 open 上**，stat 上几乎没有。⇒ 冷启动慢的是杀软，不是内核。
//!
//! ## 范围（安全边界，不许放宽）
//!
//! 只排除**第三方包代码**两个目录：
//!
//! 1. 活动内核安装树 `<data_dir>/kernels/<version>`（只读树，安装时按摘要
//!    校验过，里面没有用户数据）；
//! 2. 当前实例 profile 的 `node_modules`（纯第三方包代码）。
//!
//! **不排除**实例数据目录的其余部分（`sessions/`、`.credentials.yaml`、
//! `storages/`）——那是用户数据与凭据，让它继续被实时扫描。也不排除用户
//! 目录、系统临时目录、进程可执行文件。
//!
//! ## 代价（界面上必须写清）
//!
//! 排除之后这两个目录里的东西**不再被实时扫描**：装进来的第三方包若被篡改，
//! 杀软不会当场发现。这就是换来冷启动速度的代价。
//!
//! ## 第三方杀软
//!
//! 360 等没有公开可用的接口。改它们的私有配置文件既脆、又会被升级冲掉，
//! 等于绕过它们自己的界面。所以这里只**列出要加信任区的路径**并给一句指引，
//! 由用户自己在它的界面里加一次；加完之后同样受益。
//!
//! ## 提权
//!
//! 改 Defender 排除项要管理员。用 `Start-Process -Verb RunAs` 起一个提权
//! 的 `powershell.exe`（UAC 弹窗由用户点「是」），等它跑完并取回退出码。
//! 载荷经 `-EncodedCommand`（UTF-16LE + base64）传：只含 ASCII，不受
//! PowerShell 引号规则与 cmd 转发规则影响。

use std::path::{Path, PathBuf};
use std::time::Duration;

/// 事件日志的逻辑名（落在「查看日志」面板里）。
const EVENT_LOG: &str = "av-trust";

/// 一次测量建多少个探针文件。
const PROBE_FILES: usize = 100;

/// 提权等待上限。UAC 弹窗要用户点「是」，默认的 30 秒不够。
const ELEVATION_TIMEOUT: Duration = Duration::from_secs(120);

/// 面板读到的「杀软排除项」现状。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AvTrustStatus {
    /// 当前平台是否支持（只有 Windows 有「排除项」这回事）。
    pub supported: bool,
    /// 非 Windows 平台的一句话说明（Windows 上为空串）。
    pub platform_note: String,
    /// 系统里有没有 Defender。
    pub defender_present: bool,
    /// Defender 实时保护是否开着。
    pub defender_realtime: bool,
    /// 检出的第三方杀软（按驱动 / 服务名认，不按进程名猜）。
    pub third_party: Vec<ThirdPartyAv>,
    /// Defender 当前生效的排除项（注册表里 `Exclusions\Paths` 的值名）。
    pub exclusions: Vec<String>,
    /// 上面这份清单**读到了没有**。Windows 把 `Exclusions` 子树的 ACL 收紧到
    /// 管理员，普通用户读不动（实测本机即如此）——读不到时清单是空的，界面
    /// 必须说「读不到」，不能让它看起来像「一条排除项都没有」。
    pub exclusions_readable: bool,
    /// 要排除的两个目录及其当前状态。
    pub targets: Vec<ExclusionTarget>,
}

/// 检出的第三方杀软。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThirdPartyAv {
    /// 产品名。
    pub name: String,
    /// 命中它的驱动 / 服务名。
    pub drivers: Vec<String>,
}

/// 一个要排除的目录。
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExclusionTarget {
    /// 人类可读的标签。
    pub label: String,
    /// 绝对路径原文。
    pub path: String,
    /// 是否已经在 Defender 的排除项里。
    pub excluded: bool,
    /// 这个目录现在存在吗（不存在就没什么可扫的）。
    pub exists: bool,
}

/// 内部用的目标目录（标签 + 路径）。
#[derive(Debug, Clone, PartialEq, Eq)]
struct Target {
    label: &'static str,
    path: PathBuf,
}

/// 要排除的目录（标签 + 路径）。**纯函数**：两个路径显式传进来，
/// 单测因此不必碰用户真实的数据目录。
///
/// 只收第三方包代码：内核安装树与实例 profile 的 `node_modules`。实例数据
/// 目录的其余部分（会话、凭据、storages）是用户数据，**不在**这里。
fn targets(kernel_tree: Option<&Path>, profile_modules: Option<&Path>) -> Vec<Target> {
    let mut out = Vec::new();
    if let Some(path) = kernel_tree {
        out.push(Target {
            label: "内核安装树",
            path: path.to_path_buf(),
        });
    }
    if let Some(path) = profile_modules {
        out.push(Target {
            label: "实例依赖包",
            path: path.to_path_buf(),
        });
    }
    out
}

/// 活动内核安装树：`<data_dir>/kernels/<version>`。
fn kernel_tree_dir(data_dir: &Path, version: &str) -> PathBuf {
    crate::kernel::lifecycle::kernel_dir(data_dir, version)
}

/// 实例 profile 的第三方依赖包目录：`<instance_dir>/profiles/<profile>/node_modules`。
///
/// profile 名必须先过 `validate_id_component`——`paths::instance_profile_dir`
/// 内部是 `expect`，一个非法 profile 名会让整个壳崩掉。
fn profile_modules_dir(family: &str, instance_id: &str, profile: &str) -> Option<PathBuf> {
    crate::shell::paths::validate_id_component(profile).ok()?;
    Some(
        crate::shell::paths::instance_profile_dir(family, instance_id, profile)
            .join("node_modules"),
    )
}

/// 当前机器上要排除的两个目录。
fn resolve_targets() -> Vec<Target> {
    let (family, instance_id) = crate::shell::instance::resolve_default();
    let data_dir = crate::kernel::lifecycle::data_dir(family);
    let kernel_tree = crate::kernel::lifecycle::read_active(&data_dir)
        .map(|version| kernel_tree_dir(&data_dir, &version));
    let profile =
        crate::shell::settings::load_for_shell(crate::shell::settings::current_mode()).profile;
    let profile_modules = profile_modules_dir(family, instance_id, &profile);
    targets(kernel_tree.as_deref(), profile_modules.as_deref())
}

/// 两个路径是不是同一个目录。
///
/// Windows 上路径大小写不敏感，而排除项里存的路径可能带尾随分隔符、也可能
/// 与我们算出来的写法差一个反斜杠——按字面比会把「已经排除了」读成
/// 「没排除」，用户于是反复点「加入排除项」。
fn same_path(a: &str, b: &str) -> bool {
    fn norm(value: &str) -> String {
        // Windows 上两种分隔符等价（`C:/a` 与 `C:\a` 是同一个目录），
        // 排除项里存的路径可能带尾随分隔符、也可能与我们算出来的写法差一个
        // 反斜杠——按字面比会把「已经排除了」读成「没排除」。
        value
            .replace('/', "\\")
            .trim_end_matches('\\')
            .trim()
            .to_lowercase()
    }
    norm(a) == norm(b)
}

/// 把目标目录与当前排除项合成面板要显示的清单（纯函数）。
fn mark_targets(targets: Vec<Target>, exclusions: &[String]) -> Vec<ExclusionTarget> {
    targets
        .into_iter()
        .map(|target| {
            let path = target.path.display().to_string();
            let excluded = exclusions.iter().any(|item| same_path(item, &path));
            ExclusionTarget {
                label: target.label.to_string(),
                path,
                excluded,
                exists: target.path.is_dir(),
            }
        })
        .collect()
}

/// 面板读到的现状（只读）。
pub fn status() -> Result<AvTrustStatus, String> {
    Ok(build_status(platform::exclusion_paths()))
}

/// 组装面板要显示的状态。`exclusions` 为 `None` 表示**读不到**（需要管理员），
/// 不能拿空清单冒充「一条排除项都没有」。
fn build_status(exclusions: Option<Vec<String>>) -> AvTrustStatus {
    let readable = exclusions.is_some();
    let list = exclusions.unwrap_or_default();
    AvTrustStatus {
        supported: cfg!(target_os = "windows"),
        platform_note: if cfg!(target_os = "windows") {
            String::new()
        } else {
            "本平台不需要：只有 Windows 的杀软有「排除项 / 信任区」这回事。".to_string()
        },
        defender_present: platform::defender_present(),
        defender_realtime: platform::defender_realtime(),
        third_party: platform::third_party_av(),
        targets: mark_targets(resolve_targets(), &list),
        exclusions: list,
        exclusions_readable: readable,
    }
}

/// 加 / 删排除项（需要管理员，走 UAC 提权）。
///
/// 返回**生效后的状态**，让面板一次往返就能回显。失败时给的是带下一步的
/// 完整句子——GUI 程序里 `eprintln!` 没有去处，用户只拿得到这个字符串。
fn apply_exclusion(add: bool) -> Result<AvTrustStatus, String> {
    if !cfg!(target_os = "windows") {
        return Err("本平台不需要：只有 Windows 的杀软有「排除项 / 信任区」这回事。".to_string());
    }
    let all = resolve_targets();
    // 加入时只排存在着的目录（Defender 拒收不存在的路径）；移除时不排——
    // 目录没了但排除项还在，正是移除该做的事。
    let paths: Vec<String> = if add {
        all.iter()
            .filter(|target| target.path.is_dir())
            .map(|target| target.path.display().to_string())
            .collect()
    } else {
        all.iter()
            .map(|target| target.path.display().to_string())
            .collect()
    };
    if paths.is_empty() {
        return Err(if add {
            "还没有可排除的目录：内核尚未安装，或实例依赖目录还不存在。\
             请先在「内核版本」页装好内核，再回到这里加入排除项。"
                .to_string()
        } else {
            "没有可移除的排除项：内核尚未安装。".to_string()
        });
    }

    crate::shell::shell_events::record(
        EVENT_LOG,
        &format!(
            "{}排除项：{}",
            if add { "加入" } else { "移除" },
            paths.join("；")
        ),
    );

    let report_path = std::env::temp_dir().join(format!(
        "dsh-xlink-avtrust-{}-{}.txt",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    let script = elevated_script(add, &paths, &report_path.display().to_string());
    let encoded = encode_powershell(&outer_script(&encode_powershell(&script)));

    let mut cmd = crate::shell::process::command_with_path("powershell.exe");
    cmd.arg("-NoProfile").arg("-EncodedCommand").arg(&encoded);
    let (_ok, stdout, stderr) = crate::shell::process::run_command_capture_with_timeout(
        cmd,
        "powershell",
        ELEVATION_TIMEOUT,
    )
    .map_err(|error| {
        format!(
            "调用 PowerShell 失败：{error}。\
             请确认系统里有 powershell.exe（Windows 自带），再点一次；\
             若反复失败，打开「查看日志」把 av-trust 那几行发给维护者。"
        )
    })?;

    // 提权进程把逐条结果写进报告文件（它的 stdout 落在自己的控制台里，
    // 我们拿不到），退出码与「UAC 被拒」则从外层脚本的 stdout 读。
    let report = std::fs::read_to_string(&report_path).unwrap_or_default();
    let _ = std::fs::remove_file(&report_path);
    let report = report.trim_start_matches('\u{feff}');

    let mut exit_code: Option<i32> = None;
    let mut launch_error: Option<String> = None;
    for line in stdout.lines() {
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("EXITCODE=") {
            exit_code = rest.trim().parse::<i32>().ok();
        } else if let Some(rest) = line.strip_prefix("LAUNCHERROR=") {
            launch_error = Some(rest.trim().to_string());
        }
    }

    if let Some(message) = launch_error {
        crate::shell::shell_events::record(EVENT_LOG, &format!("提权未获批准：{message}"));
        return Err(format!(
            "系统没有批准管理员授权（UAC）：{message}。\
             请重新点一次，并在弹出的授权窗口里选「是」；\
             若不想授权，可手动在「Windows 安全中心 → 病毒和威胁防护 → 管理设置 → 排除项」\
             里添加上面列出的目录。"
        ));
    }

    let mut failures: Vec<String> = Vec::new();
    let mut applied = 0usize;
    // 提权进程顺手把**改完之后**的排除项清单报回来：普通用户读不动
    // Defender 的 Exclusions 子树，只有它读得到。清单读不到时是 `None`，
    // 界面据此说「读不到」，而不是显示成「一条都没有」。
    let mut listed: Vec<String> = Vec::new();
    let mut list_readable = false;
    for line in report.lines() {
        let mut parts = line.splitn(3, '\t');
        match (parts.next(), parts.next()) {
            (Some("OK"), Some(_path)) => applied += 1,
            (Some("ERR"), Some(path)) => {
                let reason = parts.next().unwrap_or("未知原因").trim();
                failures.push(format!("{path}：{reason}"));
            }
            (Some("LISTOK"), _) => list_readable = true,
            (Some("LIST"), Some(path)) => listed.push(path.to_string()),
            (Some("LISTERR"), _) => list_readable = false,
            _ => {}
        }
    }

    if !failures.is_empty() {
        let detail = failures.join("；");
        crate::shell::shell_events::record(EVENT_LOG, &format!("排除项改动失败：{detail}"));
        return Err(format!(
            "Defender 拒绝了这次改动：{detail}。\
             下一步：确认「Windows 安全中心」里 Defender 正在运行、且「篡改防护」没有拦下这次修改；\
             也可以手动在「病毒和威胁防护 → 管理设置 → 排除项」里添加上面列出的目录。"
        ));
    }
    if exit_code.is_some_and(|code| code != 0) {
        // stderr 里常混着 PowerShell 的 CLIXML 进度噪声（模块首次加载），那不是
        // 给人看的错误——滤掉，只留正常文本的前一小段。
        let detail: String = stderr
            .lines()
            .filter(|line| {
                let trimmed = line.trim_start();
                !trimmed.starts_with('<') && !trimmed.starts_with("#<")
            })
            .collect::<Vec<_>>()
            .join(" ")
            .chars()
            .take(300)
            .collect();
        crate::shell::shell_events::record(
            EVENT_LOG,
            &format!("排除项改动失败：退出码 {exit_code:?}，stderr：{detail}"),
        );
        return Err(format!(
            "排除项改动没有成功（提权进程退出码 {exit_code:?}）。\
             {detail}。下一步：确认「Windows 安全中心 → 病毒和威胁防护」里的 Defender 正在运行，\
             然后重试；仍失败请打开「查看日志」把 av-trust 那几行发给维护者。"
        ));
    }
    if applied == 0 {
        crate::shell::shell_events::record(
            EVENT_LOG,
            &format!(
                "{}排除项：没有实际改动（报告为空）",
                if add { "加入" } else { "移除" }
            ),
        );
        return Err("这次没有改动任何排除项（提权进程没有回报结果）。\
             下一步：确认「Windows 安全中心 → 病毒和威胁防护」里的 Defender 正在运行，然后重试；\
             也可以手动在「管理设置 → 排除项」里添加上面列出的目录。"
            .to_string());
    }

    crate::shell::shell_events::record(
        EVENT_LOG,
        &format!(
            "已{}排除项：{}",
            if add { "加入" } else { "移除" },
            paths.join("；")
        ),
    );
    // 用提权进程报回来的清单：它是改完之后的状态，也是这台机器上唯一读得到
    // 排除项的地方。报不回来时退回普通读（多半读不到）。
    if list_readable {
        Ok(build_status(Some(listed)))
    } else {
        status()
    }
}

/// 本机「首次打开一个文件」的实测耗时（毫秒 / 文件）。
///
/// 造一批**当场新建**的临时文件，每个只 open 一次就关（不读内容），取平均。
/// 刻意不去碰内核树或程序目录里的文件——那些早被扫过，测出来是热态
/// （本机 0.07ms），没有参考价值。
///
/// 临时目录建在系统临时目录里，用完自己删。
pub fn measure_open_cost(count: usize) -> Result<f64, String> {
    let dir = std::env::temp_dir().join(format!(
        "dsh-xlink-avprobe-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&dir)
        .map_err(|error| format!("无法在系统临时目录建探针目录 {}：{error}", dir.display()))?;
    let result = probe_open_cost(&dir, count);
    let _ = std::fs::remove_dir_all(&dir);
    result
}

/// 探针本体：先建好全部文件，再逐个计时打开（创建本身也可能被扫，不计入测量）。
fn probe_open_cost(dir: &Path, count: usize) -> Result<f64, String> {
    let mut files = Vec::with_capacity(count);
    for index in 0..count {
        let path = dir.join(format!("probe-{index}.bin"));
        std::fs::write(&path, b"dsh-xlink antivirus open-cost probe")
            .map_err(|error| format!("无法写探针文件 {}：{error}", path.display()))?;
        files.push(path);
    }
    Ok(average_open_cost(count, |index| {
        let started = std::time::Instant::now();
        {
            // 每个文件只 open 一次就关，不读内容。块结束时句柄被关闭，
            // 所以 close 的代价也算在这一次里（与「open+close」的实测口径一致）。
            let _file = std::fs::File::open(&files[index]);
        }
        started.elapsed().as_secs_f64() * 1000.0
    }))
}

/// 打开一次一个文件的平均耗时（毫秒）。
///
/// `open_once` 每次被调用一次，返回那一次 open+close 的耗时（毫秒）。抽成
/// 可注入的纯函数是为了能测：真实计时要碰文件系统，而单测不该依赖真机杀软
/// 的行为。
fn average_open_cost<F: FnMut(usize) -> f64>(count: usize, mut open_once: F) -> f64 {
    if count == 0 {
        return 0.0;
    }
    let mut total = 0.0;
    for index in 0..count {
        total += open_once(index);
    }
    total / count as f64
}

/// PowerShell 单引号串：`'` 要写成 `''`。
fn ps_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

/// 提权后那段 PowerShell（加 / 删排除项）。
///
/// 逐条 try/catch 并把结果写进报告文件：提权进程的 stdout 落在它自己的
/// 控制台里，我们拿不到，只有文件能穿回来。
fn elevated_script(add: bool, paths: &[String], report_path: &str) -> String {
    let list = paths
        .iter()
        .map(|path| ps_quote(path))
        .collect::<Vec<_>>()
        .join(", ");
    let action = if add {
        "Add-MpPreference"
    } else {
        "Remove-MpPreference"
    };
    // 移除时先看它在不在排除项里——Remove-MpPreference 对没排过的路径会报错。
    let skip = if add {
        "$false".to_string()
    } else {
        "($current -notcontains $p)".to_string()
    };
    format!(
        "$ErrorActionPreference = 'Continue'\n\
$lines = @()\n\
$current = @()\n\
try {{ $current = @(Get-MpPreference).ExclusionPath }} catch {{ $current = @() }}\n\
foreach ($p in @({list})) {{\n\
  try {{\n\
    if ({skip}) {{ $lines += ('SKIP' + \"`t\" + $p) }}\n\
    else {{ {action} -ExclusionPath $p -ErrorAction Stop; $lines += ('OK' + \"`t\" + $p) }}\n\
  }}\n\
  catch {{ $lines += ('ERR' + \"`t\" + $p + \"`t\" + $_.Exception.Message) }}\n\
}}\n\
try {{ $after = @(Get-MpPreference).ExclusionPath; $lines += 'LISTOK'; foreach ($p in $after) {{ $lines += ('LIST' + \"`t\" + $p) }} }}\n\
catch {{ $lines += 'LISTERR' }}\n\
Set-Content -LiteralPath {report} -Value $lines -Encoding UTF8\n",
        list = list,
        skip = skip,
        action = action,
        report = ps_quote(report_path),
    )
}

/// 外层那段 PowerShell：起一个提权的 `powershell.exe` 跑内层脚本，等它结束，
/// 把退出码写回 stdout。UAC 被拒时 `Start-Process` 抛错，这里接住并回报。
fn outer_script(inner_encoded: &str) -> String {
    format!(
        "$ErrorActionPreference = 'Stop'\n\
try {{\n\
  $p = Start-Process -FilePath 'powershell.exe' -ArgumentList '-NoProfile','-NonInteractive','-EncodedCommand','{encoded}' -Verb RunAs -Wait -PassThru -WindowStyle Hidden\n\
  Write-Output ('EXITCODE=' + $p.ExitCode)\n\
}}\n\
catch {{\n\
  Write-Output ('LAUNCHERROR=' + $_.Exception.Message)\n\
}}\n",
        encoded = inner_encoded,
    )
}

/// 把一段 PowerShell 脚本编成 `-EncodedCommand` 的载荷（UTF-16LE + base64）。
///
/// 只含 ASCII，因此不受引号规则与 cmd 转发规则影响——路径里有空格、单引号、
/// 中文都不会被拆开。
fn encode_powershell(script: &str) -> String {
    use base64::Engine;
    let bytes: Vec<u8> = script
        .encode_utf16()
        .flat_map(|unit| unit.to_le_bytes())
        .collect();
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

/// 面板读到的杀软排除项现状（只读）。
#[tauri::command]
pub async fn av_trust_status() -> Result<AvTrustStatus, String> {
    crate::commands::blocking(status).await
}

/// 加入 / 移除 Defender 排除项，返回生效后的状态。
#[tauri::command]
pub async fn av_trust_set_exclusion(enabled: bool) -> Result<AvTrustStatus, String> {
    crate::commands::blocking(move || apply_exclusion(enabled)).await
}

/// 现测本机「首次打开一个文件」的耗时（毫秒 / 文件）。
#[tauri::command]
pub async fn av_trust_measure_open_cost() -> Result<f64, String> {
    crate::commands::blocking(move || measure_open_cost(PROBE_FILES)).await
}

/// 平台相关的只读探测。
#[cfg(target_os = "windows")]
mod platform {
    use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_MORE_DATA, ERROR_SUCCESS};
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegEnumValueW, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_LOCAL_MACHINE,
        KEY_READ, REG_DWORD,
    };

    /// Defender 排除项（路径）所在的位置。
    ///
    /// **读它可能要管理员**：Windows 把 `Windows Defender\Exclusions` 子树的
    /// ACL 收紧到管理员 / SYSTEM，普通用户 `RegOpenKeyExW` 直接被拒（实测
    /// 本机即如此；Defender 自己的 `Get-MpPreference` 也回 "Must be an
    /// administrator to view exclusions"）。所以读不到时必须**说读不到**，
    /// 不能拿空清单冒充「一条排除项都没有」。
    const EXCLUSIONS_KEY: &str = r"SOFTWARE\Microsoft\Windows Defender\Exclusions\Paths";
    /// 实时保护开关（1 = 关）。
    const REALTIME_KEY: &str = r"SOFTWARE\Microsoft\Windows Defender\Real-Time Protection";
    const REALTIME_VALUE: &str = "DisableRealtimeMonitoring";
    /// 「禁用 Defender」策略（1 = Defender 整个不跑）。
    const DISABLE_ANTISPYWARE_KEY: &str = r"SOFTWARE\Policies\Microsoft\Windows Defender";
    const DISABLE_ANTISPYWARE_VALUE: &str = "DisableAntiSpyware";
    /// Defender 的驱动服务：它不在就不是 Defender。
    const DEFENDER_SERVICE: &str = r"SYSTEM\CurrentControlSet\Services\WinDefend";

    /// 第三方杀软：按**驱动 / 服务名**认。
    ///
    /// 只列「装了就一定在」的文件系统过滤驱动与常驻服务，不按进程名或窗口
    /// 标题猜——那会把任何同名程序误报成杀软。
    const THIRD_PARTY_AV: &[(&str, &[&str])] = &[
        (
            "360 安全卫士",
            &["360FsFlt", "360Box64", "360AntiHacker", "qhdrv"],
        ),
        ("火绒安全", &["HipsDrv", "SysDiag"]),
        ("金山毒霸 / 猎豹", &["kwshita", "ksapi"]),
        ("腾讯电脑管家", &["QMBASE", "QQSysMon"]),
        ("Avast", &["asvfs"]),
        ("AVG", &["avgfile"]),
        ("Avira", &["avrnfs"]),
        ("ESET", &["epfw"]),
        ("卡巴斯基", &["klif"]),
        ("Bitdefender", &["bdatr"]),
        ("McAfee", &["mfehidk"]),
        ("Norton / Symantec", &["eeCtrl"]),
        ("Sophos", &["savxdi"]),
    ];

    /// 打开 HKLM 下的一个键（只读）。
    fn open_key(subkey: &str) -> Result<HKEY, u32> {
        let wide: Vec<u16> = subkey.encode_utf16().chain(std::iter::once(0)).collect();
        let mut key = std::ptr::null_mut();
        // SAFETY: 只写 `key`；参数是本函数持有的 NUL 结尾缓冲。
        let status =
            unsafe { RegOpenKeyExW(HKEY_LOCAL_MACHINE, wide.as_ptr(), 0, KEY_READ, &mut key) };
        if status != ERROR_SUCCESS {
            return Err(status);
        }
        Ok(key)
    }

    /// 键存在吗？
    pub(super) fn key_exists(subkey: &str) -> bool {
        match open_key(subkey) {
            Ok(key) => {
                // SAFETY: `key` 由 `open_key` 返回且未关闭过。
                unsafe { RegCloseKey(key) };
                true
            }
            Err(_) => false,
        }
    }

    /// 枚举一个键下的全部**值名**（排除项的路径就是值名）。
    ///
    /// 键打不开时返回空清单——调用方要区分「读不到」与「没有」，走
    /// [`exclusion_paths`] 那条路。
    fn value_names(key: HKEY) -> Vec<String> {
        let mut out = Vec::new();
        let mut index = 0u32;
        let mut name = vec![0u16; 512];
        loop {
            let mut len = name.len() as u32;
            let mut kind = 0u32;
            // SAFETY: 缓冲由本函数持有；`len` 是 in/out（进：容量，出：实际长度）。
            let status = unsafe {
                RegEnumValueW(
                    key,
                    index,
                    name.as_mut_ptr(),
                    &mut len,
                    std::ptr::null(),
                    &mut kind,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                )
            };
            if status == ERROR_MORE_DATA {
                // 值名比缓冲长：按它要的长度重开一次，仍停在同一个下标。
                name = vec![0u16; len as usize + 2];
                continue;
            }
            if status != ERROR_SUCCESS {
                break;
            }
            let text = String::from_utf16_lossy(&name[..(len as usize).min(name.len())]);
            let text = text.trim_end_matches('\0').to_string();
            if !text.is_empty() {
                out.push(text);
            }
            index += 1;
        }
        out
    }

    /// 读一个 REG_DWORD 值。
    fn read_dword(subkey: &str, value: &str) -> Option<u32> {
        let key = open_key(subkey).ok()?;
        let wide: Vec<u16> = value.encode_utf16().chain(std::iter::once(0)).collect();
        let mut kind = 0u32;
        let mut data = 0u32;
        let mut size = std::mem::size_of::<u32>() as u32;
        // SAFETY: `data` 恰好 4 字节，`size` 是 in/out。
        let status = unsafe {
            RegQueryValueExW(
                key,
                wide.as_ptr(),
                std::ptr::null(),
                &mut kind,
                (&mut data as *mut u32).cast::<u8>(),
                &mut size,
            )
        };
        // SAFETY: `key` 有效且尚未关闭。
        unsafe { RegCloseKey(key) };
        if status != ERROR_SUCCESS || kind != REG_DWORD {
            return None;
        }
        Some(data)
    }

    /// Defender 当前生效的排除项（路径）。
    ///
    /// `None` = **读不到**（Windows 把 `Exclusions` 子树收给管理员了，
    /// 实测本机普通用户 `RegOpenKeyExW` 直接被拒）；`Some(空)` = 读到了，
    /// 确实一条排除项都没有。这两件事在界面上必须分开说。
    pub(super) fn exclusion_paths() -> Option<Vec<String>> {
        match open_key(EXCLUSIONS_KEY) {
            Ok(key) => {
                let names = value_names(key);
                // SAFETY: `key` 有效且尚未关闭。
                unsafe { RegCloseKey(key) };
                Some(names)
            }
            // 键不存在 = 还没有任何排除项（读得到，只是空的）。
            Err(status) if status == ERROR_FILE_NOT_FOUND => Some(Vec::new()),
            // 其余（多半是 ACCESS_DENIED）= 读不到。
            Err(_) => None,
        }
    }

    /// 系统里有没有 Defender。
    pub(super) fn defender_present() -> bool {
        key_exists(DEFENDER_SERVICE)
    }

    /// Defender 实时保护是否开着。
    ///
    /// 读注册表里的 `DisableRealtimeMonitoring`（1 = 关）。键 / 值不存在时按
    /// **开着**处理——那是 Defender 的默认（实测本机：该值不存在，而
    /// `Get-MpComputerStatus` 报 `RealTimeProtectionEnabled=True`）。
    /// 「禁用 Defender」策略（`DisableAntiSpyware`）也一并看：它一开，
    /// Defender 整个不跑，实时保护当然谈不上。
    pub(super) fn defender_realtime() -> bool {
        if !defender_present() {
            return false;
        }
        if read_dword(DISABLE_ANTISPYWARE_KEY, DISABLE_ANTISPYWARE_VALUE) == Some(1) {
            return false;
        }
        match read_dword(REALTIME_KEY, REALTIME_VALUE) {
            Some(value) => value == 0,
            None => true,
        }
    }

    /// 检出的第三方杀软。
    pub(super) fn third_party_av() -> Vec<super::ThirdPartyAv> {
        let mut out = Vec::new();
        for (name, drivers) in THIRD_PARTY_AV {
            let found: Vec<String> = drivers
                .iter()
                .filter(|driver| {
                    key_exists(&format!(
                        r"SYSTEM\CurrentControlSet\Services\{driver}",
                        driver = driver
                    ))
                })
                .map(|driver| driver.to_string())
                .collect();
            if !found.is_empty() {
                out.push(super::ThirdPartyAv {
                    name: name.to_string(),
                    drivers: found,
                });
            }
        }
        out
    }
}

/// 平台相关的只读探测。非 Windows 上什么都没有，界面显示「本平台不需要」。
#[cfg(not(target_os = "windows"))]
mod platform {
    pub(super) fn exclusion_paths() -> Option<Vec<String>> {
        Some(Vec::new())
    }

    pub(super) fn defender_present() -> bool {
        false
    }

    pub(super) fn defender_realtime() -> bool {
        false
    }

    pub(super) fn third_party_av() -> Vec<super::ThirdPartyAv> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::scoped_xlink_home;

    /// 目标目录只收**第三方包代码**：内核安装树 + 实例 profile 的 node_modules。
    /// 少一个就少一份收益，多一个就越界（用户数据与凭据不该被免检）。
    #[test]
    fn targets_are_the_two_third_party_code_directories() {
        let list = targets(
            Some(Path::new("/data/kernels/0.2.2")),
            Some(Path::new(
                "/data/instances/default/profiles/web/node_modules",
            )),
        );
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].label, "内核安装树");
        assert_eq!(list[0].path, Path::new("/data/kernels/0.2.2"));
        assert_eq!(list[1].label, "实例依赖包");
        assert!(list[1].path.ends_with("node_modules"));
    }

    /// 缺哪一项都只给剩下的那一个，不许拿别的东西顶替。
    #[test]
    fn targets_skip_the_missing_ones() {
        assert!(targets(None, None).is_empty());
        assert_eq!(targets(None, Some(Path::new("/m"))).len(), 1);
        assert_eq!(targets(Some(Path::new("/k")), None).len(), 1);
    }

    /// 实例依赖包目录必须落在实例的 profile 下，且**不能**把实例数据目录的
    /// 其余部分（会话、凭据）圈进来。
    #[test]
    fn profile_modules_dir_is_under_the_profile() {
        let home = std::env::temp_dir().join(format!(
            "dsh-av-trust-home-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let _guard = scoped_xlink_home(&home);
        let dir = profile_modules_dir("dsh", "default", "web").expect("合法 profile");
        assert!(
            dir.ends_with("profiles/web/node_modules"),
            "{}",
            dir.display()
        );
        assert!(dir.starts_with(&home), "路径必须在 xlink_home 下");
        assert!(
            !dir.display().to_string().contains("sessions"),
            "不能把会话目录圈进排除项：{}",
            dir.display()
        );
    }

    /// 非法 profile 名不能让整个壳崩掉（`paths::instance_profile_dir` 内部是
    /// `expect`），这里必须安静地跳过这一项。
    #[test]
    fn profile_modules_dir_rejects_invalid_profile() {
        assert!(profile_modules_dir("dsh", "default", "../escape").is_none());
        assert!(profile_modules_dir("dsh", "default", "").is_none());
    }

    /// 内核安装树就是 `<data_dir>/kernels/<version>`。
    #[test]
    fn kernel_tree_dir_is_under_kernels() {
        let dir = kernel_tree_dir(Path::new("/data"), "0.2.2-alpha.2");
        assert!(dir.ends_with("kernels/0.2.2-alpha.2"), "{}", dir.display());
    }

    /// 排除项的判定要扛住大小写与尾随分隔符：Defender 存下来的写法与我们
    /// 算出来的可能差一个反斜杠，按字面比会把「已排除」读成「未排除」。
    #[test]
    fn same_path_ignores_case_and_trailing_separators() {
        assert!(same_path(
            r"C:\Data\kernels\0.2.2",
            r"c:\data\kernels\0.2.2\"
        ));
        assert!(same_path("C:/data/kernels", "C:\\data\\kernels"));
        assert!(!same_path(r"C:\data\kernels", r"C:\data\kernels\0.2.2"));
    }

    /// 面板要显示的清单：已排除 / 未排除、目录在不在。
    #[test]
    fn mark_targets_reads_the_current_exclusions() {
        let list = mark_targets(
            targets(
                Some(Path::new(r"C:\data\kernels\0.2.2")),
                Some(Path::new(r"C:\data\profiles\web\node_modules")),
            ),
            &[r"C:\data\kernels\0.2.2\".to_string()],
        );
        assert!(list[0].excluded, "首项应当被判为已排除");
        assert!(!list[1].excluded, "第二项不在排除项里");
        // 目录不存在时如实说不存在。
        assert!(!list[0].exists);
    }

    /// 平均值就是平均值：注入固定耗时，不依赖真机杀软行为。
    #[test]
    fn average_open_cost_averages_the_injected_costs() {
        let value = average_open_cost(4, |index| match index {
            0 => 1.0,
            1 => 2.0,
            2 => 3.0,
            _ => 4.0,
        });
        assert!((value - 2.5).abs() < 1e-9, "实际 {value}");
    }

    /// 零个文件不许除零。
    #[test]
    fn average_open_cost_of_zero_files_is_zero() {
        assert_eq!(average_open_cost(0, |_| 1.0), 0.0);
    }

    /// 计时探针必须落在系统临时目录里、用完自己删——不许碰用户数据目录。
    #[test]
    fn probe_runs_in_the_system_temp_dir_and_cleans_up() {
        let before = std::fs::read_dir(std::env::temp_dir())
            .map(|entries| entries.count())
            .unwrap_or(0);
        let value = measure_open_cost(3).expect("探针应能跑完");
        assert!(value >= 0.0);
        let after = std::fs::read_dir(std::env::temp_dir())
            .map(|entries| entries.count())
            .unwrap_or(0);
        assert_eq!(before, after, "探针目录必须自己删掉");
    }

    /// PowerShell 单引号串里的 `'` 要加倍，否则路径里的单引号会提前闭合字符串。
    #[test]
    fn ps_quote_doubles_single_quotes() {
        assert_eq!(ps_quote(r"C:\a'b"), r"'C:\a''b'");
        assert_eq!(ps_quote(r"C:\a"), r"'C:\a'");
    }

    /// 加与删必须是官方那两条命令，且路径逐字进去。
    #[test]
    fn elevated_script_uses_the_official_cmdlets() {
        let add = elevated_script(true, &[r"C:\data\kernels\0.2.2".to_string()], r"C:\t\r.txt");
        assert!(add.contains("Add-MpPreference -ExclusionPath $p"));
        assert!(!add.contains("Remove-MpPreference -ExclusionPath $p"));
        assert!(add.contains(r"'C:\data\kernels\0.2.2'"));
        assert!(add.contains(r"'C:\t\r.txt'"), "报告路径要进脚本");

        let remove = elevated_script(
            false,
            &[r"C:\data\kernels\0.2.2".to_string()],
            r"C:\t\r.txt",
        );
        assert!(remove.contains("Remove-MpPreference -ExclusionPath $p"));
        assert!(
            remove.contains("$current -notcontains $p"),
            "移除前要先看它在不在"
        );
    }

    /// 载荷必须是纯 ASCII 的 base64（UTF-16LE）：路径里有空格、单引号、中文
    /// 都不会被 cmd / PowerShell 的转发规则拆开。
    #[test]
    fn encoded_payload_is_ascii_base64_of_utf16le() {
        use base64::Engine;
        let script = elevated_script(true, &[r"C:\我的 目录\kernels".to_string()], r"C:\t\r.txt");
        let encoded = encode_powershell(&script);
        assert!(encoded.is_ascii(), "载荷必须是 ASCII");
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&encoded)
            .expect("应能解回 base64");
        let expected: Vec<u8> = script
            .encode_utf16()
            .flat_map(|unit| unit.to_le_bytes())
            .collect();
        assert_eq!(bytes, expected, "载荷必须是脚本的 UTF-16LE 字节");
    }

    /// 外层脚本必须用 `-Verb RunAs` 提权，并把退出码回报出来。
    #[test]
    fn outer_script_elevates_and_reports_the_exit_code() {
        let outer = outer_script("QUJD");
        assert!(outer.contains("-Verb RunAs"));
        assert!(outer.contains("EXITCODE="));
        assert!(outer.contains("LAUNCHERROR="), "UAC 被拒也要有话可说");
        assert!(outer.contains("'QUJD'"));
    }

    /// 提权脚本必须把**改完之后**的排除项清单报回来：Windows 把
    /// `Exclusions` 子树收给管理员了，普通用户读不动——只有提权进程读得到。
    #[test]
    fn elevated_script_reports_the_resulting_exclusion_list() {
        let script = elevated_script(true, &[r"C:\data\kernels\0.2.2".to_string()], r"C:\t\r.txt");
        assert!(script.contains("'LISTOK'"), "读到了清单要有标记");
        assert!(
            script.contains("'LISTERR'"),
            "读不到也要有标记（不能当成空清单）"
        );
        assert!(script.contains("('LIST' + \"`t\" + $p)"));
    }

    /// 注册表读的是 HKLM 下 Defender 的键。**读不动时必须说读不动**，
    /// 不能拿空清单冒充「一条排除项都没有」——Windows 把 `Exclusions` 子树
    /// 收给管理员了，普通用户读它会被拒。
    #[cfg(target_os = "windows")]
    #[test]
    fn registry_reads_distinguish_missing_from_denied() {
        assert!(!platform::key_exists(
            r"SYSTEM\CurrentControlSet\Services\dsh-xlink-not-a-driver"
        ));
        // 真实的排除项键：读得到就是清单，读不到就是 None（不许假装是空的）。
        match platform::exclusion_paths() {
            Some(list) => {
                for path in list {
                    assert!(!path.is_empty(), "排除项的值名就是路径，不该为空");
                }
            }
            None => {
                // 读不到（要管理员）：这是允许的结果，但必须是 None 而不是空清单。
                assert!(
                    platform::defender_present(),
                    "Defender 服务在、却连排除项键都读不到时，应如实报「读不到」"
                );
            }
        }
        // 实时保护的判定：Defender 在时必须有结论（开或关），不许含糊。
        if platform::defender_present() {
            let _ = platform::defender_realtime();
        }
    }
}
