//! 「一个会话目录里当前该读哪个文件」的解析与记忆。
//!
//! 独立成文件而不是留在 [`super::local`]：那边答的是「用量账目怎么算」，
//! 这里答的是「这一轮该打开哪个 `session[.vN].jsonl.zstd`」。抽出来是因为
//! 2026-10-15 的一次 perf 分析给它添了一份**进程内全局状态**——那份状态与
//! 账目结构无关、与 session 目录的形状强相关，而它的正确性判据（本目录的
//! mtime 变了没有）与账目的判据（本文件的 offset 推进到哪了）也不是同一条。
//! 两条判据混在一个文件里，读的人得同时装两件事。

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::UNIX_EPOCH;

/// 会话目录里的当前格式：`session[.vN].jsonl.zstd` 中 N 最大者
/// （无 `.vN` 视为 v0）。旧代文件是迁移遗留，绝不能与新版一起计入。
pub(super) fn newest_variant(dir: &Path) -> Option<PathBuf> {
    let mut best: Option<(u32, PathBuf)> = None;
    for entry in fs::read_dir(dir).ok()?.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        let Some(rest) = name.strip_prefix("session") else {
            continue;
        };
        let version = if rest == ".jsonl.zstd" {
            0
        } else {
            // 不认识的文件名（session.lock、未来的其它产物）只跳过自身，
            // 不影响目录里其余候选。
            let Some(digits) = rest.strip_prefix(".v") else {
                continue;
            };
            let Some((digits, tail)) = digits.split_once('.') else {
                continue;
            };
            if tail != "jsonl.zstd"
                || digits.is_empty()
                || !digits.bytes().all(|b| b.is_ascii_digit())
            {
                continue;
            }
            digits.parse::<u32>().unwrap_or(0)
        };
        if best.as_ref().is_none_or(|(v, _)| version > *v) {
            best = Some((version, entry.path()));
        }
    }
    best.map(|(_, path)| path)
}

/// 一个 session 目录的 variant 解析缓存。
struct CachedVariant {
    /// 目录 mtime（纳秒）。**闸门判据**：目录里的条目增删改一定会刷新它。
    mtime_ns: u128,
    /// 该目录里版本号最高的 variant 文件。
    variant: PathBuf,
    /// 最近一次访问到它的扫描序号，用于每轮结束时剪掉已消失的目录。
    gen: u64,
}

#[derive(Default)]
pub(super) struct VariantCache {
    /// 按 sessions 根分组，每根自带扫描序号与条目表，互不影响。
    roots: HashMap<PathBuf, RootVariants>,
}

#[derive(Default)]
pub(super) struct RootVariants {
    /// 本根最近一次扫描的序号。
    gen: u64,
    /// 字段保持私有：`local.rs` 只经 [`begin_round`] / [`resolve_variant`] /
    /// [`end_round`] 三条缝碰这张表，直接开 `pub(super)` 会让
    /// `CachedVariant`（只在本模块内有意义）的可见性被迫跟着放大。
    entries: HashMap<PathBuf, CachedVariant>,
    /// 本轮计数器。**不记下来就没法判断缓存是不是废了**：2026-10-15 之前
    /// 这个缓存连一个数字都不出，于是「命中率跌到 0」这件事只能靠再写
    /// 一遍基准才能发现——而基准得挂一台真机、跑一轮全量。
    hits: u64,
    misses: u64,
}

/// 一轮扫描的缓存统计，供 perf 采样记进日志。
#[derive(Default)]
pub(super) struct RoundStats {
    /// mtime 未变、复用上一轮答案的目录数。
    pub hits: u64,
    /// 首次见到或 mtime 变了、真的 `read_dir` 过的目录数。
    pub misses: u64,
    /// 本轮剪掉的条目数（会话目录已被删除）。
    pub pruned: u64,
}

static VARIANT_CACHE: OnceLock<Mutex<VariantCache>> = OnceLock::new();

