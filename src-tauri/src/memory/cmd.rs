//! 记忆的 Tauri 命令层。
//!
//! 每个命令只做三件事：解析本壳默认实例的命名空间、把动作交给
//! [`crate::memory::store`]、把错误转成用户能读的字符串。**不碰 `commands.rs`**：
//! 那份文件受反棘轮约束（只许下调），记忆这一族命令按 `snapshot_cmd.rs` 的先例
//! 自成模块。
//!
//! 全部命令都走 `blocking`：store 要读写实例目录（可能在机械盘上），而 Tauri
//! 命令的 async 上下文不允许阻塞。

use crate::commands::blocking;
use crate::memory::export;
use crate::memory::jobs;
use crate::memory::query;
use crate::memory::store::{self, ListFilter, Namespace, SaveInput};
use crate::memory::workspaces;
use crate::shell::paths;

/// 面板要的状态摘要（内核没跑也能读）。
#[tauri::command]
pub async fn memory_status() -> Result<store::MemoryStatus, String> {
    blocking(|| Ok::<_, std::convert::Infallible>(store::status(&Namespace::current()))).await
}

/// 提取任务表：摘要 + 最近几条（面板的「提取任务」卡片用）。
///
/// **为什么要有它**（独立评审第 12 条）：`/jobs/status` 与 `jobs::summary` 原先只有测试在调，
/// 面板没有任何入口——用户问「这个会话为什么一条记忆都没提取出来」时，唯一可查处是手动打开
/// `jobs.json`。卡片要答的正是那句问话，所以带上**最近几条的状态与失败原因**（`stage：reason`），
/// 而不只是一个计数。**只读，不动任务表**：看它一眼不该改变任何东西。
#[tauri::command]
pub async fn memory_jobs() -> Result<JobsView, String> {
    blocking(|| {
        let doc = jobs::load(&Namespace::current())?;
        Ok::<_, crate::shell::error::AppError>(JobsView {
            summary: jobs::summary(&doc),
            // 最近几条排在前面：用户要看的几乎总是「刚刚那次」。
            jobs: doc
                .jobs
                .iter()
                .rev()
                .take(MAX_JOBS_SHOWN)
                .cloned()
                .collect(),
        })
    })
    .await
}

/// 面板要的提取任务视图：摘要 + 最近几条（最多 [`MAX_JOBS_SHOWN`] 条）。
#[derive(serde::Serialize)]
pub struct JobsView {
    pub summary: jobs::JobsSummary,
    pub jobs: Vec<jobs::Job>,
}

/// 卡片上最多列几条。**不做分页**：这是「刚刚发生了什么」的窗口，不是任务管理器；
/// 真要翻全部就去「查看日志」（那里有 `memory-jobs` 标签的完整留痕）。
const MAX_JOBS_SHOWN: usize = 5;

/// 列出记忆与待确认候选。
#[tauri::command]
pub async fn memory_list(filter: Option<ListFilter>) -> Result<store::MemoryListView, String> {
    let filter = filter.unwrap_or_default();
    blocking(move || Ok::<_, std::convert::Infallible>(store::list(&Namespace::current(), &filter)))
        .await
}

/// 待确认候选的条数（工作台会话头的待确认徽标用）。
///
/// **只有计数**：标题与正文都不出壳，内容留给管理面板。模型提了候选之后，
/// 用户不去面板就看不见——这枚徽标回答的正是「现在有没有东西在等我」。
#[tauri::command]
pub async fn memory_pending_count() -> Result<usize, String> {
    blocking(|| Ok::<_, std::convert::Infallible>(store::pending_count(&Namespace::current())))
        .await
}

/// 新建或更新一条记忆（更新走 compare-and-swap）。
#[tauri::command]
pub async fn memory_save(input: SaveInput) -> Result<store::MemoryRecord, String> {
    blocking(move || store::save(&Namespace::current(), input)).await
}

