//! 草稿图片的**字节与上限**：落盘、读回、base64 收发、以及「存不下」时如实说清。
//!
//! ## 为什么单独成文件
//!
//! 它回答的是「一段二进制怎么进出这个进程」，与「草稿什么时候存在、什么时候作废」
//! 是两回事：前者是编解码与配额，后者是生命周期。混在一起的后果是
//! `harness_draft.rs` 里 base64 的位运算淹掉了草稿本身的语义，而调上限的人分不清
//! 自己动的是哪一半。
//!
//! ## 上限为什么比内核更紧
//!
//! 内核允许单张 20MB / 20 张 / 合计 200MB（`dsh-attachment-local` 的默认值），而这里
//! 的字节还要以 base64 过一次 Tauri IPC（约为原体积的 4/3）。几百 KB 的截图是常态、
//! 几 MB 已经少见，所以取 8 张 / 单张 4MB / 合计 12MB：**存不下就如实说存不下**
//! （`dropped` 一路带回页面并落进「查看日志」），不假装都存下了。

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// 草稿图片的三个上限。
pub const MAX_IMAGES: usize = 8;
pub const MAX_IMAGE_BYTES: usize = 4 * 1024 * 1024;
pub const MAX_TOTAL_IMAGE_BYTES: usize = 12 * 1024 * 1024;

/// 草稿里的一张图（**落盘**形态）。字节在 [`media_dir`] 下的同名文件里，JSON 只记
/// 文件名与元数据——把 base64 塞进 JSON 会让一张几百 KB 的截图变成同样大的文本
/// 文件，而那个文件每次停顿输入都要整份重写。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DraftImage {
    /// 原文件名（多半是 `image.png` 之类）。只作显示用，**不参与路径**。
    pub name: String,
    /// 浏览器给出的 MIME。内核只认 png / jpeg / webp / gif 四种（`isImageMediaType`），
    /// 别的在采集侧就不会出现，这里仍然按未知处理（落 `.bin`）。
    pub mime: String,
    /// 媒体目录里的文件名（`01.png` 这样，由壳生成，不是页面给的名字）。
    pub file: String,
}

/// 页面递上来的一张图：base64 字节 + 元数据（尚未落盘）。
#[derive(Debug, Clone, Deserialize)]
pub struct ImageInput {
    pub name: String,
    pub mime: String,
    /// 标准 base64（无 data URL 前缀）。
    pub data: String,
}

/// 交还给页面的一张图（字节是内联的 base64）。
#[derive(Debug, Clone, Serialize)]
pub struct RestoredImage {
    pub name: String,
    pub mime: String,
    pub data: String,
}

/// 草稿图片的落盘目录。**整目录属于当前这一份草稿**——没有按草稿 id 再分一层：
/// 每次 [`write`] 开头就把它整个删掉重建，于是上一次写了一半留下的孤儿文件会被下一
/// 次顺手收走，不需要任何清扫任务。
pub fn media_dir(family: &str, id: &str) -> PathBuf {
    crate::shell::paths::instance_runtime_dir(family, id).join("harness-draft-media")
}

/// 把页面递来的图写进媒体目录，按上限取舍。返回**存下的那些**与**丢掉的张数**。
///
/// 顺序即优先级：先到的先留。用户一次贴十几张时，保住的是他先贴的那几张。
pub fn write(family: &str, id: &str, images: &[ImageInput]) -> (Vec<DraftImage>, u32) {
    // 先把上一份的媒体整个删掉：它既是「换内容时别留旧图」的需要，也是**孤儿自愈**
    // （上一次写了一半就断了）的需要。
    let _ = std::fs::remove_dir_all(media_dir(family, id));
    let mut kept: Vec<DraftImage> = Vec::new();
    let mut total = 0usize;
    let mut dropped = 0u32;
    for image in images {
        let Some(bytes) = decode_base64(&image.data) else {
            dropped += 1;
            continue;
        };
        if kept.len() >= MAX_IMAGES
            || bytes.is_empty()
            || bytes.len() > MAX_IMAGE_BYTES
            || total + bytes.len() > MAX_TOTAL_IMAGE_BYTES
        {
            dropped += 1;
            continue;
        }
        let file = format!("{:02}.{}", kept.len() + 1, extension_of(&image.mime));
        if std::fs::create_dir_all(media_dir(family, id)).is_err()
            || std::fs::write(media_dir(family, id).join(&file), &bytes).is_err()
        {
            dropped += 1;
            continue;
        }
        total += bytes.len();
        kept.push(DraftImage {
            name: image.name.clone(),
            mime: image.mime.clone(),
            file,
        });
    }
    if dropped > 0 {
        // 壳是 GUI 程序，eprintln! 在 Windows 上没有去处，而「用户的图少了几张」正是
        // 那种只有后果没有原因的事。落进 shell_events 后它自动出现在「查看日志」里。
        crate::shell::shell_events::record(
            "harness-draft",
            &format!("{dropped} 张图超过草稿上限（{MAX_IMAGES} 张 / 单张 4MB / 合计 12MB）没存下"),
        );
    }
    (kept, dropped)
}

