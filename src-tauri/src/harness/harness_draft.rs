//! 工作台**没发出去的东西**在重载 / 重建窗口时的存续：一段文字 + 还没发出去的那
//! 几张图。
//!
//! ## 为什么用户丢掉的是这个
//!
//! 工作台页面有两条会把它整个换掉的路径：页面内自愈（`harness-health.js` 命中槽位
//! 装配不变量后 `window.location.reload()`）与壳侧看门狗（`reload` / `recreate`）。
//! **两条都不保留任何页面状态**，而用户此刻最可能正在做的，就是在输入框里打一段还没
//! 发出去的话、或者刚甩进来一张截图。会话本身不会丢（它在服务端），丢的只有这些。
//!
//! ## 为什么必须经过壳，不能只放 sessionStorage
//!
//! `recreate` 会**换掉整个 webview**（新渲染进程），新窗口拿不到旧窗口的
//! `sessionStorage`——自愈额度 flag `dsh-harness-slot-recovery` 就是这么丢的，
//! 于是「刷新救不回来」这件事每换一个窗口就要重新交一次学费。草稿比额度重要得多。
//!
//! ## 存的是「什么」，以及不存什么
//!
//! 一段**纯文本**、附件轨道上那**几张图**、当时的页面地址。不存会话内容（服务端有），
//! 不存任何凭据。取回是**一次性**的：读走即删，所以几天后误开工作台不会凭空冒出一
//! 段旧话。页面侧找不到可写的输入框时**不取**（`take` 是读+删，见下）。
//!
//! **图的字节归 [`crate::harness::harness_media`] 管**：图片在核心里不是编辑器的一部分，而是
//! composer 卡片上一排 `blob:` 缩略图，字节只活在那一个页面里，**页面上没有任何服务端
//! 副本**。这里只管「什么时候有、什么时候作废」这层生命周期。
//!
//! 反过来也成立：**输入框空了就是作废**。用户把话发出去之后，编辑器被程序化清空，
//! 那一刻盘上那一份就成了「已经送达却还躺在磁盘上的一句」，下一次打开工作台它会自己
//! 坐回输入框。`stash` 收到空文本**且**没有图即删除，页面另有 `clear_harness_draft`
//! 走同样的语义——见 [`stash`]。

use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::harness::harness_media::{DraftImage, ImageInput, RestoredImage};

/// 落盘的内容。字段名走 snake_case（与 `KernelStatus` 同一约定，无 `rename_all`）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Draft {
    /// 写下这段话时的页面地址（含会话 id）。恢复时要求地址一致——地址变了说明
    /// 用户已经换到别的会话/页面，把上一处的草稿塞进当前输入框是帮倒忙。
    pub href: String,
    pub text: String,
    /// 还没发出去的图片（截图 / 粘贴的图）。`default` 是为了读懂**加图片之前**写下的
    /// 草稿——那批文件里没有这个字段，而读不回来的症状是「用户的话凭空消失」。
    #[serde(default)]
    pub images: Vec<DraftImage>,
    /// 写下时刻（Unix 毫秒），只用于排查。
    pub at_ms: u64,
}

/// 交还给页面的一份草稿。图片在这里是**内联的 base64**：`take` 是读走即删的，页面
/// 取到之后才需要字节，中间没有第二次读的机会——所以壳必须在删除之前把它们读出来。
#[derive(Debug, Clone, Serialize)]
pub struct RestoredDraft {
    pub href: String,
    pub text: String,
    pub images: Vec<RestoredImage>,
    /// 因超过上限而**没能存下**的图有几张。非零时页面要如实说出来——静悄悄地少给
    /// 几张，比明确告诉用户「有 N 张超出草稿上限」更糟。
    pub dropped: u32,
}

/// 草稿的存活上限。
///
/// 一段没发出去的话在两天后还躺在磁盘上，多半是用户已经不需要了，而它可能含
/// 用户不想留在这台机器上的内容。**自动过期是这类数据的默认归宿**。
const DRAFT_TTL_MS: u64 = 24 * 60 * 60 * 1000;

