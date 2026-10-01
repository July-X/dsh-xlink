//! 「搬错实例」的遗留数据回收：`~/.dsh` 的会话被并进了**别的**实例的 home。
//!
//! 背景是一次真实事故（2026-09-28）。多内核改造前，内核一直以默认 `~/.dsh`
//! 运行，会话都积累在那里；改造后内核经 `DSH_HOME` 指向**实例 home**，壳在启动
//! 期把它们一次性并入（[`crate::shell::instance::migrate_legacy_dsh_home_if_needed`]）。
//! 「并进哪个实例」这道闸门（[`crate::shell::instance::legacy_migration_target`]，
//! 恒为 release）是 commit `713bb40`（2026-09-28 12:09）才补上的——而本机 dev
//! 壳在当天 11:51 就已经跑过旧逻辑，把用户的历史会话并进了 `default-dev`。
//! release 壳随后打开工作台看到的是**空列表**：数据没丢，只是不在本实例的
//! `DSH_HOME` 里，而 `~/.dsh` 已经空了，再也搬不动第二次。
//!
//! 本模块是那条缺失的回收路径，纪律与 [`crate::diagnostics::restore`] 同一套：
//! - **只读扫描先行**（[`scan_misplaced_home`]），用户看见「谁的什么、几个文件、
//!   多大」再点确认；
//! - **只复制缺失的条目**，目标已有的一律不碰；
//! - **源永不删除**——数据搬过去之后原样留在另一个实例里，回收不是搬家；
//! - 部分失败照实报（`failed` 列表），不假装全成。
//!
//! ## 回收要搬两样东西，少一样用户仍然看不到历史
//!
//! 1. `sessions/<工作区>/session-<uuid>/`（连同 `attachments/`）——会话正文；
//! 2. `<home>/storages/workspace.json` 里的**工作区条目**——会话列表的来源。
//!
//! 第 2 样是 2026-09-29 在本机实测出来的：只把 `sessions/` 拷过去，内核确实
//! 重新解析了那个会话（`storages/session_projcache/` 里出现了新条目），但工作台
//! 里**仍然不显示**——因为内核列会话读的不是目录，而是 `workspace.json` 的
//! `tables.workspaces[<wsId>].sessionIds`。本实例那份是今天新建的，里面没有那个
//! 工作区，于是会话文件在磁盘上、却不在任何列表里。因此这一项是 **JSON 合并**
//! 而不是目录复制：目标已有的工作区与字段一律以目标为准，只补缺失的 sessionIds。

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::{json, Value};

/// 允许从别的实例 home 收回的目录。刻意只有这两项：
/// - `sessions/`：内核按 `sessions/<工作区>/session-<uuid>/` 落会话，是用户唯一
///   会「凭空消失」的东西；
/// - `attachments/`：会话附件，与会话一一对应。
///
/// 其它项各有归属，不在这里动：`profiles/` 是接线按本实例 `package.json` 重建的
/// pnpm 产物（源侧那棵可能连着别的实例的依赖），`logs/` 归壳，凭据是单文件、由
/// 工作台的凭据界面单独管理。把它们一并搬过来只会制造「看起来对、其实是别处
/// 状态」的目录。（`storages/workspace.json` 是例外——它要**合并**，见上。）
const RECOVERABLE: &[&str] = &["sessions", "attachments"];

/// 工作区注册表在 home 下的相对位置。会话列表由它决定，见模块头。
const WORKSPACE_REGISTRY: &str = "storages/workspace.json";

/// 一条可回收的目录及其缺失条目（`sessions/` 或 `attachments/`）。
///
/// 三个结构体都**刻意不**套 `#[serde(rename_all = "camelCase")]`：本模块与
/// `migration.rs` 是同一条 IPC 链路，那边的 `MigrationItemPreview` /
/// `MigrationReport` 全部以 snake_case 上线（`file_count` / `total_bytes`）。
/// 同一块面板里混两种命名，读代码的人要靠记忆切换——这里跟着邻居走。
#[derive(Debug, Clone, Serialize)]
pub struct MisplacedDir {
    /// 目录名（`sessions` / `attachments`）。
    pub name: String,
    /// 数据当前在哪个实例的 home 里。
    pub holder: String,
    pub source: PathBuf,
    pub target: PathBuf,
    /// 本实例缺、源侧有的条目名（通常是 `<工作区>/` 或 `session-<uuid>/`）。
    pub entries: Vec<String>,
    pub file_count: usize,
    pub total_bytes: u64,
}

/// 一个持有者实例的工作区注册表里，本实例还缺的工作区。
#[derive(Debug, Clone, Serialize)]
pub struct MisplacedWorkspace {
    /// 数据当前在哪个实例的 home 里。
    pub holder: String,
    /// 持有者的 `workspace.json`。
    pub source: PathBuf,
    /// 本实例的 `workspace.json`（回收时**合并**进它，不整体覆盖）。
    pub target: PathBuf,
    /// 持有者有、本实例没有（或会话没跟过来）的工作区路径。
    pub paths: Vec<String>,
    /// 这些工作区里，本实例尚缺的会话数。
    pub session_count: usize,
}

