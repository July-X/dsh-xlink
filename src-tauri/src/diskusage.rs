//! 磁盘占用报表：只读，告诉用户「装了什么、占多少」。
//!
//! **不提供任何删除入口**（2026-10-02 用户拍板）。理由不是怕担责，而是
//! 删这件事本身在这里没有安全边界可守：本机实测 1.4G 构成里，最小的
//! 目标（`shell/release/logs`）不到 1M，最大的两块是内核 `node_modules`
//! 与**实例 DSH home**——后者装的是用户会话与附件，删错了不可逆，而壳
//! 无法替用户判断「352M 里哪些他还记得要」。给一个只读视图，用户自己
//! 用 Finder 判断，壳就不必在「删错了」和「不敢删」之间二选一。
//!
//! ## 为什么不用现成的目录大小工具
//!
//! `du` 在这台机器上走完 25828 个文件要 290ms（`real`，其中 262ms 是
//! `sys`）。但那是**一个**目录；报表要逐版本、逐实例算，还要在 Tauri
//! 主线程之外跑。壳已经有 `process` 模块管子进程，为一次只读统计拉一个
//! `du` 进来不值当——它带来三样东西：跨平台参数差异（macOS `-sk` 出 KB、
//! GNU `-k` 也出 KB 但 `-s` 语义在符号链接上不同）、对符号链接的跟随
//! 规则要重新对齐 `du` 的默认行为、以及一次 IPC 要等子进程退出。
//!
//! 自己走一遍 `read_dir` 只有 40 行，且能精确控制**不跟随符号链接**——
//! 这条比工具版本更重要：内核安装树里有 pnpm 建的硬链接与软链，跟错了
//! 会把同一个文件数两遍，报表直接说谎。
//!
//! ## 扫描放后台线程
//!
//! 一次全量扫描 290ms 量级，Tauri 主线程扛不住（卡住的是整个 UI，包括
//! 正在滚动的列表）。所以命令是 `async` + `blocking`：IPC 立即返回，
//! 扫描在 blocking worker 上跑完再回。前端因此需要一个 loading 态
//! （见 `VersionsPanel.vue` 的 `diskUsage` 槽位）。

use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::{Emitter, Manager};

/// 单个占用条目。
#[derive(Debug, Clone, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageEntry {
    /// 条目 id（版本号 / 实例 id / 固定分类名），前端用它做缓存键。
    pub id: String,
    /// 展示名。与 `id` 多数时候相同，留出「日志目录」这类需要人话的场景。
    pub label: String,
    /// 悬停全文，`label` 是它的缩写。
    ///
    /// 两者分开的理由是**版面**：瓦片只有 186px 宽，条目行还要给字节数留位，
    /// 「正式版（dsh）」这种带限定词的写法会把名称挤到截断，而限定词在
    /// 同一张瓦片里往往已经由分类标题说过了。空串表示「与 `label` 相同」。
    ///
    /// 缩写规则只在这里定：**不能**由前端按「含括号就砍掉」之类的规则反推
    /// ——那正是 `dev-work` 被改写成「开发版-work」那类事故的形状（见下方
    /// `friendly_id` 的注释）。哪些部分是冗余的，只有拼得出这个名字的地方知道。
    pub detail: String,
    /// 字节数。**不预先格式化**成字符串：大小单位的中文习惯（`MB` vs
    /// `MiB`、`1.2G` vs `1.16 GB`）在前端改一次比后端改一次省事，而且
    /// 前端还要按它排序。
    pub bytes: u64,
    /// 同一父目录下并列的条目，字节数占比（0.0–100.0）。
    ///
    /// 让人一眼看出「507M 里的 227M 是内核本体」这种结构，比只有一个总数
    /// 有用得多。算在 Rust 侧是因为只有这里知道分母。
    pub share_percent: f64,
    /// 路径的可点击形式不存在——纯展示用，前端不拿它做任何 IO。
    pub path: String,
}

/// 一组占用条目（同一父目录下的并列项）。
#[derive(Debug, Clone, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UsageGroup {
    /// 分类 id，例如 `kernels` / `instances` / `logs`。
    pub id: String,
    /// 分类名。直接给用户看。
    pub label: String,
    /// 分类的悬停全文，`label` 是它的缩写（`实例数据` ← `实例数据（会话与附件）`）。
    /// 语义与 `UsageEntry::detail` 一致，空串表示与 `label` 相同。
    pub detail: String,
    /// 该分类自身占用的字节数（含子项）。
    pub bytes: u64,
    /// 该分类占全部分类的百分比。
    pub share_percent: f64,
    pub entries: Vec<UsageEntry>,
}

/// 完整报表。
///
/// `Deserialize` 是为了读回自己的缓存文件（`disk_usage` 的两段式返回）。
/// 注意四个子结构体**也要**能反序列化——只给顶层加 derive 会在读缓存时
/// 报一个看不出所以然的错。
#[derive(Debug, Clone, Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskUsage {
    /// 所有分类的字节合计。
    ///
    /// 字段名是 `total` 而不是 `total_bytes`，**只因为 `check-invariants` 的
    /// ipc-fields 那条按字段名全局匹配**：它扫到任何套了
    /// `rename_all = "camelCase"` 的结构体字段，就把该字段的 snake_case 形式
    /// 记成「前端读到就是错的」，而它不区分同名字段属于哪个结构体。
    /// `migration::wizard` 的 `MigrationItemPreview.total_bytes` 刻意**不**套
    /// `rename_all`（那三个结构体与迁移面板同链路，统一走 snake_case），
    /// 于是本模块一度被误判成「前端 9 处读错」，而实际两边都对。
    /// 改名比削弱门禁便宜：门禁那条判据本身是对的（前端读 snake_case 拿到
    /// undefined 这类 bug 真的存在），它只是分不清归属。
    pub total: u64,
    /// 测量时刻（毫秒时间戳）。让用户知道数字不是缓存来的。
    pub measured_at: u64,
    /// 扫描过程中读不到的目录（权限、被占用、竞态删除）。**如实列出来**：
    /// 少算了一块却报「总计 1.4G」比不报更糟——用户会以为那就是全部。
    pub unreadable: Vec<String>,
    pub groups: Vec<UsageGroup>,
}