/// 超过这个长度的文本不存。输入框本身能装下的量与它相当，而超长文本更可能是
/// 误粘的一大段文件内容。
const MAX_DRAFT_CHARS: usize = 20_000;

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn draft_file(family: &str, id: &str) -> std::path::PathBuf {
    crate::shell::paths::instance_runtime_dir(family, id).join("harness-draft.json")
}

/// 记下草稿。**既没有文字也没有图 = 没有正在输入的东西** ⇒ 盘上那一份已经作废
/// （多半是用户刚把它发了出去），**清掉**。
///
/// 这一条不是洁癖，是 2026-09-30 用户明说的那条：「已经发送的消息，下次打开工作台
/// 不应该再次填充在输入区域」。只写不清的话，磁盘上会一直留着那句已送达的话，而
/// 恢复那侧看到的只是一段「新鲜的草稿」——它不知道那句话已经躺在会话里了。图片同理。
/// **只发图不发字**是最容易漏的那一种，所以判据是「两者都空」而不是「文字为空」。
///
/// 写失败只落 stderr：**这条路径跑在页面重载的临界点上，绝不能反过来把重载搞挂。**
pub fn stash(family: &str, id: &str, href: &str, text: &str, images: &[ImageInput]) {
    // 判「有没有东西」用 trim，**存的是原文**。用户丢过一次的东西回来时再被悄悄
    // 改掉（哪怕只是首尾空白）是二次伤害；而 trim 后为空的那一串本来就不值得存。
    let text_ok = !text.trim().is_empty() && text.chars().count() <= MAX_DRAFT_CHARS;
    if !text_ok && images.is_empty() {
        clear(family, id);
        return;
    }
    let file = draft_file(family, id);
    if let Some(parent) = file.parent() {
        if std::fs::create_dir_all(parent).is_err() {
            return;
        }
    }
    let (kept, dropped) = crate::harness::harness_media::write(family, id, images);
    let draft = Draft {
        href: href.to_string(),
        text: if text_ok {
            text.to_string()
        } else {
            String::new()
        },
        images: kept,
        at_ms: now_ms(),
    };
    match serde_json::to_vec(&draft) {
        Ok(bytes) => {
            if let Err(error) = crate::shell::process::atomic_write(&file, &bytes) {
                eprintln!("harness-draft: 写草稿失败（不阻断页面）：{error}");
            }
        }
        Err(error) => eprintln!("harness-draft: 序列化失败：{error}"),
    }
    let _ = dropped; // 上限与丢弃的账由 harness_media 记进「查看日志」
}

/// 取走草稿：**读走即删**（图片字节一并删）。
///
/// 一次性是刻意的：草稿文件留在磁盘上，就会在用户下一次**正常**打开工作台时把一段
/// 他早就放弃的话塞回输入框。页面侧必须**先确认能找到可写的输入框**再调它
/// （找不到就别取），否则这一次性的保护反而会吞掉草稿。
pub fn take(family: &str, id: &str) -> Option<RestoredDraft> {
    let file = draft_file(family, id);
    let raw = std::fs::read(&file).ok()?;
    let draft: Draft = serde_json::from_slice(&raw).ok()?;
    // 读走即删：先删再判断有效性，坏文件与过期文件都不会留下来反复被读。
    let _ = std::fs::remove_file(&file);
    // 图片必须在删目录**之前**读出来。
    let images = crate::harness::harness_media::read_and_remove(family, id, &draft.images);
    if now_ms().saturating_sub(draft.at_ms) > DRAFT_TTL_MS {
        return None;
    }
    Some(RestoredDraft {
        href: draft.href,
        text: draft.text,
        dropped: (draft.images.len() - images.len()) as u32,
        images,
    })
}