/// 扫描结果：`dirs` 与 `workspaces` 都为空表示没有可回收的遗留数据。
#[derive(Debug, Clone, Serialize)]
pub struct MisplacedScan {
    /// 本实例 id。
    pub instance: String,
    /// 本实例的 `DSH_HOME`。
    pub home: PathBuf,
    pub dirs: Vec<MisplacedDir>,
    /// 会话**列表**的来源：目录复制之后还要合并它，否则文件在磁盘上却不被列出。
    pub workspaces: Vec<MisplacedWorkspace>,
}

impl MisplacedScan {
    /// 没有任何可回收内容。
    pub fn is_empty(&self) -> bool {
        self.dirs.is_empty() && self.workspaces.is_empty()
    }

    /// 全部缺失条目的文件数（UI 的「共 N 个文件」）。
    pub fn total_files(&self) -> usize {
        self.dirs.iter().map(|dir| dir.file_count).sum()
    }
}

/// 回收执行结果。**源永不被删除**，因此这里只有「复制了几条 / 跳过几条 /
/// 哪几条失败」——没有任何一条会从别的实例里消失。
#[derive(Debug, Clone, Default, Serialize)]
pub struct MisplacedRecovery {
    pub copied: Vec<String>,
    /// 目标已存在、因此没有覆盖的条目。
    pub skipped: Vec<String>,
    /// 合并进本实例工作区注册表的工作区路径。
    pub workspaces: Vec<String>,
    /// `"<条目>：<原因>"`，UI 直接展示。
    pub failed: Vec<String>,
}

/// 扫描：别的实例 home 里有没有本实例缺的历史会话。**只读**——不创建任何
/// 目录、不触碰源 / 目标。
pub fn scan_misplaced_home(family: &str, id: &str) -> MisplacedScan {
    let home = crate::shell::paths::instance_dsh_home(family, id);
    let mut dirs = Vec::new();
    let mut workspaces = Vec::new();
    for (holder, holder_home) in holder_homes(family, id) {
        for name in RECOVERABLE {
            let source = holder_home.join(name);
            let target = home.join(name);
            if let Some(dir) = scan_dir(&source, &target, name, &holder) {
                dirs.push(dir);
            }
        }
        if let Some(entry) = scan_workspace_registry(&holder_home, &home, &holder) {
            workspaces.push(entry);
        }
    }
    MisplacedScan {
        instance: id.to_string(),
        home,
        dirs,
        workspaces,
    }
}

/// 回收：把扫描出的缺失条目**复制**进本实例 home，随后**合并**工作区注册表。
///
/// 单条失败不中断整批——结果由 [`MisplacedRecovery`] 照实汇报，UI 逐条显示。
pub fn recover_misplaced_home(family: &str, id: &str) -> MisplacedRecovery {
    let mut outcome = MisplacedRecovery::default();
    for dir in scan_misplaced_home(family, id).dirs {
        let one = recover_dir(&dir);
        outcome.copied.extend(one.copied);
        outcome.skipped.extend(one.skipped);
        outcome.failed.extend(one.failed);
    }
    // 目录搬完再合并注册表：合并时要按「本实例确实有那个会话目录」来挑，
    // 顺序反了就会收编出一堆指向不存在会话的条目。
    for entry in scan_misplaced_home(family, id).workspaces {
        merge_workspace_registry(&entry, &mut outcome);
    }
    outcome
}

/// 单个目录的回收主体（[`scan_dir`] 的可测对偶：路径由扫描结果给出）。
fn recover_dir(dir: &MisplacedDir) -> MisplacedRecovery {
    let mut outcome = MisplacedRecovery::default();
    for entry in &dir.entries {
        let from = dir.source.join(entry);
        let to = dir.target.join(entry);
        // 扫描与点击之间可能有另一个壳写进了同名会话：这里再挡一次，
        // 绝不覆盖本实例已有的内容。
        if to.exists() {
            outcome.skipped.push(entry.clone());
            continue;
        }
        match copy_recursive(&from, &to) {
            Ok(()) => outcome.copied.push(entry.clone()),
            Err(error) => outcome.failed.push(format!("{entry}：{error}")),
        }
    }
    outcome
}

// --- 工作区注册表（会话列表的真正来源）-----------------------------------