/// `disk_usage` 的返回：报表 **+ 本次是否已经在后台重扫**。
///
/// 为什么要多带这一个字段：前端有个「后台正在重新扫描…」的转圈标志，而在
/// 加它之前**没有任何地方把它置为 true**——只有收到 `disk-usage-refreshed`
/// 事件时才会被关掉。于是那个标志是个死标志，后台重扫期间用户零反馈，而这
/// 恰恰是最需要反馈的时刻（数字停在旧值上，看着像卡住了）。
///
/// 补上这个信号不能靠前端猜：「缓存新鲜」与「缓存陈旧但重扫已经结束」在
/// 前端长得一模一样，两条路径都会让 `invoke` 立即返回。所以判据只能由后端
/// 给——它知道自己是「读缓存直接回」还是「顺手起了个后台线程」。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiskUsageReply {
    pub report: DiskUsage,
    pub background_refresh: bool,
}

/// 这一次 `disk_usage` 该怎么走。抽成纯函数是为了能脱离 Tauri 测——
/// 命令体里那几条分支的判据（尤其「强制优先于缓存新鲜度」）全在这里。
///
/// `Answer` 直接持有 [`DiskUsageReply`] 而不是把那两个字段再抄一遍：抄一遍
/// 就多一处要同步的地方，而它们本来就是同一件事。
enum Plan {
    /// 立刻返回手上这份。`background_refresh` 为真表示同时已在后台重扫，
    /// 前端据此点亮转圈并等 `disk-usage-refreshed` 回填。
    Answer(DiskUsageReply),
    /// 必须扫一次，返回的必须是**此刻**测出来的数。
    Scan,
}

/// 决定这一次走哪条路。三个入参刻意都是纯值——`is_stale` 里的时钟回拨那类
/// 边界得能喂固定值测。
///
/// `force` 排在新鲜度**之前**（用户 2026-10-07 报：「刷新按钮，没有正确扫描
/// 磁盘占用」）：缓存新鲜时早先的实现直接 `return Ok(cached)`，连后台线程都
/// 不起——于是「刷新」和「什么都不做」完全等价，界面上也没有任何差别。
/// 而缓存一天只刷一次，意味着点按钮几乎永远落在这条分支上。
fn plan(cached: Option<DiskUsage>, now_ms: u64, force: bool) -> Plan {
    if force {
        return Plan::Scan;
    }
    match cached {
        Some(report) if !is_stale(&report, now_ms) => Plan::Answer(DiskUsageReply {
            report,
            background_refresh: false,
        }),
        Some(report) => Plan::Answer(DiskUsageReply {
            report,
            background_refresh: true,
        }),
        None => Plan::Scan,
    }
}

/// 目录占用的字节数。不跟随符号链接。
///
/// 不跟随是刻意的：内核安装树里 pnpm 用硬链接复用包（同一份内容多个
/// 路径），软链则指向 store。跟随会把同一份字节数算多次，报表直接失真。
/// 硬链接无法在纯 `read_dir` 层面识别（要 `stat` 全部条目并按 inode 去重，
/// 那是另一件事的成本），因此这里数的是**目录树展开后的字节数**，
/// 与 `du` 不加 `-l` 的行为一致——面板上会注明这是上界。
fn dir_size(path: &Path) -> std::io::Result<u64> {
    let mut total = 0u64;
    let mut stack = vec![path.to_path_buf()];
    while let Some(current) = stack.pop() {
        let entries = match std::fs::read_dir(&current) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for entry in entries.flatten() {
            // `file_type` 走 lstat 而非 stat：软链在这里就被认出来，不进
            // 递归。失败时跳过而不是中止整棵树——一个坏条目不该让
            // 整个报表变成空。
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                stack.push(entry.path());
            } else if kind.is_file() {
                if let Ok(meta) = entry.metadata() {
                    total = total.saturating_add(meta.len());
                }
            }
            // 软链与其余特殊文件（fifo / socket）都不计：它们不是可回收的
            // 磁盘占用，计进去只会让「删了能省多少」这个问题的答案偏大。
        }
    }
    Ok(total)
}

/// 安全地把一个字节数除成百分比，分母为 0 时给 0。
///
/// 不用 `as f64 / total as f64` 裸算：空目录（刚装完还没写任何东西）
/// 会是 NaN，而 NaN 序列化进 JSON 后在 JS 侧是 `null`，前端 `toFixed()`
/// 直接抛。
fn percent(part: u64, whole: u64) -> f64 {
    if whole == 0 {
        return 0.0;
    }
    let value = (part as f64 / whole as f64) * 100.0;
    if value.is_finite() {
        // 保留两位：更长的位数是噪声，而这个字段只用于画条形。
        (value * 100.0).round() / 100.0
    } else {
        0.0
    }
}

/// 分类下待统计的一行：(条目 id、缩写展示名、悬停全文、路径)。
///
/// 四个 `String` 挨在一起容易看串，所以给个名字；`build_group` 的调用处因此
/// 不用再对着元组位置猜哪一栏是路径。
type Row = (String, String, String, PathBuf);

