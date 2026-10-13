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
use crate::memory::query;
use crate::memory::store::{self, CandidateInput, ListFilter, Namespace, SaveInput};

/// 面板要的状态摘要（内核没跑也能读）。
#[tauri::command]
pub async fn memory_status() -> Result<store::MemoryStatus, String> {
    blocking(|| Ok::<_, std::convert::Infallible>(store::status(&Namespace::current()))).await
}

/// 列出记忆与待确认候选。
#[tauri::command]
pub async fn memory_list(filter: Option<ListFilter>) -> Result<store::MemoryListView, String> {
    let filter = filter.unwrap_or_default();
    blocking(move || Ok::<_, std::convert::Infallible>(store::list(&Namespace::current(), &filter)))
        .await
}

/// 新建或更新一条记忆（更新走 compare-and-swap）。
#[tauri::command]
pub async fn memory_save(input: SaveInput) -> Result<store::MemoryRecord, String> {
    blocking(move || store::save(&Namespace::current(), input)).await
}

/// 读一条记忆，并记一次「显式使用」（只有面板点开才算）。
#[tauri::command]
pub async fn memory_read(id: String) -> Result<store::MemoryRecord, String> {
    blocking(move || store::read_and_note_use(&Namespace::current(), &id)).await
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

/// 开关：本次使用已批准的记忆。
#[tauri::command]
pub async fn memory_set_enabled(use_approved: bool) -> Result<store::MemoryStatus, String> {
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
) -> Result<Option<store::MemoryRecord>, String> {
    blocking(move || store::decide_candidate(&Namespace::current(), &id, accept)).await
}

/// 提交一条候选（面板的「手动记一条」也走这里，转手由用户自己批准；
/// 模型侧将来经回环桥调用同一函数）。
#[tauri::command]
pub async fn memory_submit_candidate(
    input: CandidateInput,
) -> Result<store::MemoryCandidate, String> {
    blocking(move || store::submit_candidate(&Namespace::current(), input)).await
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