/// 进程内唯一的缓存表。**跨调用存活**是它全部价值的来源：每轮重新枚举的
/// 代价（见 [`begin_round`]）正是它省掉的东西。
pub(super) fn variant_cache() -> &'static Mutex<VariantCache> {
    VARIANT_CACHE.get_or_init(|| Mutex::new(VariantCache::default()))
}

/// 开一轮扫描：取出这条 sessions 根自己的缓存分区并把序号 +1。
///
/// **按根分区**是必须的，不是洁癖：共用一个全局序号时，同进程内另一条根
/// （另一个实例）跑一轮就会把序号刷上去，把本根的条目整片判成陈旧并剪掉——
/// 生产上 dev / release 的实例目录同属一个进程时会互相抹缓存，并行跑的单测
/// 之间同理（守卫测试会变成偶发绿：别人的一轮扫描刚好把本测试刚建的缓存清掉，
/// 变异被掩盖）。
pub(super) fn begin_round<'a>(
    cache: &'a mut VariantCache,
    root: &Path,
) -> (&'a mut RootVariants, u64) {
    let entry = cache.roots.entry(root.to_path_buf()).or_default();
    entry.gen += 1;
    entry.hits = 0;
    entry.misses = 0;
    let gen = entry.gen;
    (entry, gen)
}

/// 解析一个 session 目录里的 variant 文件，mtime 没变就复用上一轮的答案。
///
/// **为什么这道闸是安全的**（两条都得成立，少一条就会漏账）：
///
/// 1. 在目录里**增删 / 改名**条目必然刷新目录 mtime ⇒ 新一代 variant 文件
///    出现、文件被删，都会让缓存失效并重新 `read_dir`；
/// 2. 向**已存在**的 variant 追加帧**不会**改目录 mtime —— 但那种情况我们
///    本来就不靠目录判断：调用方每轮都对文件本身取 `metadata` 比 offset，
///    追加会被照常发现。缓存里存的是「路径」，而路径没变。
pub(super) fn resolve_variant(dir: &Path, cache: &mut RootVariants, gen: u64) -> Option<PathBuf> {
    // 取不到目录 mtime 就退回原来的每轮 `read_dir`：比缓存多花一次系统
    // 调用，但绝不因为判据缺失而少看见一个会话。
    let mtime_ns = fs::metadata(dir)
        .ok()?
        .modified()
        .ok()?
        .duration_since(UNIX_EPOCH)
        .ok()?
        .as_nanos();
    if let Some(entry) = cache.entries.get_mut(dir) {
        entry.gen = gen;
        if entry.mtime_ns == mtime_ns {
            cache.hits += 1;
            return Some(entry.variant.clone());
        }
    }
    // 目录变了（或首次见到）：重新枚举。这里只在「确有 variant」时写缓存
    // —— `newest_variant` 返回 None 既可能是目录里没有会话文件，也可能是
    // `read_dir` 失败，把后者当成「确实没有」缓存住会让一个会话永久失踪。
    let variant = newest_variant(dir)?;
    cache.misses += 1;
    cache.entries.insert(
        dir.to_path_buf(),
        CachedVariant {
            mtime_ns,
            variant: variant.clone(),
            gen,
        },
    );
    Some(variant)
}