/// 扫描持有者的 `workspace.json`：哪些会话本实例**还没被列出来**。
///
/// 判据只看**清单里有没有这条 id**，不看会话目录在不在——「目录在、清单里没有」
/// 正是 2026-09-29 本机那个状态（会话文件已经拷过去、内核也重新解析了，但工作台
/// 里不显示）。把它当成「不缺」，这张卡片就永远消失了。目录存在性留给合并阶段：
/// 那里要决定**哪几条 id 可以写进清单**（指向不存在会话的条目会留下一条打不开的
/// 历史，比不收编更糟）。
fn scan_workspace_registry(
    holder_home: &Path,
    self_home: &Path,
    holder: &str,
) -> Option<MisplacedWorkspace> {
    let source = holder_home.join(WORKSPACE_REGISTRY);
    let target = self_home.join(WORKSPACE_REGISTRY);
    let src = read_workspaces(&source)?;
    let dst = read_workspaces(&target).unwrap_or_default();
    // 目标侧按**路径**索引：工作区 id 是 uuid，同一个路径在两个实例里几乎一定
    // 是不同 id，按 id 比会把同一个工作区当成两个（多出一条重复条目）。
    let mut target_by_path: BTreeMap<String, String> = BTreeMap::new();
    for (id, workspace) in &dst {
        if let Some(path) = workspace.get("path").and_then(Value::as_str) {
            target_by_path.insert(path.to_string(), id.clone());
        }
    }
    let mut paths = Vec::new();
    let mut session_count = 0usize;
    for workspace in src.values() {
        let Some(path) = workspace.get("path").and_then(Value::as_str) else {
            continue;
        };
        let known: BTreeSet<String> = match target_by_path.get(path) {
            // 目标已有这个工作区：只算它没列出来的会话
            Some(target_id) => dst
                .get(target_id)
                .map(session_ids_of)
                .unwrap_or_default()
                .into_iter()
                .collect(),
            None => BTreeSet::new(),
        };
        let missing = session_ids_of(workspace)
            .iter()
            .filter(|id| !known.contains(*id))
            .count();
        if missing == 0 {
            continue;
        }
        session_count += missing;
        paths.push(path.to_string());
    }
    if paths.is_empty() {
        return None;
    }
    paths.sort();
    Some(MisplacedWorkspace {
        holder: holder.to_string(),
        source,
        target,
        paths,
        session_count,
    })
}

