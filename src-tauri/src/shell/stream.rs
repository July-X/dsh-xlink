//! 带超时的流式子进程输出捕获：进程还活着就把它的输出逐行交给调用方。
//!
//! 为什么不复用 [`crate::shell::process::run_with_progress`]：那条路径是给
//! 「pnpm 装包 + 跑原生模块构建」设计的，它强制两件 `git clone` 都不想要的
//! 事。一是把输出轮转落进 `logs/`，而 clone 的输出紧接着就被 `on_progress`
//! 收进诊断记录——同一段 `Receiving objects: 43%` 在磁盘上存在两份，且
//! `LOG_RETENTION_DAYS = 30` 让它一个月不散。二是 30 分钟的固定上限：内核
//! 装包确实可能真要那么久，但 clone 卡满 5 分钟就该告诉用户换来源，而不是
//! 再陪他等 25 分钟。
//!
//! 真正该共享的那几件——`quiet`（隐藏 Windows 控制台窗口）、
//! `isolate_process`（超时时能整棵进程树一起收）、逐行封顶读取、
//! `terminate_process_tree`——全部沿用 `process` 里的同一份实现，不重造。

use std::io::{BufReader, Read};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use crate::shell::process;

/// 排队等主循环消费的输出行上限。两个 drain 线程只要这个队列满就会阻塞，
/// 反过来给子进程的管道施加背压——这是要的行为：一个疯吐输出的子进程不该
/// 把内存吃光。
const QUEUE_CAPACITY: usize = 256;
/// 主循环在「暂时没有新行」时的等待步长。轮询只为尽早发现子进程已经退出，
/// 不会让 UI 收到更密的进度。
const POLL: Duration = Duration::from_millis(200);
/// 心跳节流：完全没有输出时，每隔这么久告诉用户一次「还活着、已进行多久」，
/// 否则一个安静的长任务看起来和挂死没有区别。
const HEARTBEAT: Duration = Duration::from_secs(15);
/// 子进程退出后等管道收尾的宽限。孙进程可能继承了 stdout/stderr 并一直
/// 持有它（`rx` 于是永远不会 Disconnected），此时若一直等到全局 deadline，
/// 一次**已经成功**的 clone 会被报成「运行超过 5 分钟」的失败。
const DRAIN_GRACE: Duration = Duration::from_secs(2);
/// 累积的输出字节上限。`read_capped_line` 只封顶**单行**（64 KiB），总量在这
/// 里封。git clone --progress 的每行只有几十字节，5 分钟内远达不到这个数——
/// 它防的不是今天会发生的 clone，而是一个把 stdout 当日志转储的子进程。
/// 超出后仍然照常 drain 管道（不 drain 会让子进程卡在写上），只是不再累积。
const MAX_TOTAL_BYTES: usize = 2 * 1024 * 1024;

/// 一次流式捕获的结果。
pub struct Captured {
    /// 子进程是否以 0 退出。
    pub success: bool,
    /// stdout 与 stderr 按到达顺序合并的全文。git 把诊断信息写在 stderr、
    /// `--progress` 的进度也走 stderr，但合并后才与真实到达顺序一致，
    /// 排查时那条「最后一个错误」不会被两个流各自的顺序骗到。
    pub output: String,
}