/// 本轮结束后剪掉没再被访问到的条目：对应的会话目录已经被删除，
/// 不收的话长期运行的壳会为早就消失的目录一直留着条目（键是绝对路径，
/// 无界增长）。返回本轮统计供调用方记进 perf 采样。
pub(super) fn end_round(cache: &mut RootVariants, gen: u64) -> RoundStats {
    let before = cache.entries.len() as u64;
    cache.entries.retain(|_, entry| entry.gen == gen);
    let after = cache.entries.len() as u64;
    RoundStats {
        hits: cache.hits,
        misses: cache.misses,
        pruned: before.saturating_sub(after),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("usage-variant-cache-{label}"));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).expect("create scratch dir");
        dir
    }

    /// 把目录的 mtime 推过**当前 tick**，返回是否真的推动了。
    ///
    /// 为什么不能直接 `File::open(dir).set_modified(…)`：**Windows 上对目录打开文件句柄
    /// 直接被拒**（`skills/store_busy.rs` 里那条注释记的是同一件事：「`File::open` 对目录
    /// 直接失败」）。所以走「在目录里增删一个条目」——目录 mtime 本来就因条目增删而变。
    ///
    /// 第一轮通常就够了（NTFS 上目录 mtime 是**惰性**更新的：一次新增条目会把它带到当前
    /// 时间，而不是 +tick）。不稳时 sleep 一个 tick 再来一轮，最多三轮；三轮都没推动就
    /// 如实回 `false`，由调用方断言——**不要让判据红在一句与真因无关的缓存断言上**。
    fn advance_dir_mtime(dir: &Path) -> bool {
        let stamp = || fs::metadata(dir).and_then(|m| m.modified()).ok();
        let before = stamp();
        for attempt in 0..3 {
            let touch = dir.join(format!(".mtime-touch-{attempt}"));
            if fs::write(&touch, b"x").is_err() {
                return false;
            }
            let _ = fs::remove_file(&touch);
            let now = stamp();
            if now.is_some() && now != before {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(40));
        }
        false
    }

    /// 版本号最高者胜出；`session.lock` 这类不认识的文件名只跳过自身，
    /// 不影响目录里其余候选。
    #[test]
    fn newest_variant_prefers_highest_version_and_ignores_locks() {
        let dir = scratch("parse");
        fs::write(dir.join("session.jsonl.zstd"), b"v0").unwrap();
        fs::write(dir.join("session.v2.jsonl.zstd"), b"v2").unwrap();
        fs::write(dir.join("session.v3.jsonl.zstd"), b"v3").unwrap();
        fs::write(dir.join("session.lock"), b"lock").unwrap();
        assert_eq!(
            newest_variant(&dir).unwrap().file_name().unwrap(),
            "session.v3.jsonl.zstd"
        );

        let plain = scratch("parse-plain");
        fs::write(plain.join("session.jsonl.zstd"), b"v0").unwrap();
        assert_eq!(
            newest_variant(&plain).unwrap().file_name().unwrap(),
            "session.jsonl.zstd"
        );
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_dir_all(&plain);
    }

    /// 目录没动就命中缓存；目录里多出一代 variant 就必须重新枚举。
    /// 反向验：把 `resolve_variant` 里的 mtime 比较短路成恒真，本条必红。
    #[test]
    fn mtime_gate_reenumerates_when_a_new_generation_appears() {
        let dir = scratch("generation");
        let session = dir.join("session-x");
        fs::create_dir_all(&session).unwrap();
        fs::write(session.join("session.jsonl.zstd"), b"v0").unwrap();

        let mut cache = VariantCache::default();
        let (root_cache, gen) = begin_round(&mut cache, &dir);
        let first = resolve_variant(&session, root_cache, gen).expect("首轮应找到 v1");
        assert_eq!(first.file_name().unwrap(), "session.jsonl.zstd");
        // 同目录再走一轮：没变 → 复用同一个路径。
        let (root_cache, gen) = begin_round(&mut cache, &dir);
        let again = resolve_variant(&session, root_cache, gen).expect("第二轮复用");
        assert_eq!(again, first, "目录未变时必须走缓存");

        // 目录里多出一代：mtime 变了 → 缓存失效并重新枚举到 v2。
        fs::write(session.join("session.v2.jsonl.zstd"), b"v2").unwrap();
        let advanced = advance_dir_mtime(&session);
        // **先确认前提真的成立**，再判缓存行为：文件系统的时间戳粒度不是纳秒（NTFS 的
        // 目录 mtime 按整 tick 刷新），上面那几次 `create_dir_all` / `fs::write` /
        // `resolve_variant` 常常落在同一个 tick 里——那时「新增了条目」这件事在 mtime 上
        // **看不见**，本用例就会随机红在一句与真因无关的话上（2026-10-16 带负载实测：
        // 6 次全量 `cargo test` 里红 1 次）。
        //
        // 这是**测试与文件系统的耦合**，不是 `resolve_variant` 的缺陷：它整份逻辑就是
        // 「mtime 变了才重新枚举」。把前提摆到确定状态，判据才钉的是接线而不是 tick 撞车。
        //
        // **代价（如实记）**：日常路径里「同一 tick 内新增了一代 variant」仍然看不见，
        // 要等下一轮扫描；那一轮由调用方（`local.rs` 每次读用量）驱动，不是常驻轮询。
        assert!(
            advanced,
            "推不动目录 mtime：这条判据的前提（mtime 会因条目增删而变）在这个文件系统上不成立，\
             请修 advance_dir_mtime，而不是改断言"
        );
        let (root_cache, gen) = begin_round(&mut cache, &dir);
        let third = resolve_variant(&session, root_cache, gen).expect("第三轮重新枚举");
        assert_eq!(
            third.file_name().unwrap(),
            "session.v2.jsonl.zstd",
            "目录新增条目必须让缓存失效"
        );
        let _ = fs::remove_dir_all(&dir);
    }

    /// 收尾剪枝只留本轮访问到的条目：被删掉的会话目录不该在表里赖着。
    #[test]
    fn end_round_drops_entries_the_round_never_visited() {
        let dir = scratch("prune");
        let alive = dir.join("session-a");
        let doomed = dir.join("session-b");
        fs::create_dir_all(&alive).unwrap();
        fs::create_dir_all(&doomed).unwrap();
        fs::write(alive.join("session.jsonl.zstd"), b"a").unwrap();
        fs::write(doomed.join("session.jsonl.zstd"), b"b").unwrap();

        let mut cache = VariantCache::default();
        let (root_cache, gen) = begin_round(&mut cache, &dir);
        resolve_variant(&alive, root_cache, gen).expect("a");
        resolve_variant(&doomed, root_cache, gen).expect("b");
        assert_eq!(root_cache.entries.len(), 2);

        // 下一轮只访问 a（b 的目录已被删掉）。
        let (root_cache, gen2) = begin_round(&mut cache, &dir);
        resolve_variant(&alive, root_cache, gen2).expect("a 仍在");
        end_round(root_cache, gen2);
        assert_eq!(
            root_cache.entries.len(),
            1,
            "本轮没访问到的条目必须被剪掉，否则缓存无界增长"
        );
        assert!(root_cache.entries.contains_key(&alive));
        let _ = fs::remove_dir_all(&dir);
    }

    /// 两条 sessions 根各记各的序号：A 扫一轮不该把 B 的条目判成陈旧。
    /// 这是 2026-10-15 反向验时才发现的那条——全局序号会让并行测试互相抹掉
    /// 缓存，守卫测试因此变成偶发绿。
    #[test]
    fn roots_keep_independent_generations() {
        let root_a = scratch("root-a");
        let root_b = scratch("root-b");
        let session = root_a.join("session-x");
        fs::create_dir_all(&session).unwrap();
        fs::write(session.join("session.jsonl.zstd"), b"a").unwrap();

        let mut cache = VariantCache::default();
        let (cache_a, gen_a) = begin_round(&mut cache, &root_a);
        resolve_variant(&session, cache_a, gen_a).expect("a 首轮");
        // 另一条根插进来跑一轮。
        let (_cache_b, _gen_b) = begin_round(&mut cache, &root_b);
        // a 的第二轮：条目必须还在（没有被 B 的扫描剪掉）。
        let (cache_a, gen_a2) = begin_round(&mut cache, &root_a);
        assert_eq!(cache_a.entries.len(), 1, "另一条根的扫描不得剪掉本根的条目");
        let again = resolve_variant(&session, cache_a, gen_a2).expect("a 第二轮");
        assert!(again.ends_with("session.jsonl.zstd"));
        let _ = fs::remove_dir_all(&root_a);
        let _ = fs::remove_dir_all(&root_b);
    }
}