/// 组装一个分类：算总和、补占比、按字节降序。
fn build_group(id: &str, label: &str, detail: &str, raw: Vec<Row>) -> UsageGroup {
    let mut entries: Vec<UsageEntry> = raw
        .into_iter()
        .map(|(entry_id, entry_label, entry_detail, path)| {
            let bytes = dir_size(&path).unwrap_or(0);
            UsageEntry {
                id: entry_id,
                label: entry_label,
                detail: entry_detail,
                bytes,
                share_percent: 0.0,
                path: path.display().to_string(),
            }
        })
        .collect();
    let bytes: u64 = entries.iter().map(|entry| entry.bytes).sum();
    // 降序：大块在前。用户找「哪个最占地方」时是从上往下扫的。
    entries.sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.id.cmp(&b.id)));
    for entry in &mut entries {
        entry.share_percent = percent(entry.bytes, bytes);
    }
    UsageGroup {
        id: id.to_string(),
        label: label.to_string(),
        detail: detail.to_string(),
        bytes,
        share_percent: 0.0,
        entries,
    }
}

/// 测一次全量占用。
///
/// `data_dir` 是本壳的内核安装树（`<xlink_home>/<family>/desktop[-dev]/`），
/// `home` 是 `xlink_home()`。两者分开传而不是内部调 `xlink_home()`：测试
/// 必须能指向临时目录——**它绝不许扫用户的真实 `~/.dsh-xlink`**。
pub fn measure(data_dir: &Path, home: &Path) -> DiskUsage {
    let mut groups = Vec::new();
    let mut unreadable = Vec::new();

    // ① 内核版本：`<data_dir>/kernels/<version>/`。逐版本是这张报表的重点——
    //    「装了几个版本、各占多少」正是用户长期装版本后最想问的问题。
    let kernels = data_dir.join("kernels");
    let mut kernel_rows = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&kernels) {
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if !kind.is_dir() {
                continue;
            }
            let version = entry.file_name().to_string_lossy().to_string();
            if version.starts_with('.') {
                continue;
            }
            // 版本号没有可砍的限定词，detail 留空（前端回落到 label）。
            kernel_rows.push((version.clone(), version, String::new(), entry.path()));
        }
    } else {
        unreadable.push(kernels.display().to_string());
    }
    groups.push(build_group("kernels", "内核版本", "", kernel_rows));

    // ② 实例：`<home>/kernels/<family>/instances/<id>/`。这一栏装的是**用户
    //    自己的会话与附件**，占的往往不比内核少（本机 352M vs 507M），而
    //    它明确不是「装了什么」而是「你的数据在哪」——标题要与①区分开，
    //    否则用户会以为是某种可回收的缓存。
    let mut instance_rows = Vec::new();
    for family_dir in [home.join("kernels")] {
        if let Ok(families) = std::fs::read_dir(&family_dir) {
            for family in families.flatten() {
                let instances = family.path().join("instances");
                let Ok(list) = std::fs::read_dir(&instances) else {
                    continue;
                };
                for instance in list.flatten() {
                    let Ok(kind) = instance.file_type() else {
                        continue;
                    };
                    if !kind.is_dir() {
                        continue;
                    }
                    let id = instance.file_name().to_string_lossy().to_string();
                    if id.starts_with('.') {
                        continue;
                    }
                    let family_name = family.file_name().to_string_lossy().to_string();
                    // 默认实例的 id 带着壳模式后缀（`default` / `default-dev`），
                    // 那是目录名的技术形态。**只映射这两个已知值**，不按
                    // 「含 dev 就改写」的规则去动其它 id——用户自建的实例
                    // 可能叫 `dev-work`、`my-dev-env` 之类，那是他们自己起的
                    // 名字，报表没有资格改写。
                    let friendly_id = match id.as_str() {
                        "default" => "正式版".to_string(),
                        "default-dev" => "开发版".to_string(),
                        other => other.to_string(),
                    };
                    // 缩写只留实例名，限定词「（内核族）」收进悬停：它在同一张
                    // 瓦片里对每一条都一样，写在行内是三份重复。
                    instance_rows.push((
                        id.clone(),
                        friendly_id.clone(),
                        format!("{friendly_id}（{family_name}）"),
                        instance.path(),
                    ));
                }
            }
        } else {
            unreadable.push(family_dir.display().to_string());
        }
    }
    // 分类名在瓦片里只有 186px 宽，「实例数据（会话与附件）」会被截成
    // 「实例数据（会话与附…」，而「装的是用户会话」这句恰恰是最不能省的
    // ——它告诉用户这 352M 不是可回收的缓存。缩写上行，全文进悬停。
    groups.push(build_group(
        "instances",
        "实例数据",
        "实例数据（会话与附件）",
        instance_rows,
    ));

    // ③ 插件与技能中央库。两者都很小（本机 48K + 508K），但它们是「装
    //    了什么」的一部分，漏掉会让报表与插件面板对不上。
    let mut store_rows = Vec::new();
    for (id, label, path) in [
        ("plugins", "插件中央库", home.join("plugins")),
        ("skills", "技能中央库", home.join("skills")),
        ("backups", "备份", home.join("backups")),
    ] {
        if path.is_dir() {
            store_rows.push((id.to_string(), label.to_string(), String::new(), path));
        }
    }
    groups.push(build_group("stores", "插件 / 技能 / 备份", "", store_rows));

    // ④ 壳日志。两个壳各一份，分开列——它们本就是分开写的
    //（`registry_split` 那一套），合在一起会让人以为日志是共享的。
    //
    // 目录名 `dev` / `release` 在这里是**壳模式的技术标识**，报表是给用户
    // 看的，说 `壳日志（dev）` 等于把内部枚举值直接倒给人，而它对「我想
    // 知道哪份日志占地方」这个问题毫无帮助。换成「开发版 / 正式版」——
    // 同一对壳的另一种叫法，用户不需要知道 debug / release 这两个词与
    // 端口 3091 / 3090 的对应关系也能分清是哪一份。
    let mut log_rows = Vec::new();
    let shell_root = home.join("shell");
    if let Ok(modes) = std::fs::read_dir(&shell_root) {
        for mode in modes.flatten() {
            let logs = mode.path().join("logs");
            if !logs.is_dir() {
                continue;
            }
            let name = mode.file_name().to_string_lossy().to_string();
            let friendly = match name.as_str() {
                "dev" => "开发版",
                "release" => "正式版",
                other => other,
            };
            // 「壳日志」四个字在分类标题上已经有了，行内再写一遍是废话；
            // 缩写只留「正式版 / 开发版」，全文进悬停。
            log_rows.push((
                format!("logs-{name}"),
                friendly.to_string(),
                format!("壳日志（{friendly}）"),
                logs,
            ));
        }
    } else {
        unreadable.push(shell_root.display().to_string());
    }
    groups.push(build_group("logs", "壳日志", "", log_rows));

    let total: u64 = groups.iter().map(|group| group.bytes).sum();
    for group in &mut groups {
        group.share_percent = percent(group.bytes, total);
    }
    // 分类也按大小降序，和条目保持同一个读法。
    groups.sort_by(|a, b| b.bytes.cmp(&a.bytes).then_with(|| a.id.cmp(&b.id)));

    DiskUsage {
        total,
        // 秒级足够：这张表不是秒级变化的量，毫秒只是让「这次是新算的」
        // 与「这是十分钟前的缓存」能被区分开。
        measured_at: now_millis(),
        unreadable,
        groups,
    }
}