/// 显式丢弃草稿。三个调用方：输入框恢复成功后、页面报告「输入框空了」（多半是
/// 用户把它发出去了）、以及 `stash` 收到空内容时的兜底。**图片字节一并删**。
pub fn clear(family: &str, id: &str) {
    let _ = std::fs::remove_file(draft_file(family, id));
    crate::harness::harness_media::clear(family, id);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::scoped_xlink_home;

    fn temp_home(tag: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("harness-draft-{tag}-{}", std::process::id()))
    }

    const KERNEL_FAMILY: &str = "dsh";

    /// 一段话要能**原样**回来：中间经过 JSON 与磁盘，空白、换行、emoji 都不能被
    /// 悄悄改掉——用户丢过一次的东西，回来时再变形一次是二次伤害。
    #[test]
    fn a_draft_survives_the_round_trip_unchanged() {
        let home = temp_home("roundtrip");
        let _xlink = scoped_xlink_home(&home);
        std::fs::create_dir_all(&home).expect("create home");

        let text = "  第一行\n第二行 \t 缩进  \n🎯 表情与中文  ";
        stash(
            KERNEL_FAMILY,
            "default",
            "http://127.0.0.1:3090/?session=abc",
            text,
            &[],
        );
        let taken = take(KERNEL_FAMILY, "default").expect("草稿应该还在");
        assert_eq!(taken.href, "http://127.0.0.1:3090/?session=abc");
        assert_eq!(taken.text, text);
        std::fs::remove_dir_all(&home).ok();
    }

    /// 取走即删：否则用户下一次**正常**打开工作台，会被一段他早就放弃的话糊一脸。
    /// 这一条是「草稿不变成垃圾」与「草稿不变成惊吓」的分界。
    #[test]
    fn taking_a_draft_deletes_it() {
        let home = temp_home("once");
        let _xlink = scoped_xlink_home(&home);
        std::fs::create_dir_all(&home).expect("create home");

        stash(KERNEL_FAMILY, "default", "http://x/", "还没发出去的话", &[]);
        assert!(take(KERNEL_FAMILY, "default").is_some());
        assert!(
            take(KERNEL_FAMILY, "default").is_none(),
            "草稿必须是一次性的：留在盘上会在下次正常打开时凭空冒出来"
        );
        std::fs::remove_dir_all(&home).ok();
    }

    /// 空文本不写。用户没在输入东西时，磁盘上不该有草稿，恢复那侧也不该做判断。
    #[test]
    fn an_empty_input_writes_nothing() {
        let home = temp_home("empty");
        let _xlink = scoped_xlink_home(&home);
        std::fs::create_dir_all(&home).expect("create home");

        stash(KERNEL_FAMILY, "default", "http://x/", "", &[]);
        stash(KERNEL_FAMILY, "default", "http://x/", "   \n\t  ", &[]);
        assert!(take(KERNEL_FAMILY, "default").is_none());
        std::fs::remove_dir_all(&home).ok();
    }

    /// **已经发出去的那句话不会再被交回来。** 空输入不只是「不写」，它必须把之前
    /// 存的那一份**删掉**——否则它会静静躺在盘上，等用户下次打开工作台时自己坐回
    /// 输入框，而恢复那侧无从知道那句话已经在会话里了（2026-09-30 用户原话）。
    #[test]
    fn an_emptied_composer_drops_what_was_stashed() {
        let home = temp_home("sent");
        let _xlink = scoped_xlink_home(&home);
        std::fs::create_dir_all(&home).expect("create home");

        stash(
            KERNEL_FAMILY,
            "default",
            "http://x/",
            "已经发出去的一句话",
            &[],
        );
        stash(KERNEL_FAMILY, "default", "http://x/", "", &[]);
        assert!(
            take(KERNEL_FAMILY, "default").is_none(),
            "输入框一空就必须作废：留着就是下次打开工作台时的凭空冒出"
        );
        std::fs::remove_dir_all(&home).ok();
    }

    /// 装不下的那段也不该把更旧的一段留下来顶替它：那份旧草稿已经不代表用户正在
    /// 写的东西，恢复它等于写回一段错的话。
    #[test]
    fn an_oversized_input_drops_the_stale_draft() {
        let home = temp_home("oversized");
        let _xlink = scoped_xlink_home(&home);
        std::fs::create_dir_all(&home).expect("create home");

        stash(KERNEL_FAMILY, "default", "http://x/", "用户正在写的话", &[]);
        let too_long = "长".repeat(MAX_DRAFT_CHARS + 1);
        stash(KERNEL_FAMILY, "default", "http://x/", &too_long, &[]);
        assert!(
            take(KERNEL_FAMILY, "default").is_none(),
            "存不下现在这段时，交回一段更旧的话比不交更糟"
        );
        std::fs::remove_dir_all(&home).ok();
    }

    /// `clear` 之后草稿真的不在盘上——它是「作废」那半边唯一的落点。
    #[test]
    fn clearing_removes_the_draft() {
        let home = temp_home("clear");
        let _xlink = scoped_xlink_home(&home);
        std::fs::create_dir_all(&home).expect("create home");

        stash(KERNEL_FAMILY, "default", "http://x/", "还没发出去的话", &[]);
        clear(KERNEL_FAMILY, "default");
        assert!(!draft_file(KERNEL_FAMILY, "default").exists());
        assert!(take(KERNEL_FAMILY, "default").is_none());
        std::fs::remove_dir_all(&home).ok();
    }

    fn image(name: &str, bytes: &[u8]) -> ImageInput {
        ImageInput {
            name: name.into(),
            mime: "image/png".into(),
            data: crate::harness::harness_media::encode_base64(bytes),
        }
    }

    /// **只发图、不打字是最容易漏的那一种**：`text` 为空但有图时草稿必须留下，否则
    /// 用户甩进来一张截图、页面一崩，图片就无声无息地没了。
    #[test]
    fn an_image_alone_is_enough_to_keep_a_draft() {
        let home = temp_home("imageonly");
        let _xlink = scoped_xlink_home(&home);
        std::fs::create_dir_all(&home).expect("create home");

        stash(
            KERNEL_FAMILY,
            "default",
            "http://x/",
            "",
            &[image("a.png", &[1, 2, 3])],
        );
        let taken = take(KERNEL_FAMILY, "default").expect("只有图也要留着");
        assert_eq!(taken.text, "");
        assert_eq!(taken.images.len(), 1);
        std::fs::remove_dir_all(&home).ok();
    }

    /// 图被删掉（用户甩进来又删了）也要作废。**判据是「两者都空」**：只看文字的话，
    /// 这种「发过一张图、没打字」的消息会连着上一份草稿一起留在盘上。
    #[test]
    fn a_removed_image_also_drops_the_draft() {
        let home = temp_home("imagedrop");
        let _xlink = scoped_xlink_home(&home);
        std::fs::create_dir_all(&home).expect("create home");

        stash(
            KERNEL_FAMILY,
            "default",
            "http://x/",
            "",
            &[image("a.png", &[1, 2, 3])],
        );
        stash(KERNEL_FAMILY, "default", "http://x/", "", &[]);
        assert!(take(KERNEL_FAMILY, "default").is_none());
        assert!(
            !crate::harness::harness_media::media_dir(KERNEL_FAMILY, "default").exists(),
            "作废时图片字节也要一起清掉，不能留在盘上"
        );
        std::fs::remove_dir_all(&home).ok();
    }

    /// 一次 `take` 之后媒体目录整个消失——图片字节不能比它描述的那份草稿活得更久，
    /// 而字节必须在删除之前取出来（否则页面拿到的是有元数据没图片的草稿）。
    #[test]
    fn taking_a_draft_removes_its_images() {
        let home = temp_home("imagetake");
        let _xlink = scoped_xlink_home(&home);
        std::fs::create_dir_all(&home).expect("create home");

        stash(
            KERNEL_FAMILY,
            "default",
            "http://x/",
            "带图",
            &[image("a.png", &[137, 80, 78, 71])],
        );
        let taken = take(KERNEL_FAMILY, "default").expect("草稿应该还在");
        assert_eq!(taken.images.len(), 1);
        assert!(!crate::harness::harness_media::media_dir(KERNEL_FAMILY, "default").exists());
        assert!(take(KERNEL_FAMILY, "default").is_none());
        std::fs::remove_dir_all(&home).ok();
    }

    /// 上限之上的部分**如实丢掉**并把张数带回页面，不假装都存下了。
    #[test]
    fn images_over_the_cap_are_dropped_but_the_rest_survive() {
        let home = temp_home("cap");
        let _xlink = scoped_xlink_home(&home);
        std::fs::create_dir_all(&home).expect("create home");

        let mut all = vec![image(
            "big.png",
            &vec![0x5au8; crate::harness::harness_media::MAX_IMAGE_BYTES + 1],
        )];
        for index in 0..crate::harness::harness_media::MAX_IMAGES + 3 {
            all.push(image(&format!("{index}.png"), &[1, 2, 3, 4]));
        }
        stash(KERNEL_FAMILY, "default", "http://x/", "一堆图", &all);
        let taken = take(KERNEL_FAMILY, "default").expect("草稿应该还在");
        assert_eq!(
            taken.images.len(),
            crate::harness::harness_media::MAX_IMAGES,
            "超出的丢掉，留下的都要在"
        );
        std::fs::remove_dir_all(&home).ok();
    }

    /// 加图片之前写下的草稿没有 `images` 字段，读它不能失败——那份文件最多再活一天，
    /// 但「读不回来」的症状是「用户的话凭空消失」，那不能靠过期兜底。
    #[test]
    fn a_draft_written_before_images_existed_still_parses() {
        let home = temp_home("legacy");
        let _xlink = scoped_xlink_home(&home);
        std::fs::create_dir_all(&home).expect("create home");

        let file = draft_file(KERNEL_FAMILY, "default");
        std::fs::create_dir_all(file.parent().unwrap()).expect("create dir");
        // `at_ms` 必须是「现在」：过期那份在另一条用例里钉着，这里要验的是**解析**。
        let legacy = format!(
            r#"{{"href":"http://x/","text":"no images field","at_ms":{}}}"#,
            now_ms()
        );
        std::fs::write(&file, legacy).expect("write");
        let taken = take(KERNEL_FAMILY, "default").expect("旧草稿必须读得回来");
        assert_eq!(taken.text, "no images field");
        assert!(taken.images.is_empty());
        std::fs::remove_dir_all(&home).ok();
    }

    /// 过期即失效。草稿可能含用户不想留在这台机器上的内容，放着不过期是数据卫生
    /// 问题，而不只是「UI 不干净」。
    #[test]
    fn an_expired_draft_is_not_handed_back() {
        let home = temp_home("ttl");
        let _xlink = scoped_xlink_home(&home);
        std::fs::create_dir_all(&home).expect("create home");

        let file = draft_file(KERNEL_FAMILY, "default");
        std::fs::create_dir_all(file.parent().unwrap()).expect("create dir");
        let stale = Draft {
            href: "http://x/".into(),
            text: "两天前没发出去的话".into(),
            images: Vec::new(),
            at_ms: now_ms() - DRAFT_TTL_MS - 1000,
        };
        std::fs::write(&file, serde_json::to_vec(&stale).unwrap()).expect("write");
        assert!(
            take(KERNEL_FAMILY, "default").is_none(),
            "过期草稿不该被交回来"
        );
        assert!(!file.exists(), "过期草稿也要被清掉，不能留在盘上反复被读");
        std::fs::remove_dir_all(&home).ok();
    }
}