/// 运行 `cmd` 并把它的输出**在进程退出前**逐行喂给 `on_progress`，
/// 超时后杀掉整棵进程树并返回 `ErrorKind::TimedOut`。
///
/// 命令由调用方完整装配：程序、参数、代理环境（git 只认 gitconfig 与
/// `http_proxy`，看不见操作系统的代理设置，见 [`crate::shell::env`]）都在
/// 那里决定，本函数只补上管道、进程隔离与隐藏控制台窗口这三件在这里做
/// 才不会破坏调用方已装好的东西。
pub fn capture(
    cmd: &mut Command,
    timeout: Duration,
    mut on_progress: impl FnMut(&str),
) -> std::io::Result<Captured> {
    cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
    process::isolate_process(cmd);
    let mut child = process::quiet(cmd).spawn()?;
    let stdout = child.stdout.take().expect("child stdout was piped");
    let stderr = child.stderr.take().expect("child stderr was piped");

    let (tx, rx) = mpsc::sync_channel::<String>(QUEUE_CAPACITY);
    let tx_out = tx.clone();
    let drain_stdout = std::thread::spawn(move || pump(stdout, tx_out));
    let drain_stderr = std::thread::spawn(move || pump(stderr, tx));

    let started = Instant::now();
    let deadline = started + timeout;
    let mut last_heartbeat = started;
    let mut lines: Vec<String> = Vec::new();
    let mut collected = 0usize;
    let mut child_exited = false;
    let mut drain_deadline: Option<Instant> = None;
    let mut timed_out = false;
    let mut pipe_truncated = false;
    loop {
        if !child_exited {
            match child.try_wait() {
                Ok(Some(_)) => {
                    child_exited = true;
                    drain_deadline = Some(Instant::now() + DRAIN_GRACE);
                }
                Ok(None) => {}
                Err(error) => {
                    drop(rx);
                    process::terminate_process_tree(&mut child);
                    return Err(error);
                }
            }
        }
        if let Some(grace) = drain_deadline {
            if Instant::now() >= grace {
                pipe_truncated = true;
                break;
            }
        }
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            timed_out = true;
            break;
        }
        // 「子进程已退出、只等管道关闭」这一段睡到宽限点，不做无意义的心跳。
        let wait = match drain_deadline {
            Some(grace) => grace.saturating_duration_since(Instant::now()),
            None => POLL,
        };
        match rx.recv_timeout(wait.min(remaining)) {
            Ok(line) => {
                on_progress(line.trim_end());
                if collected < MAX_TOTAL_BYTES {
                    collected += line.len();
                    lines.push(line);
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if last_heartbeat.elapsed() >= HEARTBEAT {
                    last_heartbeat = Instant::now();
                    let secs = started.elapsed().as_secs();
                    on_progress(&format!("… 仍在下载（已进行 {secs} 秒）"));
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    if timed_out {
        drop(rx);
        process::terminate_process_tree(&mut child);
        // 杀完之后仍然要 reap：Rust 的 `Child` 在 drop 时**不会**自动 wait，
        // 不收这一下，超时的 git 会在进程表里留一条僵尸直到壳退出。
        let _ = child.wait();
        // 不要 join drain 线程：组外某个进程可能仍持有继承的管道，drain 线程
        // 会卡在 `read` 上等一个永远不会到来的字节。drop 接收端已让它在下一个
        // `send` 处自行退出——命令必须遵守它的 deadline，而不是为那个
        // 外部进程无限等待。
        drop(drain_stdout);
        drop(drain_stderr);
        return Err(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            format!("子进程运行超过 {} 秒", timeout.as_secs()),
        ));
    }

    if pipe_truncated {
        on_progress("子进程已退出；仍有其它进程持有它的输出管道，剩余输出未收录（进程本身已结束）");
    } else {
        // 两个流都已 Disconnected，drain 线程必然已走到末尾。
        let _ = drain_stdout.join();
        let _ = drain_stderr.join();
    }

    let success = child.wait()?.success();
    Ok(Captured {
        success,
        output: lines.join("\n"),
    })
}

/// 把一个流按行读干净并送给主循环。`read_capped_line` 把超长行截断到
/// `MAX_OUTPUT_LINE_BYTES`，所以一个不带换行的巨型输出不会把内存吃光。
fn pump<R: Read>(reader: R, tx: mpsc::SyncSender<String>) {
    let mut reader = BufReader::new(reader);
    let mut buffer = Vec::new();
    while let Ok(Some(line)) = process::read_capped_line(&mut reader, &mut buffer) {
        if tx.send(line).is_err() {
            break;
        }
    }
}
