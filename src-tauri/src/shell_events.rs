//! 壳侧事件的落盘：给「用户看得见后果、却看不见原因」的动作留证据。
//!
//! 壳是 GUI 应用，`eprintln!` 的输出在 Windows 上**没有任何去处**——Tauri
//! 发布的可执行文件走 GUI 子系统，没有控制台可接（dev 模式从终端启动时才
//! 看得到）。而恰恰是那些只有后果、没有原因的动作用了它：
//!
//! - 工作台窗口卡住 15s 后被自动重载（`harness_window::reload_stalled`）——
//!   用户只看到页面闪了一下，壳做过什么无处可查；
//! - 装内核时给 pnpm 降了优先级（`child_priority::deprioritize`）。
//!
//! 2026-09-29 实测的代价：dev 壳安装内核的 8 秒里，**release 壳的工作台
//! webview 被重载**并撞上内核的启动顺序竞态
//! （`renderSlot('root') before any 'root' registration`，记录在
//! `dsh/desktop/last-incident.json`，`at=19:09:11`）。壳这边没有任何一行
//! 日志能说明自己做过什么，用户与维护者都只能靠时间戳对猜。
//!
//! 落盘沿用既有的按日轮转约定（`<shell_logs_dir>/<kind>-<name>-<date>.log`），
//! 因此**会自动出现在「查看日志」面板**——`list_log_files` 按 `.log` 收整个
//! 壳日志目录，不需要为它单开 UI 入口。

use time::format_description::FormatItem;
use time::macros::format_description;
use time::OffsetDateTime;

use crate::process::{build_log_kind, current_date_string, shell_logs_dir, LogSpec, RotatingLog};

/// 追加一行到壳侧事件日志。`logical_name` 决定文件名，惯例与内核日志的
/// `kernel` / `install-<version>` 同形（例如 `harness-window`）。
///
/// **绝不阻断调用方**：写不进去只落 stderr。事件记录是排障材料，不是功能，
/// 它的失败不该影响用户正在做的事。
pub fn record(logical_name: &str, line: &str) {
    let logs_dir = shell_logs_dir();
    let spec = LogSpec::new(build_log_kind(), logical_name);
    // 先解析路径再开文件：出错时 stderr 至少能告诉用户日志本该在哪。
    let path = spec.path_for(&logs_dir, &current_date_string());
    match RotatingLog::new(&logs_dir, spec) {
        Ok(mut log) => {
            if let Err(error) = log.write_line(&format!("{} {}", clock(), line)) {
                eprintln!("dsh-xlink: 事件日志写入失败 {}：{error}", path.display());
            }
        }
        Err(error) => eprintln!("dsh-xlink: 无法打开事件日志 {}：{error}", path.display()),
    }
}

/// 本地时刻 `HH:MM:SS`。与 `process::local_date_string` 同源（都用本地
/// 偏移量），否则两处时间戳会各走各的时区，对齐时反而要二次换算。
fn clock() -> String {
    const CLOCK: &[FormatItem<'static>] = format_description!("[hour]:[minute]:[second]");
    OffsetDateTime::now_local()
        .ok()
        .and_then(|now| now.format(CLOCK).ok())
        .unwrap_or_else(|| String::from("--:--:--"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 必须持住 scoped_xlink_home 到用例结束：`shell_logs_dir()` 解析的是
    /// 真实用户目录，裸 set_var 拦不住并发测试，写进去就是污染用户数据。
    #[test]
    fn records_a_timestamped_line_into_the_shell_log() {
        let home = std::env::temp_dir().join(format!(
            "dsh-shell-events-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        let _xlink_home = crate::tests::scoped_xlink_home(&home);

        record("harness-window", "工作台窗口已自动重载（第 1 次，上限 2）");
        record("harness-window", "第二行应当追加而不是覆盖");

        let logs = shell_logs_dir();
        let files: Vec<_> = std::fs::read_dir(&logs)
            .expect("日志目录应已创建")
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                path.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.contains("harness-window") && n.ends_with(".log"))
            })
            .collect();
        assert_eq!(files.len(), 1, "同一逻辑名当天只应有一个文件：{files:?}");
        let text = std::fs::read_to_string(&files[0]).expect("读回事件日志");
        assert!(text.contains("已自动重载（第 1 次"), "{text}");
        assert!(
            text.contains("第二行应当追加"),
            "两次 record 必须追加到同一文件：{text}"
        );
        let stamp = clock();
        assert!(
            text.starts_with(&format!("{stamp} ")),
            "每行都要带本地时刻，便于与 last-incident.json 的 `at` 对齐：{text}"
        );

        std::fs::remove_dir_all(&home).ok();
    }
}