/// 停用 / 重新启用一条记忆。
#[tauri::command]
pub async fn memory_set_state(id: String, state: String) -> Result<store::MemoryRecord, String> {
    blocking(move || store::set_state(&Namespace::current(), &id, &state)).await
}

/// 删除一条记忆（正文指纹会留下来，防 P2 的自动提取把它长回来）。
#[tauri::command]
pub async fn memory_delete(id: String) -> Result<(), String> {
    blocking(move || store::delete(&Namespace::current(), &id)).await
}

/// 清空这个实例的全部记忆与候选（导出目录不在此列，面板文案要说清）。
///
/// 返回清掉了多少条：面板的确认框与结果提示都读它——「已清空」不说数量，用户无法判断
/// 是不是清错了范围。
#[tauri::command]
pub async fn memory_clear() -> Result<store::ClearOutcome, String> {
    blocking(move || store::clear(&Namespace::current())).await
}

/// 开关：本次使用已批准的记忆。
#[tauri::command]
pub async fn memory_set_enabled(use_approved: bool) -> Result<store::MemoryStatus, String> {
    // 记下这次改动：面板据此说「运行中的工作台还没认」（E6）。**只影响这一个进程内的显示**。
    crate::memory::recent::mark_setting_changed();
    blocking(move || {
        let namespace = Namespace::current();
        store::set_use_approved(&namespace, use_approved)?;
        Ok::<_, crate::shell::error::AppError>(store::status(&namespace))
    })
    .await
}

/// 批准或拒绝一条候选；批准返回新建的记录。
#[tauri::command]
pub async fn memory_decide_candidate(
    id: String,
    accept: bool,
    scope: Option<String>,
) -> Result<Option<store::MemoryRecord>, String> {
    blocking(move || store::decide_candidate(&Namespace::current(), &id, accept, scope.as_deref()))
        .await
}

/// 最近几次「模型实际看到了哪些记忆」（新的在前；壳重启后为空）。
///
/// 与 [`memory_preview_context`] 的区别要说清：那是**模拟**（拿你填的查询算一遍），
/// 这一条是**实况**（插件报告它真的把哪几条放进了上下文）。用户要判断「记忆到底有没有
/// 生效」看的是这一条。
#[tauri::command]
pub async fn memory_recent_injections() -> Result<Vec<crate::memory::recent::InjectionNote>, String>
{
    blocking(|| Ok::<_, std::convert::Infallible>(crate::memory::recent::recent_injections())).await
}

/// 预览「现在按这个查询会注入哪几条记忆」。
///
/// 面板用它把检索结果摊开给用户看（工作台那句「本次使用了 N 条记忆」背后是
/// 同一条检索），也是 P0 验收里不启动模型就能验证注入内容的入口。
#[tauri::command]
pub async fn memory_preview_context(
    query_text: String,
    workspace_id: String,
    workspace_path: String,
) -> Result<query::RetrievedContext, String> {
    blocking(move || {
        query::retrieve(
            &Namespace::current(),
            &query_text,
            &workspace_id,
            &workspace_path,
        )
    })
    .await
}

/// 内核已知的工作区（只读 `<DSH_HOME>/storages/workspace.json`）。
///
/// 面板把「本项目」从手填路径变成一个下拉选择，并顺手拿到真实的 `workspace_id`：
/// 只靠路径字符串相等匹配，项目改名或换盘一次就静默失配。**读不出来回空列表**，
/// 面板退回手填——内核的存储格式不是我们的接口（见 `memory::workspaces`）。
#[tauri::command]
pub async fn memory_workspaces() -> Result<Vec<workspaces::WorkspaceView>, String> {
    blocking(|| {
        let namespace = Namespace::current();
        Ok::<_, std::convert::Infallible>(workspaces::list(&paths::instance_dsh_home(
            &namespace.family,
            &namespace.instance_id,
        )))
    })
    .await
}