/// 读回一份草稿的全部图片，**顺带把媒体目录删掉**。
///
/// 「读走即删」是对整份草稿的承诺，图片是它的一部分，所以这里必须在返回之前就把
/// 字节读出来——否则页面拿到的是一张有元数据、没图片的草稿，而字节已经被删了。
pub fn read_and_remove(family: &str, id: &str, draft: &[DraftImage]) -> Vec<RestoredImage> {
    let dir = media_dir(family, id);
    let images: Vec<RestoredImage> = draft
        .iter()
        .filter_map(|image| {
            let bytes = std::fs::read(dir.join(&image.file)).ok()?;
            Some(RestoredImage {
                name: image.name.clone(),
                mime: image.mime.clone(),
                data: encode_base64(&bytes),
            })
        })
        .collect();
    let _ = std::fs::remove_dir_all(&dir);
    images
}

/// 丢掉媒体目录（草稿作废时）。字节不能比描述它的那份草稿活得更久。
pub fn clear(family: &str, id: &str) {
    let _ = std::fs::remove_dir_all(media_dir(family, id));
}

fn extension_of(mime: &str) -> &'static str {
    match mime {
        "image/png" => "png",
        "image/jpeg" => "jpg",
        "image/webp" => "webp",
        "image/gif" => "gif",
        _ => "bin",
    }
}

/// 标准 base64 解码。**非法字符一律返回 None**：存进一半的字节恢复出来是一张花屏，
/// 那比丢掉这张图更糟。
fn decode_base64(data: &str) -> Option<Vec<u8>> {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = Vec::with_capacity(data.len() / 4 * 3);
    let mut acc: u32 = 0;
    let mut bits = 0;
    for byte in data.bytes() {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' | b'\n' | b'\r' => continue,
            _ => return None,
        };
        if TABLE[value as usize] != byte {
            return None;
        }
        acc = (acc << 6) | u32::from(value);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push(((acc >> bits) & 0xff) as u8);
        }
    }
    Some(out)
}