/// 缓存文件：`<data_dir>/disk-usage-cache.json`。
///
/// **放 data_dir 而不是 xlink_home 根上**：它按壳模式分家（`desktop` /
/// `desktop-dev`），dev 与 release 看到的数字不会互相覆盖。放根上就得再
/// 自己拼一层模式后缀，收益为零。
fn cache_file(data_dir: &Path) -> PathBuf {
    data_dir.join("disk-usage-cache.json")
}

/// 缓存多久算新鲜：**一天**（用户 2026-10-03 拍板）。
///
/// 一天是这类量的自然尺度——磁盘占用是被「装了一个内核 / 攒了一批会话」
/// 推动的，而那两件事都不按分钟发生。更短会让每次进面板都重扫 2.5 万个
/// 文件（290ms 看着不多，但它在 blocking worker 上、且这台机器的盘可能
/// 更慢）；更长则会让刚装完内核的用户看到一天前的数字，反而失真。
const FRESH_MS: u64 = 24 * 60 * 60 * 1000;

/// 报表是否还需要重新扫。`measured_at` 是毫秒时间戳。
pub fn is_stale(report: &DiskUsage, now_ms: u64) -> bool {
    // `measured_at == 0` = 测不出时刻（系统时钟落在 1970 之前）。判成陈旧
    // 去重扫一次就好：那是无法比较的输入，重扫完就会用正常值覆盖它。
    if report.measured_at == 0 {
        return true;
    }
    // `now < measured_at` = 系统时钟被往回调了。此时 `now - measured_at`
    // 在无符号减法下会绕成巨大值，把一次新鲜的缓存判成「永远过期」，
    // 于是每次都重扫。宁可当成新鲜，等时钟追上再说。
    now_ms.saturating_sub(report.measured_at) > FRESH_MS
}

fn read_cache(data_dir: &Path) -> Option<DiskUsage> {
    let text = std::fs::read_to_string(cache_file(data_dir)).ok()?;
    serde_json::from_str(&text).ok()
}

fn write_cache(data_dir: &Path, report: &DiskUsage) {
    let Ok(text) = serde_json::to_string(report) else {
        return;
    };
    // 缓存丢了只是下次多扫一次，不该因此打扰用户：失败只落 stderr。
    if let Err(error) = crate::shell::process::atomic_write(&cache_file(data_dir), text.as_bytes())
    {
        eprintln!("dsh-xlink: 写入磁盘占用缓存失败：{error}");
    }
}

/// 读一次磁盘占用：**先给缓存，后台重扫**（用户 2026-10-03 拍板），
/// **除非调用方显式 `force`**（用户 2026-10-07 报刷新按钮不生效）。
///
/// 两段式返回而不是「等扫完再返回」：用户点开面板立刻看到数字（哪怕是
/// 一天前的），不等那 290ms；扫描在后台线程完成后通过事件回填，数字自己
/// 更新。缓存新鲜时**不重扫**——那正是「一天一次」的意义。
///
/// `force` 是给「刷新」按钮用的：那是用户明说「现在就要新数字」，此时拿缓存
/// 回他等于没听见。形状照 `usage::get_model_usage(force: Option<bool>)`——
/// 同一份面板里的同类查询，共用一个约定比各发明一个强。
#[tauri::command]
pub async fn disk_usage(
    app: tauri::AppHandle,
    force: Option<bool>,
) -> Result<DiskUsageReply, String> {
    let data_dir = app
        .try_state::<crate::AppState>()
        .map(|state| state.data_dir.clone());
    let Some(data_dir) = data_dir else {
        return Err("应用状态尚未就绪".to_string());
    };
    let home = crate::shell::paths::xlink_home();

    let force = force.unwrap_or(false);
    match plan(read_cache(&data_dir), now_millis(), force) {
        // ① 有缓存可给。只有后台线程**真的起来了**才报 `background_refresh`：
        // 起不来却照报 true，前端那个转圈就永远停不下来，比没有转圈更糟。
        Plan::Answer(mut reply) => {
            if reply.background_refresh {
                reply.background_refresh = spawn_refresh(app, data_dir.clone(), home.clone());
            }
            Ok(reply)
        }
        // ② 强制刷新，或压根没有缓存：老实扫一次再回。重活走
        // `commands::blocking`（模块头那条约定），返回的一定是刚测的数。
        Plan::Scan => {
            let report = crate::commands::blocking(move || {
                let report = measure(&data_dir, &home);
                write_cache(&data_dir, &report);
                Ok::<_, String>(report)
            })
            .await?;
            Ok(DiskUsageReply {
                report,
                background_refresh: false,
            })
        }
    }
}