/// 导出一次 Markdown 快照（一次性，不参与运行时；见 `memory::export`）。
#[tauri::command]
pub async fn memory_export() -> Result<export::ExportResult, String> {
    blocking(|| export::export(&Namespace::current())).await
}

/// 记忆效果报告（P0 只读事实层）：扫本实例会话日志 + 读 memory.json / jobs.json 后聚合。
///
/// **不落正文、不落标题**：账本只存 id、字节与计数；榜单标题在出报告时按 id 现查。
/// 首次全量扫描要解全部会话帧（数百 MB 级），必须走 blocking，绝不能占 Tauri 主线程。
#[tauri::command]
pub async fn memory_effect_report(
    range_days: Option<u32>,
) -> Result<crate::memory::effect::report::EffectReport, String> {
    blocking(move || crate::memory::effect::report(range_days)).await
}

/// 会话下钻：按内核会话 id 返回只读的结构时间线（注入/请求/压缩节点，无正文）。
///
/// 与效果报告同一条只读纪律：解码在 blocking 里做，不返回任何正文/标题/路径。
#[tauri::command]
pub async fn memory_effect_session(
    session_id: String,
) -> Result<crate::memory::effect::session::SessionTimeline, String> {
    blocking(move || crate::memory::effect::session::timeline(&Namespace::current(), &session_id))
        .await
}

/// 开始一次会话级随机对照实验（P2，L3）：默认关闭，只有面板命令面能开。
///
/// 参数越界**报错而不是静默 clamp**：这两个值会改变正在跑的行为。写 experiment.json
/// （0600）后返回状态；已有进行中的实验会拒绝。
#[tauri::command]
pub async fn memory_effect_experiment_start(
    days: Option<u32>,
    ratio_bp: Option<u16>,
) -> Result<crate::memory::effect::experiment::ExperimentState, String> {
    blocking(move || {
        let namespace = Namespace::current();
        let path = paths::instance_memory_effect_experiment_file(
            &namespace.family,
            &namespace.instance_id,
            &namespace.profile,
        );
        crate::memory::effect::experiment::start(
            &path,
            days,
            ratio_bp,
            crate::shell::process::epoch_millis(),
        )
    })
    .await
}

/// 停止对照实验：立即生效（control 会话最迟下一步重查分组后恢复注入）。
#[tauri::command]
pub async fn memory_effect_experiment_stop(
) -> Result<crate::memory::effect::experiment::ExperimentState, String> {
    blocking(move || {
        let namespace = Namespace::current();
        let path = paths::instance_memory_effect_experiment_file(
            &namespace.family,
            &namespace.instance_id,
            &namespace.profile,
        );
        crate::memory::effect::experiment::stop(&path, crate::shell::process::epoch_millis())
    })
    .await
}

/// 在文件管理器里打开导出目录。
///
/// 与 `open_data_dir` 同一条路（服务端调 opener，绕开插件只授了 `open_path`
/// 之外几项的 IPC scope），也同样是**先建目录再打开**：`open_path` 对不存在的
/// 路径只报一个含糊的错误，用户看不到「其实只是还没导出过」。
#[tauri::command]
pub async fn memory_open_exports(app: tauri::AppHandle) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let namespace = Namespace::current();
    let dir = paths::instance_memory_dir(
        &namespace.family,
        &namespace.instance_id,
        &namespace.profile,
    )
    .join("exports");
    // 建目录也放进 `blocking`：这条路径在机械盘上时 `mkdir` 不是免费的，
    // 而 Tauri 命令的 async 上下文里阻塞是错的（根 AGENTS.md 的约定）。
    blocking(move || {
        std::fs::create_dir_all(&dir).map_err(|error| {
            format!(
                "无法访问导出目录 {}：{error}。请检查该目录的写入权限。",
                dir.display()
            )
        })?;
        app.opener()
            .open_path(dir.to_string_lossy().into_owned(), None::<&str>)
            .map_err(|error| format!("无法打开导出目录：{error}"))
    })
    .await
}