/// 标准 base64 编码。`pub` 是因为草稿那一侧的测试也要用它造样本——两边共用一份编解码
/// 才谈得上「往返一次字节不变」。
pub fn encode_base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let bits = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        // 尾部不足 3 字节时按缺几个补几个 '='：缺 1 字节后两格都补，缺 2 字节只补最后一格。
        let tail = [TABLE[(bits >> 6) as usize & 63], TABLE[bits as usize & 63]];
        for slot in [
            TABLE[(bits >> 18) as usize & 63],
            TABLE[(bits >> 12) as usize & 63],
            if chunk.len() > 1 { tail[0] } else { b'=' },
            if chunk.len() > 2 { tail[1] } else { b'=' },
        ] {
            out.push(char::from(slot));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::scoped_xlink_home;

    fn temp_home(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!("harness-media-{tag}-{}", std::process::id()))
    }

    const FAMILY: &str = "dsh";

    fn input(name: &str, bytes: &[u8]) -> ImageInput {
        ImageInput {
            name: name.into(),
            mime: "image/png".into(),
            data: encode_base64(bytes),
        }
    }

    /// base64 要能扛住**任意长度**的字节：3n / 3n+1 / 3n+2 三种补位都要对。
    /// 编解码错一个字节，恢复出来的就是一张用户没见过的图。
    #[test]
    fn base64_round_trips_every_padding_case() {
        for len in 0..24usize {
            let bytes: Vec<u8> = (0..len).map(|i| (i * 37 % 251) as u8).collect();
            let encoded = encode_base64(&bytes);
            assert_eq!(
                encoded.len() % 4,
                0,
                "长度 {len} 的输出必须是 4 的倍数：{encoded}"
            );
            assert_eq!(
                decode_base64(&encoded).as_deref(),
                Some(bytes.as_slice()),
                "长度 {len} 的字节要原样回来"
            );
        }
    }

    /// 已知向量：空串、`f`、`fo`、`foo`、`foob`、以及 RFC 4649 的 "foobar"。
    #[test]
    fn base64_matches_the_known_vectors() {
        assert_eq!(encode_base64(b""), "");
        assert_eq!(encode_base64(b"f"), "Zg==");
        assert_eq!(encode_base64(b"fo"), "Zm8=");
        assert_eq!(encode_base64(b"foo"), "Zm9v");
        assert_eq!(encode_base64(b"foob"), "Zm9vYg==");
        assert_eq!(encode_base64(b"foobar"), "Zm9vYmFy");
    }

    /// 写进去再读出来，**逐字节**相同。文件名由壳生成，与页面给的名字无关。
    #[test]
    fn images_survive_write_and_read() {
        let home = temp_home("roundtrip");
        let _xlink = scoped_xlink_home(&home);
        std::fs::create_dir_all(&home).expect("create home");

        let png = [
            0x89u8, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a, 0x00, 0xff, 0x7f, 0x80,
        ];
        let (kept, dropped) = write(
            FAMILY,
            "default",
            &[input("截图.png", &png), input("b.jpg", &[1, 2, 3])],
        );
        assert_eq!(dropped, 0);
        assert_eq!(kept.len(), 2);
        assert_eq!(kept[0].name, "截图.png");
        assert_eq!(kept[0].file, "01.png", "文件名由壳生成，扩展名跟着 MIME 走");

        let read = read_and_remove(FAMILY, "default", &kept);
        assert_eq!(read.len(), 2);
        assert_eq!(
            decode_base64(&read[0].data).as_deref(),
            Some(png.as_slice())
        );
        assert!(
            !media_dir(FAMILY, "default").exists(),
            "读走即删：字节不能留下来"
        );
        std::fs::remove_dir_all(&home).ok();
    }

    /// 上限之上如实丢，**且先到的先留**——用户一次贴十几张时保住的是他先贴的那几张。
    #[test]
    fn over_the_cap_drops_the_later_ones_and_says_how_many() {
        let home = temp_home("cap");
        let _xlink = scoped_xlink_home(&home);
        std::fs::create_dir_all(&home).expect("create home");

        let many: Vec<ImageInput> = (0..MAX_IMAGES + 3)
            .map(|i| input(&format!("{i}.png"), &[i as u8, 1, 2]))
            .collect();
        let (kept, dropped) = write(FAMILY, "default", &many);
        assert_eq!(kept.len(), MAX_IMAGES);
        assert_eq!(dropped, 3, "丢了几张要报出来：悄悄少给比说清楚更糟");
        assert_eq!(kept[0].name, "0.png");
        std::fs::remove_dir_all(&home).ok();
    }

    /// 单张超限的整张丢掉，**不留半个文件**——恢复出来的半张图是花屏。
    #[test]
    fn an_oversized_image_is_dropped_whole() {
        let home = temp_home("oversized");
        let _xlink = scoped_xlink_home(&home);
        std::fs::create_dir_all(&home).expect("create home");

        let big = vec![0x5au8; MAX_IMAGE_BYTES + 1];
        let (kept, dropped) = write(FAMILY, "default", &[input("big.png", &big)]);
        assert!(kept.is_empty());
        assert_eq!(dropped, 1);
        assert!(!media_dir(FAMILY, "default").exists());
        std::fs::remove_dir_all(&home).ok();
    }

    /// 坏 base64 整张丢掉。**花屏比丢图更糟。**
    #[test]
    fn corrupt_base64_drops_the_image_entirely() {
        assert!(decode_base64("AA*EC").is_none());
        assert!(decode_base64("中文").is_none());
        // 合法字符但被截断的串不报错：它解出来就是短一截的字节，交给上层的长度检查。
        assert_eq!(decode_base64("AAEC").map(|b| b.len()), Some(3));
    }

    /// 换内容时不留旧图：第二次写完之后目录里只应有新的一份。
    #[test]
    fn a_second_write_leaves_no_trace_of_the_first() {
        let home = temp_home("replace");
        let _xlink = scoped_xlink_home(&home);
        std::fs::create_dir_all(&home).expect("create home");

        let (first, _) = write(FAMILY, "default", &[input("a.png", &[1, 2, 3])]);
        let (second, _) = write(FAMILY, "default", &[input("b.png", &[4, 5])]);
        assert_eq!(
            first[0].file, second[0].file,
            "两张都叫 01.png，靠整目录重建来区分"
        );
        let read = read_and_remove(FAMILY, "default", &second);
        assert_eq!(
            decode_base64(&read[0].data).as_deref(),
            Some([4u8, 5].as_slice()),
            "读回来的是新内容，不是上次那份"
        );
        std::fs::remove_dir_all(&home).ok();
    }
}