/// 后台重扫，完成后把新报表广播给面板。返回**线程是否真的起来了**。
///
/// 返回值不是装饰：它是 `disk-usage-refreshed` 事件会不会来的唯一可信预告。
/// 报 true 而线程没起，前端那个转圈就再也没人关。
///
/// 观测（2026-10-09 perf 补全）：扫描在后台线程跑、结果只经事件回填，此前
/// 耗时与失败只有 `eprintln!`——GUI 应用在 Windows 上它没有任何去处。成败
/// 各落一行 shell_events，让「数字多久刷新 / 为什么一直不变」可查。
fn spawn_refresh(app: tauri::AppHandle, data_dir: PathBuf, home: PathBuf) -> bool {
    let spawned = std::thread::Builder::new()
        .name("dsh-disk-usage".to_string())
        .spawn(move || {
            let started = std::time::Instant::now();
            let report = measure(&data_dir, &home);
            write_cache(&data_dir, &report);
            crate::shell::shell_events::record(
                "disk-usage",
                &format!(
                    "磁盘占用后台扫描完成，耗时 {}ms",
                    started.elapsed().as_millis()
                ),
            );
            if let Err(error) = app.emit("disk-usage-refreshed", &report) {
                crate::shell::shell_events::record(
                    "disk-usage",
                    &format!(
                        "广播磁盘占用刷新失败：{error}；面板数字本次不会更新，\
                         重新打开「内核版本」页可重试"
                    ),
                );
            }
        });
    match spawned {
        Ok(_) => true,
        Err(error) => {
            crate::shell::shell_events::record(
                "disk-usage",
                &format!(
                    "磁盘占用后台扫描线程未启动：{error}；面板将持续显示旧数据，\
                     重启应用可重试"
                ),
            );
            false
        }
    }
}