/// 把持有者的工作区条目**合并**进本实例的 `workspace.json`。
///
/// 四条纪律：
/// - **目标已有的工作区以目标为准**，只往它的 `sessionIds` 里补缺的会话；
/// - 只收编**本实例确实有会话目录**的条目——收编一个指向不存在会话的条目，
///   会在工作台里留下一条打不开的历史，比不收编更糟；
/// - `defaultWorkspaceId` / `archivedSessionIds` / `pinnedSessionIds` 一个都不动：
///   那是本实例用户的当前选择，与别的实例无关；
/// - 写盘走 `atomic_write`，中途失败不会留下半个 JSON。
///
/// 需要目标实例的内核未在运行（`home_recovery_cmd` 的命令层负责挡，判据按
/// **实例** pid 文件而非本壳工作台——另一个壳也可能正跑着这个实例）：内核把
/// storages 缓存在内存里，它下一次落盘会把这里的合并整个覆盖掉。
fn merge_workspace_registry(entry: &MisplacedWorkspace, outcome: &mut MisplacedRecovery) {
    let Some(src) = read_workspaces(&entry.source) else {
        return;
    };
    // 目标文档缺失时给一份空骨架；**损坏时不重建**——那会把用户自己的工作区
    // 清单整个抹掉，此时宁可什么都不做。
    let mut doc = match read_registry(&entry.target) {
        Some(value) => value,
        None => {
            if entry.target.exists() {
                outcome.failed.push(format!(
                    "{}：工作区注册表损坏，未收编（请先备份并检查该文件）",
                    entry.target.display()
                ));
                return;
            }
            json!({
                "unit": { "name": "workspace", "version": 2 },
                // 这三个字段内核的 unit 文档里都有（见本模块测试的 write_registry）。
                // 缺了它们，合并出来的清单在一个还不存在的实例上就是残的：没有
                // 默认工作区、没有归档与置顶。**不动**它们是另一条纪律——已经
                // 存在的目标实例里那是用户的当前选择，不归本次回收管。
                "global": {
                    "initialized": true,
                    "workspaceIds": [],
                    "archivedSessionIds": [],
                    "pinnedSessionIds": [],
                    "defaultWorkspaceId": Value::Null,
                },
                "tables": { "workspaces": {} },
            })
        }
    };
    // `<home>/storages/workspace.json` → `<home>`
    let target_home = entry
        .target
        .parent()
        .and_then(|storages| storages.parent())
        .unwrap_or_else(|| Path::new(""));
    let mut target_by_path: BTreeMap<String, String> = BTreeMap::new();
    for (id, workspace) in workspaces_of(doc.clone()).unwrap_or_default() {
        if let Some(path) = workspace.get("path").and_then(Value::as_str) {
            target_by_path.insert(path.to_string(), id);
        }
    }
    for (holder_id, workspace) in &src {
        let Some(path) = workspace.get("path").and_then(Value::as_str) else {
            continue;
        };
        if !entry.paths.iter().any(|p| p == path) {
            continue;
        }
        let all = session_ids_of(workspace);
        let available: Vec<String> = all
            .iter()
            .filter(|id| session_dir_exists(target_home, id))
            .cloned()
            .collect();
        if available.is_empty() {
            outcome.failed.push(format!(
                "{path}：{} 个会话目录尚未复制过来，工作区条目未收编",
                all.len()
            ));
            continue;
        }
        if available.len() < all.len() {
            outcome.failed.push(format!(
                "{path}：只有 {} / {} 个会话目录已复制过来，其余未收编",
                available.len(),
                all.len()
            ));
        }
        match target_by_path.get(path).cloned() {
            Some(key) => {
                // 同一路径的工作区已经在本实例：只补缺的会话，不动其它字段。
                let existing = doc["tables"]["workspaces"][&key]["sessionIds"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default();
                let known: BTreeSet<String> = existing
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect();
                let mut merged = existing;
                merged.extend(
                    available
                        .iter()
                        .filter(|id| !known.contains(*id))
                        .map(|id| Value::String(id.clone()))
                        .collect::<Vec<_>>(),
                );
                doc["tables"]["workspaces"][&key]["sessionIds"] = Value::Array(merged);
            }
            None => {
                // 全新工作区：收编，但**只写确实复制过来的会话**。整条
                // `workspace.clone()` 会把没复制成功的 id 也写进去——工作台
                // 里那就是一条点不开的历史，比不收编更糟（与本函数开头
                // 第二条纪律同源：`Some` 分支补的也一直是 `available`）。
                let mut adopted = workspace.clone();
                adopted["sessionIds"] = Value::Array(
                    available
                        .iter()
                        .map(|id| Value::String(id.clone()))
                        .collect(),
                );
                doc["tables"]["workspaces"][holder_id] = adopted;
                let mut list = doc["global"]["workspaceIds"]
                    .as_array()
                    .cloned()
                    .unwrap_or_default();
                if !list.iter().any(|v| v.as_str() == Some(holder_id.as_str())) {
                    list.push(Value::String(holder_id.clone()));
                }
                doc["global"]["workspaceIds"] = Value::Array(list);
            }
        }
        outcome.workspaces.push(path.to_string());
    }
    let text = match serde_json::to_string_pretty(&doc) {
        Ok(text) => text,
        Err(error) => {
            outcome
                .failed
                .push(format!("工作区注册表序列化失败：{error}"));
            return;
        }
    };
    if let Some(parent) = entry.target.parent() {
        if let Err(error) = fs::create_dir_all(parent) {
            outcome
                .failed
                .push(format!("无法创建 {}：{error}", parent.display()));
            return;
        }
    }
    if let Err(error) =
        crate::shell::process::atomic_write(&entry.target, format!("{text}\n").as_bytes())
    {
        outcome
            .failed
            .push(format!("写入 {} 失败：{error}", entry.target.display()));
    }
}

/// 读一份 `workspace.json`；文件不存在返回 `None`，损坏时也返回 `None` 并
/// 留 stderr——损坏文档与「没有文档」在这里必须区分开：前者绝不能被空骨架
/// 覆盖（见 [`merge_workspace_registry`]）。
fn read_registry(path: &Path) -> Option<Value> {
    let text = fs::read_to_string(path).ok()?;
    match serde_json::from_str::<Value>(&text) {
        Ok(value) => Some(value),
        Err(error) => {
            eprintln!(
                "home-recovery: 工作区注册表解析失败（{}：{error}），本次不动它",
                path.display()
            );
            None
        }
    }
}

/// `tables.workspaces` 对象，缺字段时给空表。
fn read_workspaces(path: &Path) -> Option<BTreeMap<String, Value>> {
    workspaces_of(read_registry(path)?)
}

fn workspaces_of(value: Value) -> Option<BTreeMap<String, Value>> {
    value
        .get("tables")?
        .get("workspaces")?
        .as_object()
        .map(|map| {
            map.iter()
                .map(|(id, workspace)| (id.clone(), workspace.clone()))
                .collect()
        })
}

/// 一个工作区条目声明的会话 id。
fn session_ids_of(workspace: &Value) -> Vec<String> {
    workspace
        .get("sessionIds")
        .and_then(Value::as_array)
        .map(|ids| {
            ids.iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// `<home>/sessions/**/session-<uuid>/` 在不在。
///
/// 刻意**不**自己实现「工作区路径 → 目录名」的编码规则：那是内核的约定
/// （非 ASCII 会变成 `~XXXX~`），跟着它改就会多出第二份实现，日内核一改
/// 编码就静默失配。直接在 `sessions/` 下一层找 `session-<uuid>` 目录，
/// 结果与编码规则无关。
fn session_dir_exists(home: &Path, session_id: &str) -> bool {
    fs::read_dir(home.join("sessions"))
        .into_iter()
        .flatten()
        .flatten()
        .any(|workspace| workspace.path().join(session_id).is_dir())
}

/// 可能的「持有者」实例 id：另一个壳的默认实例 + 注册表里同族的其它实例。
///
/// 两个来源都要：注册表条目可能被用户删过（`delete_instance`），而另一个壳的
/// 默认实例 id 是由编译模式写死的常量，不依赖注册表是否完整——2026-09-28 那次
/// 事故正是 dev 壳按自己的默认 id 取走了 release 的历史数据。
///
/// 读注册表是唯一的外部依赖，因此它与「解析成路径」拆成两步：判定持有者
/// **是谁**是纯逻辑（可测），把不存在的 home 滤掉是文件系统的事。
fn holder_ids(family: &str, id: &str) -> BTreeSet<String> {
    let mut ids: BTreeSet<String> = BTreeSet::new();
    for mode in [
        crate::shell::paths::ShellMode::Release,
        crate::shell::paths::ShellMode::Dev,
    ] {
        let candidate = crate::shell::instance::default_instance_id_for(mode);
        if candidate != id {
            ids.insert(candidate.to_string());
        }
    }
    // 两个壳各读**各自那份**注册表（`registry_split` 之后不再共享一个文件），
    // 于是「对方用户自建的实例里也可能有历史会话」同样能被算成持有者。
    for mode in [
        crate::shell::paths::ShellMode::Release,
        crate::shell::paths::ShellMode::Dev,
    ] {
        if let Ok(registry) = crate::shell::instance::load_registry_for(mode) {
            for record in registry.instances {
                if record.kernel_family == family && record.id != id {
                    ids.insert(record.id);
                }
            }
        }
    }
    ids
}

/// [`holder_ids`] 解析成路径并滤掉不存在的 home。
fn holder_homes(family: &str, id: &str) -> Vec<(String, PathBuf)> {
    holder_ids(family, id)
        .into_iter()
        .map(|holder| {
            let home = crate::shell::paths::instance_dsh_home(family, &holder);
            (holder, home)
        })
        .filter(|(_, home)| home.is_dir())
        .collect()
}

/// 纯函数形态的目录扫描：源目录里哪些顶层条目在目标里没有。**只读**。
fn scan_dir(source: &Path, target: &Path, name: &str, holder: &str) -> Option<MisplacedDir> {
    let entries = fs::read_dir(source).ok()?;
    let mut names = Vec::new();
    let mut file_count = 0usize;
    let mut total_bytes = 0u64;
    for entry in entries.flatten() {
        let child = entry.path();
        if target.join(entry.file_name()).exists() {
            continue;
        }
        let (count, bytes) = walk(&child);
        file_count += count;
        total_bytes += bytes;
        names.push(entry.file_name().to_string_lossy().into_owned());
    }
    if names.is_empty() {
        return None;
    }
    names.sort();
    Some(MisplacedDir {
        name: name.to_string(),
        holder: holder.to_string(),
        source: source.to_path_buf(),
        target: target.to_path_buf(),
        entries: names,
        file_count,
        total_bytes,
    })
}

/// 递归数一个条目的文件数与字节数。符号链接按文件计（`symlink_metadata`
/// 不追链），与 `migration::scan_one` 的口径一致。
fn walk(path: &Path) -> (usize, u64) {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return (0, 0);
    };
    if !metadata.is_dir() {
        return (1, metadata.len());
    }
    let mut count = 0usize;
    let mut bytes = 0u64;
    for entry in fs::read_dir(path).into_iter().flatten().flatten() {
        let (c, b) = walk(&entry.path());
        count += c;
        bytes += b;
    }
    (count, bytes)
}

/// 递归复制：目录逐层建，文件复制。目标已存在由调用方挡掉，这里不再判断。
fn copy_recursive(from: &Path, to: &Path) -> Result<(), String> {
    if let Some(parent) = to.parent() {
        fs::create_dir_all(parent).map_err(|e| format!("无法创建 {}：{e}", parent.display()))?;
    }
    let metadata =
        fs::symlink_metadata(from).map_err(|e| format!("无法读取 {}：{e}", from.display()))?;
    if !metadata.is_dir() {
        return fs::copy(from, to)
            .map(|_| ())
            .map_err(|e| format!("无法复制 {} → {}：{e}", from.display(), to.display()));
    }
    fs::create_dir_all(to).map_err(|e| format!("无法创建 {}：{e}", to.display()))?;
    for entry in fs::read_dir(from).map_err(|e| format!("无法读取 {}：{e}", from.display()))? {
        let entry = entry.map_err(|e| format!("无法读取 {}：{e}", from.display()))?;
        copy_recursive(&entry.path(), &to.join(entry.file_name()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::scoped_xlink_home;

    fn temp_dir(tag: &str) -> PathBuf {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!(
            "dsh-home-recovery-{tag}-{}-{nanos}",
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    /// 缺失的条目要被认出来，已有的同名条目不能算进去——后者是「别的实例也在
    /// 跑内核、自己也攒了会话」时的常态，误报会让用户以为自己的数据有缺口。
    #[test]
    fn scan_reports_only_what_this_instance_lacks() {
        let base = temp_dir("scan");
        let source = base.join("holder/sessions");
        let target = base.join("mine/sessions");
        fs::create_dir_all(source.join("--ws-a--/session-1")).unwrap();
        fs::write(
            source.join("--ws-a--/session-1/session.v4.jsonl.zstd"),
            "abc",
        )
        .unwrap();
        fs::create_dir_all(source.join("--ws-b--")).unwrap();
        fs::write(source.join("--ws-b--/keep.txt"), "12345").unwrap();
        // 本实例已经有的同名工作区：不该被算成缺失
        fs::create_dir_all(target.join("--ws-b--")).unwrap();

        let dir = scan_dir(&source, &target, "sessions", "default-dev").expect("有可回收内容");
        assert_eq!(dir.entries, vec!["--ws-a--".to_string()]);
        assert_eq!(dir.file_count, 1);
        assert_eq!(dir.total_bytes, 3);
        assert_eq!(dir.holder, "default-dev");

        // 目标一样都不缺时返回 None（UI 据此不渲染卡片）
        let empty = base.join("empty/sessions");
        fs::create_dir_all(&empty).unwrap();
        assert!(scan_dir(&empty, &target, "sessions", "default-dev").is_none());

        let _ = fs::remove_dir_all(&base);
    }

    /// 回收 = 复制：源必须原样留着（数据可能还属于另一个实例），目标拿到完整
    /// 一份。已存在的条目被跳过而不是覆盖。
    #[test]
    fn recover_copies_and_never_touches_the_source() {
        let base = temp_dir("recover");
        // 用 `scan_dir` + `recover_dir` 这对可测形态跑真实目录，而不是把复制
        // 逻辑在测试里重写一遍——重写的那份永远不跟着实现走。
        let source = base.join("holder/sessions");
        let target = base.join("mine/sessions");
        // 两边都有：不该被覆盖
        fs::create_dir_all(source.join("--ws-a--/session-1")).unwrap();
        fs::write(
            source.join("--ws-a--/session-1/session.v4.jsonl.zstd"),
            "abc",
        )
        .unwrap();
        fs::create_dir_all(target.join("--ws-a--/session-1")).unwrap();
        fs::write(
            target.join("--ws-a--/session-1/session.v4.jsonl.zstd"),
            "mine",
        )
        .unwrap();
        // 只有源侧有：该被复制过来
        fs::create_dir_all(source.join("--ws-b--/session-2")).unwrap();
        fs::write(
            source.join("--ws-b--/session-2/session.v4.jsonl.zstd"),
            "zzz",
        )
        .unwrap();

        let dir = scan_dir(&source, &target, "sessions", "default-dev").expect("有可回收内容");
        assert_eq!(dir.entries, vec!["--ws-b--".to_string()]);
        let outcome = recover_dir(&dir);

        assert_eq!(outcome.copied, vec!["--ws-b--".to_string()]);
        assert!(outcome.failed.is_empty(), "{:?}", outcome.failed);
        assert_eq!(
            fs::read_to_string(target.join("--ws-a--/session-1/session.v4.jsonl.zstd")).unwrap(),
            "mine",
            "已存在的条目不得被覆盖"
        );
        assert_eq!(
            fs::read_to_string(target.join("--ws-b--/session-2/session.v4.jsonl.zstd")).unwrap(),
            "zzz",
            "缺失的条目必须原样复制过来"
        );
        assert_eq!(
            fs::read_to_string(source.join("--ws-a--/session-1/session.v4.jsonl.zstd")).unwrap(),
            "abc",
            "源必须原样保留——回收不是搬家"
        );
        // 复制完再扫一次：已经没有缺口了（UI 靠这个让卡片消失）。
        assert!(scan_dir(&source, &target, "sessions", "default-dev").is_none());

        let _ = fs::remove_dir_all(&base);
    }

    /// 持有者集合：另一个壳的默认实例恒在里面，**不依赖注册表里有没有它**，
    /// 而且自己永远不算自己的持有者。
    ///
    /// 必须持 `scoped_xlink_home`：`holder_ids` 内部会
    /// `load_registry_for(mode)`，那是要经 `DSH_XLINK_HOME` 解析出
    /// `state/instances*.json` 的——它和 `holder_homes` 一样碰真实路径，只是
    /// 读的不是 home 目录而已。没有这把 guard，单跑时读的是用户机器上真实的
    /// 注册表，并发时读的是别的测试留下的临时 home（后者会让断言变成「那个
    /// 目录碰巧在不在」的 flaky）。
    #[test]
    fn holder_ids_include_the_other_shell_default_but_never_self() {
        let home = temp_dir("holder-ids");
        let _xlink = scoped_xlink_home(&home);
        let family = crate::shell::instance::KERNEL_FAMILY_DSH;
        let release_view = holder_ids(family, "default");
        assert!(
            release_view.contains("default-dev"),
            "release 实例的持有者集合必须含 dev 默认实例：{release_view:?}"
        );
        assert!(!release_view.contains("default"), "自己不算自己的持有者");

        let dev_view = holder_ids(family, "default-dev");
        assert!(
            dev_view.contains("default"),
            "dev 实例的持有者集合必须含 release 默认实例：{dev_view:?}"
        );
        assert!(!dev_view.contains("default-dev"), "自己不算自己的持有者");
        std::fs::remove_dir_all(&home).ok();
    }

    /// 写一份 `workspace.json`（字段与内核的 unit 文档一致）。
    fn write_registry(home: &Path, workspaces: Value, default_ws: &str) {
        let path = home.join(WORKSPACE_REGISTRY);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let ids: Vec<Value> = workspaces
            .as_object()
            .unwrap()
            .keys()
            .map(|id| Value::String(id.clone()))
            .collect();
        let doc = json!({
            "unit": { "name": "workspace", "version": 2 },
            "global": {
                "initialized": true,
                "workspaceIds": ids,
                "archivedSessionIds": [],
                "pinnedSessionIds": [],
                "defaultWorkspaceId": default_ws,
            },
            "tables": { "workspaces": workspaces },
        });
        fs::write(&path, serde_json::to_string_pretty(&doc).unwrap()).unwrap();
    }

    /// 只把 `sessions/` 拷过去是不够的：会话列表由 `storages/workspace.json`
    /// 的 `tables.workspaces[<wsId>].sessionIds` 决定，目录里有、清单里没有 =
    /// 工作台里看不见。2026-09-29 本机实测就是这个状态。
    #[test]
    fn recover_merges_the_workspace_registry_so_sessions_become_visible() {
        let base = temp_dir("registry");
        let holder = base.join("holder");
        let mine = base.join("mine");
        // 持有者：工作区 A（带一个会话）——但会话目录还没复制过来
        write_registry(
            &holder,
            json!({
                "ws-a": { "path": "C:\\ws\\a", "title": "a", "sessionIds": ["session-x"] }
            }),
            "ws-a",
        );
        fs::create_dir_all(holder.join("sessions/--C-ws-a--/session-x")).unwrap();
        // 本实例：工作区 B，且 defaultWorkspaceId 指向 B
        write_registry(
            &mine,
            json!({
                "ws-b": {
                    "path": "C:\\ws\\b",
                    "title": "b",
                    "sessionIds": ["session-y"],
                    "createdAt": "2026-09-29T09:23:06.548Z",
                }
            }),
            "ws-b",
        );
        fs::create_dir_all(mine.join("sessions/--C-ws-b--/session-y")).unwrap();

        // ① 目录还没复制过来：会话目录不存在 → 收编会被拒，工作台仍看不见
        let before = scan_workspace_registry(&holder, &mine, "default-dev").expect("有缺口");
        assert_eq!(before.paths, vec!["C:\\ws\\a".to_string()]);
        assert_eq!(before.session_count, 1);
        let mut outcome = MisplacedRecovery::default();
        merge_workspace_registry(&before, &mut outcome);
        assert!(outcome.workspaces.is_empty(), "会话没到就不该收编");
        assert_eq!(outcome.failed.len(), 1, "且要照实说为什么");

        // ② 目录复制完：这一步之后会话才会出现在列表里
        copy_recursive(
            &holder.join("sessions/--C-ws-a--"),
            &mine.join("sessions/--C-ws-a--"),
        )
        .expect("copy sessions");
        let entry = scan_workspace_registry(&holder, &mine, "default-dev").expect("有缺口");
        let mut outcome = MisplacedRecovery::default();
        merge_workspace_registry(&entry, &mut outcome);
        assert_eq!(outcome.workspaces, vec!["C:\\ws\\a".to_string()]);
        assert!(outcome.failed.is_empty(), "{:?}", outcome.failed);

        let merged: Value =
            serde_json::from_str(&fs::read_to_string(mine.join(WORKSPACE_REGISTRY)).unwrap())
                .unwrap();
        let ids = merged["tables"]["workspaces"]["ws-a"]["sessionIds"]
            .as_array()
            .unwrap();
        assert_eq!(ids[0], "session-x", "收编的会话必须被列出来");
        // 本实例原有的工作区、默认值一个都不许动
        assert_eq!(
            merged["tables"]["workspaces"]["ws-b"]["sessionIds"][0],
            "session-y"
        );
        assert_eq!(merged["global"]["defaultWorkspaceId"], "ws-b");
        let list = merged["global"]["workspaceIds"].as_array().unwrap();
        assert_eq!(list.len(), 2, "两个工作区都该在清单里：{list:?}");

        // 幂等：再跑一次扫描不该再报缺口（否则 UI 的卡片永远消不掉）
        assert!(scan_workspace_registry(&holder, &mine, "default-dev").is_none());

        let _ = fs::remove_dir_all(&base);
    }

    /// 只复制过来一部分会话目录时，收编**只能写已到位的那些**。整条
    /// `workspace.clone()` 会把没复制成功的 id 也写进清单，工作台里那就是
    /// 一条点不开的历史——比不收编更糟。
    #[test]
    fn merge_adopts_only_the_sessions_that_actually_landed() {
        let base = temp_dir("registry-partial");
        let holder = base.join("holder");
        let mine = base.join("mine");
        write_registry(
            &holder,
            json!({
                "ws-a": {
                    "path": "C:\\ws\\a",
                    "title": "a",
                    "sessionIds": ["session-ok", "session-missing"]
                }
            }),
            "ws-a",
        );
        // 两个会话都在持有者侧，但只有 session-ok 的目录真的复制到了本实例
        // （磁盘满 / 权限 / 竞争都会造成这种半截状态）。
        fs::create_dir_all(holder.join("sessions/--C-ws-a--/session-ok")).unwrap();
        fs::create_dir_all(holder.join("sessions/--C-ws-a--/session-missing")).unwrap();
        fs::create_dir_all(mine.join("sessions/--C-ws-a--/session-ok")).unwrap();

        let entry = scan_workspace_registry(&holder, &mine, "default-dev").expect("有缺口");
        let mut outcome = MisplacedRecovery::default();
        merge_workspace_registry(&entry, &mut outcome);
        assert!(
            !outcome.failed.is_empty(),
            "部分缺失必须照实汇报，不能当成全成功：{:?}",
            outcome.failed
        );

        let merged: Value =
            serde_json::from_str(&fs::read_to_string(mine.join(WORKSPACE_REGISTRY)).unwrap())
                .unwrap();
        let ids = merged["tables"]["workspaces"]["ws-a"]["sessionIds"]
            .as_array()
            .expect("工作区条目应被收编");
        assert_eq!(
            ids,
            &vec![Value::String("session-ok".to_string())],
            "清单里只允许出现确实复制过来的会话"
        );
        let list = merged["global"]["workspaceIds"].as_array().unwrap();
        assert_eq!(list.len(), 1, "工作区本身要收编进清单：{list:?}");

        let _ = fs::remove_dir_all(&base);
    }

    /// 同一个路径的会话**在目标已有**时，合并只能补缺的那几条，不能动目标
    /// 已有的 id 与顺序（目标侧可能刚被用户整理过）。
    #[test]
    fn merge_only_appends_missing_session_ids_to_an_existing_workspace() {
        let base = temp_dir("registry-append");
        let holder = base.join("holder");
        let mine = base.join("mine");
        write_registry(
            &holder,
            json!({
                "holder-ws": { "path": "C:\\ws\\a", "title": "a", "sessionIds": ["session-old", "session-new"] }
            }),
            "holder-ws",
        );
        // 目标侧同一路径、id 不同（这正是两个实例各记各的 uuid 的实际情况）
        write_registry(
            &mine,
            json!({
                "mine-ws": { "path": "C:\\ws\\a", "title": "我自己起的名", "sessionIds": ["session-old", "session-mine"] }
            }),
            "mine-ws",
        );
        for (session, ws) in [
            ("session-old", "mine"),
            ("session-new", "holder"),
            ("session-mine", "mine"),
        ] {
            fs::create_dir_all(mine.join(format!("sessions/--C-ws-a--/{session}"))).unwrap();
            if ws == "holder" {
                fs::create_dir_all(holder.join(format!("sessions/--C-ws-a--/{session}"))).unwrap();
            }
        }

        let entry = scan_workspace_registry(&holder, &mine, "default-dev").expect("有缺口");
        assert_eq!(entry.session_count, 1, "只缺 session-new 一条");
        let mut outcome = MisplacedRecovery::default();
        merge_workspace_registry(&entry, &mut outcome);

        let merged: Value =
            serde_json::from_str(&fs::read_to_string(mine.join(WORKSPACE_REGISTRY)).unwrap())
                .unwrap();
        let ids: Vec<String> = merged["tables"]["workspaces"]["mine-ws"]["sessionIds"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        assert_eq!(
            ids,
            vec!["session-old", "session-mine", "session-new"],
            "只能在尾部补一条，本地已有的顺序不能变"
        );
        assert_eq!(
            merged["tables"]["workspaces"]["mine-ws"]["title"], "我自己起的名",
            "目标侧的工作区标题不能被持有者覆盖"
        );
        assert!(
            merged["tables"]["workspaces"].get("holder-ws").is_none(),
            "路径已存在就不该再收编一个同名工作区"
        );
        assert_eq!(
            merged["global"]["workspaceIds"].as_array().unwrap().len(),
            1
        );

        let _ = fs::remove_dir_all(&base);
    }
}