/// 当前毫秒时间戳。抽出来是为了让 `is_stale` 的测试能喂固定值。
fn now_millis() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    /// 造一棵可控的小树。
    ///
    /// `content` 是「文件名 → 字节数」，目录名以 `/` 结尾。
    fn tree(root: &Path, spec: &[(&str, usize)]) {
        for (name, size) in spec {
            let path = root.join(name);
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).unwrap();
            }
            fs::write(&path, vec![0u8; *size]).unwrap();
        }
    }

    /// 每个用例一个独立临时目录，`Drop` 时删掉。
    ///
    /// **自己实现而不复用 `crate::tests::scoped_xlink_home`**：那个 guard 改的是
    /// `DSH_XLINK_HOME` 环境变量，而本模块的 [`measure`] 把 `home` 作为**参数**
    /// 显式接收——测它不需要、也不该去抢那个全局 env 锁（抢了就得让本用例
    /// 独占它，否则与并行跑的其它模块互相干扰）。参数化正是为了让测试能指向
    /// 临时目录而不是用户真实的 `~/.dsh-xlink`。
    struct TempTree(PathBuf);

    impl TempTree {
        fn new(tag: &str) -> Self {
            static COUNTER: AtomicU64 = AtomicU64::new(0);
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            let path = std::env::temp_dir().join(format!(
                "dsh-xlink-diskusage-{tag}-{}-{n}",
                std::process::id()
            ));
            // 上一次运行若崩了会留下同名目录；先清干净再开始。
            let _ = fs::remove_dir_all(&path);
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempTree {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    /// 大小必须真的数出来，而且子树要算进父目录。
    #[test]
    fn dir_size_counts_nested_files() {
        let temp = TempTree::new("size");
        tree(
            temp.path(),
            &[("a.bin", 100), ("sub/b.bin", 250), ("sub/deep/c.bin", 50)],
        );
        assert_eq!(dir_size(temp.path()).unwrap(), 400);
    }

    /// 软链不跟随。跟随会把同一份字节数算两遍，报表直接失真——
    /// 内核树里 pnpm 的软链指向 store，跟了就是双倍。
    #[cfg(unix)]
    #[test]
    fn dir_size_does_not_follow_symlinks() {
        use std::os::unix::fs::symlink;
        let temp = TempTree::new("symlink");
        let target = temp.path().join("target.bin");
        fs::write(&target, vec![0u8; 1000]).unwrap();
        let link_dir = temp.path().join("link-dir");
        fs::create_dir_all(&link_dir).unwrap();
        symlink(&target, link_dir.join("link.bin")).unwrap();
        // 目录软链也不能被走进去（那会把 target 整个树再数一遍）。
        symlink(&target, link_dir.join("sub")).unwrap();

        // 1000 字节的目标 + 软链本身（0 字节）= 1000。
        assert_eq!(dir_size(temp.path()).unwrap(), 1000);
    }

    /// 空目录是 0 而不是 NaN / panic。占比为 0 时也要给 0——
    /// 序列化出 NaN 会在 JS 侧变成 `null`，`toFixed()` 直接抛。
    #[test]
    fn empty_directory_is_zero_not_nan() {
        let temp = TempTree::new("empty");
        assert_eq!(dir_size(temp.path()).unwrap(), 0);
        assert_eq!(percent(0, 0), 0.0);
        assert!(percent(1, 0).is_finite());
        assert_eq!(percent(1, 4), 25.0);
    }

    /// 条目按字节降序，占比之和不超过 100（浮点尾数允许极小偏差）。
    ///
    /// 降序不是美观问题：用户问「哪个最占地方」时是从上往下扫的。
    #[test]
    fn entries_are_sorted_and_share_adds_up() {
        let temp = TempTree::new("sort");
        let small = temp.path().join("small");
        fs::create_dir_all(&small).unwrap();
        fs::write(small.join("f"), vec![0u8; 100]).unwrap();
        let big = temp.path().join("big");
        fs::create_dir_all(&big).unwrap();
        fs::write(big.join("f"), vec![0u8; 900]).unwrap();

        let group = build_group(
            "g",
            "组",
            "",
            vec![
                ("small".into(), "小".into(), String::new(), small),
                ("big".into(), "大".into(), String::new(), big),
            ],
        );
        assert_eq!(group.bytes, 1000);
        assert_eq!(group.entries[0].id, "big", "大的必须排前面");
        assert_eq!(group.entries[0].share_percent, 90.0);
        assert_eq!(group.entries[1].share_percent, 10.0);
        let sum: f64 = group.entries.iter().map(|e| e.share_percent).sum();
        assert!(
            (sum - 100.0).abs() < 0.01,
            "占比之和应约等于 100，实得 {sum}"
        );
    }

    /// `dev` / `release` 是壳模式的**技术标识**，报表给用户看时换成
    /// 「开发版 / 正式版」；但只换这两个已知值，用户自建的实例名一律原样。
    ///
    /// 反向验：把映射改成「id 里含 `dev` 就改写」，`dev-work` 这个用户
    /// 自建的实例会被改成「开发版-work」——用户自己起的名字被壳改写，
    /// 而他无从知道那对应的是哪个目录。
    #[test]
    fn shell_modes_read_as_editions_but_custom_names_survive() {
        let home = TempTree::new("edition-labels");
        let data_dir = home.path().join("dsh").join("desktop");
        tree(
            &home.path().join("kernels/dsh/instances/default"),
            &[("s.json", 10)],
        );
        tree(
            &home.path().join("kernels/dsh/instances/default-dev"),
            &[("s.json", 10)],
        );
        tree(
            &home.path().join("kernels/dsh/instances/dev-work"),
            &[("s.json", 10)],
        );
        tree(&home.path().join("shell/release/logs"), &[("a.log", 5)]);
        tree(&home.path().join("shell/dev/logs"), &[("a.log", 5)]);

        let usage = measure(&data_dir, home.path());

        // 行内只放缩写，「壳日志」四个字已由分类标题说过一遍。
        let logs = usage.groups.iter().find(|g| g.id == "logs").unwrap();
        let labels: Vec<&str> = logs.entries.iter().map(|e| e.label.as_str()).collect();
        assert!(
            labels.contains(&"正式版") && labels.contains(&"开发版"),
            "release / dev 壳应显示为正式版 / 开发版：{labels:?}"
        );
        assert!(
            !labels.iter().any(|l| l.contains("壳日志")),
            "分类标题已经说过一遍，行内不该重复：{labels:?}"
        );
        assert!(
            !labels
                .iter()
                .any(|l| l.contains("release") || l.contains("dev）")),
            "不该把技术标识直接倒给人：{labels:?}"
        );

        let instances = usage.groups.iter().find(|g| g.id == "instances").unwrap();
        let labels: Vec<&str> = instances.entries.iter().map(|e| e.label.as_str()).collect();
        assert!(
            labels.contains(&"正式版") && labels.contains(&"开发版"),
            "默认实例应显示为版本名：{labels:?}"
        );
        assert!(
            labels.contains(&"dev-work"),
            "用户自建的实例名必须原样保留：{labels:?}"
        );
        // id 本身是缓存键与路径标识，不许被改写。
        let ids: Vec<&str> = instances.entries.iter().map(|e| e.id.as_str()).collect();
        assert!(ids.contains(&"default-dev"), "id 保持目录原名：{ids:?}");
    }

    /// 缩写只是排版手段，**信息不许跟着缩写一起丢**：凡是行内被砍掉的限定词，
    /// 都必须原样出现在悬停全文 `detail` 里。
    ///
    /// 这条钉的是「缩写 + 悬停」这个新形态的固有风险：后端把名字改短的那一刻，
    /// 很容易顺手把「内核族是哪个」「装的是会话还是内核」这些只有全名才说得清
    /// 的事实一起删掉，而界面上看不出来——数字照常显示，只是用户再也没法从
    /// 报表判断那 352M 是不是自己的会话。
    ///
    /// 反向验：把 `detail` 全填成 `label`（等于回到「只有缩写」），本用例必须
    /// 变红；把 instances 分类的 detail 清空，也必须变红。
    #[test]
    fn short_labels_keep_the_full_text_for_hover() {
        let home = TempTree::new("short-labels");
        let data_dir = home.path().join("dsh").join("desktop");
        tree(
            &home.path().join("kernels/dsh/instances/default"),
            &[("s.json", 10)],
        );
        tree(&home.path().join("shell/release/logs"), &[("a.log", 5)]);
        tree(&data_dir.join("kernels/0.2.0-rc.2"), &[("k", 10)]);

        let usage = measure(&data_dir, home.path());

        let instances = usage.groups.iter().find(|g| g.id == "instances").unwrap();
        assert_eq!(instances.label, "实例数据", "瓦片上行内只放缩写");
        assert_eq!(
            instances.detail, "实例数据（会话与附件）",
            "「装的是用户会话」这句不能因为缩写而消失"
        );
        let inst = instances
            .entries
            .iter()
            .find(|e| e.id == "default")
            .unwrap();
        assert_eq!(inst.label, "正式版");
        assert_eq!(inst.detail, "正式版（dsh）", "内核族名只在悬停里也别丢");

        let logs = usage.groups.iter().find(|g| g.id == "logs").unwrap();
        let log = logs
            .entries
            .iter()
            .find(|e| e.id == "logs-release")
            .unwrap();
        assert_eq!(log.label, "正式版");
        assert_eq!(log.detail, "壳日志（正式版）");

        // 没有冗余可砍的分类/条目：detail 留空，前端回落到 label。
        let kernels = usage.groups.iter().find(|g| g.id == "kernels").unwrap();
        assert_eq!(kernels.label, "内核版本");
        assert_eq!(kernels.detail, "");
        assert_eq!(kernels.entries[0].detail, "");
    }

    /// 造一份「measured_at 在 now 之前 1 分钟」的缓存，够新鲜。
    fn fresh_cache() -> DiskUsage {
        DiskUsage {
            total: 100,
            measured_at: 1_700_000_000_000,
            unreadable: Vec::new(),
            groups: Vec::new(),
        }
    }

    /// 回归：缓存新鲜时点「刷新」必须真扫一遍，不能回缓存。
    ///
    /// 用户 2026-10-07 报的 bug 就是这条——命令没有 `force`，新鲜缓存直接
    /// `return Ok(cached)`，点按钮与什么都不点完全等价。本机实测当时缓存只有
    /// 15.16 小时（远新于 24h），于是界面上的「合计 1.7 GB」是 15 小时前的数，
    /// 而用户点了刷新、什么都没发生、界面上也看不出差别。
    ///
    /// **反向验**：把 `plan` 里 `force` 那个提前 return 删掉，本用例必须变红。
    /// 它特意用**新鲜**缓存而不是陈旧的——陈旧缓存本来就会触发后台重扫，
    /// 拿它当夹具会让这条用例在 bug 存在时**照样通过**。
    #[test]
    fn force_scans_even_when_the_cache_is_fresh() {
        let now = fresh_cache().measured_at + 60 * 1000;
        match plan(Some(fresh_cache()), now, true) {
            Plan::Scan => {}
            Plan::Answer(reply) => panic!(
                "强制刷新时不得回缓存（background_refresh={}）——\
                 这正是用户报的「刷新按钮没有正确扫描磁盘占用」",
                reply.background_refresh
            ),
        }
    }

    /// 不强制的三条分支各自的行为，尤其是 `background_refresh` 只在
    /// 「确实起了后台线程」时为真。
    ///
    /// 那一个布尔是前端「后台正在重新扫描…」转圈的唯一依据：报 true 而线程
    /// 没起，转圈就永远停不下来。
    #[test]
    fn plan_without_force_keeps_the_two_stage_behaviour() {
        let cache = fresh_cache();

        // 新鲜：直接回缓存，**不**起后台线程。
        match plan(Some(cache.clone()), cache.measured_at + 60 * 1000, false) {
            Plan::Answer(reply) => {
                assert_eq!(reply.report.total, 100);
                assert!(
                    !reply.background_refresh,
                    "缓存新鲜时没有重扫在跑，转圈不该亮"
                );
            }
            Plan::Scan => panic!("缓存新鲜时不该同步扫描"),
        }

        // 陈旧：先回旧数字，同时后台重扫。
        match plan(Some(cache.clone()), cache.measured_at + FRESH_MS + 1, false) {
            Plan::Answer(reply) => {
                assert!(reply.background_refresh, "陈旧缓存必须起后台重扫")
            }
            Plan::Scan => panic!("陈旧缓存走后台，不该同步扫描"),
        }

        // 没有缓存：只能同步扫一次，否则界面上什么都没有。
        match plan(None, cache.measured_at, false) {
            Plan::Scan => {}
            Plan::Answer(_) => panic!("首次运行没有缓存可给，必须同步扫"),
        }
    }

    /// 新鲜度判定：一天内不算陈旧，超过就重扫。
    #[test]
    fn freshness_window_is_one_day() {
        let mut report = DiskUsage {
            total: 100,
            measured_at: 1_000_000_000_000,
            unreadable: Vec::new(),
            groups: Vec::new(),
        };
        // 刚扫完：一小时内必须是新鲜的，否则「一天一次」变成了「每次都扫」。
        assert!(!is_stale(&report, report.measured_at + 60 * 60 * 1000));
        // 23 小时 59 分：仍然新鲜（留出边界余量，别让"差一分钟"就重扫）。
        assert!(!is_stale(&report, report.measured_at + FRESH_MS - 1));
        // 超过一天：重扫。
        assert!(is_stale(&report, report.measured_at + FRESH_MS + 1));
        // 测不出时刻（0）：判成陈旧、去重扫一次，下一轮会用正常值覆盖。
        // 「宁可当新鲜」那句话是针对**时钟回拨**的，不是针对这个 0。
        report.measured_at = 0;
        assert!(is_stale(&report, 2_000_000_000_000));
    }

    /// 时钟被往回调时不能把新鲜缓存判成永远过期。
    ///
    /// 无符号减法下 `now < measured_at` 会绕成巨大值，`> FRESH_MS` 立刻成立
    /// ——于是**每次调用都重扫**，而缓存形同虚设（用户把系统时间调回去几
    /// 个小时就会撞上）。
    #[test]
    fn clock_going_backwards_does_not_invalidate_cache() {
        let report = DiskUsage {
            total: 100,
            measured_at: 1_700_000_000_000,
            unreadable: Vec::new(),
            groups: Vec::new(),
        };
        assert!(
            !is_stale(&report, 1_600_000_000_000),
            "now < measured_at 时应保持新鲜，不能每��都重扫"
        );
    }

    /// 缓存能原样读回，且落在 data_dir 内（dev / release 各存一份，互不覆盖）。
    #[test]
    fn cache_round_trips_inside_data_dir() {
        let home = TempTree::new("cache-home");
        let data_dir = home.path().join("dsh").join("desktop");
        tree(
            &data_dir.join("kernels"),
            &[("0.1.0/pkg/a.bin", 1234), ("0.2.0/pkg/b.bin", 4321)],
        );

        let report = measure(&data_dir, home.path());
        write_cache(&data_dir, &report);

        let back = read_cache(&data_dir).expect("缓存应能读回");
        assert_eq!(back.total, report.total);
        assert_eq!(back.measured_at, report.measured_at);
        assert_eq!(back.groups.len(), report.groups.len());
        assert_eq!(
            back.groups[0].entries[0].bytes,
            report.groups[0].entries[0].bytes
        );
        assert_eq!(
            cache_file(&data_dir),
            data_dir.join("disk-usage-cache.json"),
            "缓存必须落在 data_dir 里，让 dev / release 各持一份"
        );
    }

    /// 缓存文件损坏时返回 None，而不是让整个命令失败。
    ///
    /// 反向验：把文件写成半截 JSON，`read_cache` 必须给 None——下一次
    /// 调用会退回同步扫描，用户看到的是「重新算的数字」，而不是一张
    /// 永远空白的报表卡在界面上。
    #[test]
    fn corrupt_cache_falls_back_to_a_rescan() {
        let home = TempTree::new("cache-corrupt");
        let data_dir = home.path().join("dsh").join("desktop");
        fs::create_dir_all(&data_dir).unwrap();
        fs::write(cache_file(&data_dir), b"{\"total\": 1, \"gro").unwrap();
        assert!(read_cache(&data_dir).is_none(), "半截 JSON 必须读作 None");
    }

    /// 端到端：造出两个内核版本，验证分类、总量与「实例不是内核」这条区分。
    ///
    /// 反向验：把 instances 组的数据删掉，total 必须变小——这钉住「实例数据
    /// 被算进报表」。它一度差点被当成可回收缓存排除掉，而那 352M 是用户会话。
    #[test]
    fn measure_reports_kernels_instances_and_total() {
        let home = TempTree::new("measure");
        let data_dir = home.path().join("dsh").join("desktop");
        tree(
            &data_dir.join("kernels"),
            &[
                ("0.1.0/node_modules/pkg/a.bin", 1000),
                ("0.2.0/node_modules/pkg/b.bin", 3000),
            ],
        );
        tree(
            &home.path().join("kernels/dsh/instances/default"),
            &[("sessions/s1.json", 500), ("attachments/a.png", 700)],
        );
        tree(&home.path().join("plugins/dsh/one"), &[("index.js", 50)]);
        tree(
            &home.path().join("shell/release/logs"),
            &[("shell.log", 20)],
        );

        let usage = measure(&data_dir, home.path());

        let total: u64 = usage.groups.iter().map(|g| g.bytes).sum();
        assert_eq!(usage.total, total, "total 必须等于各分类之和");
        assert_eq!(total, 1000 + 3000 + 500 + 700 + 50 + 20);

        let kernels = usage
            .groups
            .iter()
            .find(|g| g.id == "kernels")
            .expect("应有内核分类");
        assert_eq!(kernels.bytes, 4000);
        assert_eq!(kernels.entries[0].id, "0.2.0", "大版本排前面");
        assert_eq!(kernels.entries.len(), 2);

        // 实例必须单独成组，且标题说清它是什么——用户不该把它误读成缓存。
        let instances = usage
            .groups
            .iter()
            .find(|g| g.id == "instances")
            .expect("应有实例分类");
        assert_eq!(instances.bytes, 1200);
        // 「装的是用户数据」这句现在在悬停全文里（瓦片上行内只放「实例数据」），
        // 但它一句都不能少——少了用户就会把这 352M 当成可回收的缓存。
        assert!(
            instances.detail.contains("会话"),
            "实例分类要说明装的是用户数据：{}",
            instances.detail
        );
        assert_eq!(instances.entries[0].id, "default");

        let logs = usage.groups.iter().find(|g| g.id == "logs").unwrap();
        assert_eq!(logs.bytes, 20);
    }

    /// 没有 `kernels/` 目录（首次运行）时不报错，报表为空而不是失败。
    #[test]
    fn measure_tolerates_missing_directories() {
        let home = TempTree::new("missing");
        let usage = measure(&home.path().join("nonexistent"), home.path());
        assert_eq!(usage.total, 0);
        assert!(
            usage.groups.iter().all(|g| g.bytes == 0),
            "目录不存在时各分类应为 0，不是崩溃"
        );
        assert!(usage.unreadable.iter().any(|p| p.contains("kernels")));
    }

    /// 目录里混着软链时，报表不能把目标数两遍。
    ///
    /// 反向验：把 [`dir_size`] 改成跟随软链（`path.is_dir()` 而非
    /// `file_type().is_dir()`），本例的 1000 会变成 2000。
    #[cfg(unix)]
    #[test]
    fn measure_does_not_double_count_symlinked_installs() {
        use std::os::unix::fs::symlink;
        let home = TempTree::new("symlinked-measure");
        let data_dir = home.path().join("dsh").join("desktop");
        let real = data_dir.join("kernels").join("0.1.0");
        tree(&real.join("node_modules"), &[("pkg/big.bin", 1000)]);
        // 一个指向同一安装树的软链版，模拟「旧布局残留 / 硬链接时期的产物」。
        symlink(&real, data_dir.join("kernels").join("0.1.0-link")).unwrap();

        let usage = measure(&data_dir, home.path());
        let kernels = usage
            .groups
            .iter()
            .find(|g| g.id == "kernels")
            .expect("应有内核分类");
        assert_eq!(
            kernels.bytes, 1000,
            "软链版本不应把真实版本的 1000 字节再数一遍"
        );
    }
}
