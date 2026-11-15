//! 附件内容校验：只做结构与安全边界，不执行、不渲染、不落业务含义。
//!
//! P2 白名单：PNG / JPEG / WebP 静态图与纯文本日志。图片校验 **头部/容器结构、
//! 尺寸与像素上限、PNG 的 zlib 流完整性**；不做完整像素解码（避免为三种格式引入
//! 解码器依赖与大内存分配），这是已知边界，记录在设计契约 §15。
//! 文本只校验 UTF-8（声明为 gb18030/gbk 的字节原样保留，预览失败在 P3 展示）。

use std::io::Read;
use std::path::Path;
use std::time::{Duration, Instant};

#[derive(Debug)]
pub(crate) struct MediaReject {
    pub status: u16,
    pub message: String,
}

impl MediaReject {
    fn invalid(message: impl Into<String>) -> Self {
        Self {
            status: 400,
            message: message.into(),
        }
    }

    fn unsupported(message: impl Into<String>) -> Self {
        Self {
            status: 415,
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct ImageInfo {
    pub width: u32,
    pub height: u32,
}

/// 按声明的媒体类型校验图片；返回尺寸。任何异常都不返回“已存储”。
pub(crate) fn validate_image(
    bytes: &[u8],
    media_type: &str,
    max_pixels: u64,
) -> Result<ImageInfo, MediaReject> {
    let info = match media_type {
        "image/png" => validate_png(bytes)?,
        "image/jpeg" => validate_jpeg(bytes)?,
        "image/webp" => validate_webp(bytes)?,
        _ => {
            return Err(MediaReject::unsupported(format!(
                "不支持的图片类型 {media_type}"
            )))
        }
    };
    let pixels = u64::from(info.width) * u64::from(info.height);
    if info.width == 0 || info.height == 0 || pixels > max_pixels {
        return Err(MediaReject::unsupported(format!(
            "图片尺寸 {}×{} 超过像素上限 {max_pixels}",
            info.width, info.height
        )));
    }
    Ok(info)
}

fn read_u32_be(bytes: &[u8], offset: usize) -> Option<u32> {
    let chunk = bytes.get(offset..offset + 4)?;
    Some(u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
}

fn validate_png(bytes: &[u8]) -> Result<ImageInfo, MediaReject> {
    const SIGNATURE: [u8; 8] = [0x89, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];
    if !bytes.starts_with(&SIGNATURE) {
        return Err(MediaReject::invalid("文件不是有效的 PNG（签名不符）"));
    }
    let mut offset = 8usize;
    let mut info: Option<ImageInfo> = None;
    let mut header: Option<(u8, u8)> = None;
    let mut idat: Vec<u8> = Vec::new();
    let mut saw_iend = false;
    let mut saw_animation = false;
    while offset + 12 <= bytes.len() {
        let Some(length) = read_u32_be(bytes, offset) else {
            return Err(MediaReject::invalid("PNG 块长度损坏"));
        };
        let length = length as usize;
        let chunk_end = offset
            .checked_add(8)
            .and_then(|value| value.checked_add(length))
            .ok_or_else(|| MediaReject::invalid("PNG 块长度溢出"))?;
        if length > 64 * 1024 * 1024 || chunk_end + 4 > bytes.len() {
            return Err(MediaReject::invalid("PNG 块超出文件范围"));
        }
        let kind = &bytes[offset + 4..offset + 8];
        let data = &bytes[offset + 8..chunk_end];
        let stored_crc =
            read_u32_be(bytes, chunk_end).ok_or_else(|| MediaReject::invalid("PNG CRC 缺失"))?;
        let mut crc = flate2::Crc::new();
        crc.update(&bytes[offset + 4..chunk_end]);
        if crc.sum() != stored_crc {
            return Err(MediaReject::invalid("PNG 块校验失败"));
        }
        match kind {
            b"IHDR" => {
                if data.len() < 13 {
                    return Err(MediaReject::invalid("PNG IHDR 长度错误"));
                }
                let width = read_u32_be(data, 0).unwrap_or(0);
                let height = read_u32_be(data, 4).unwrap_or(0);
                let bit_depth = data[8];
                let color_type = data[9];
                let compression = data[10];
                let filter = data[11];
                let interlace = data[12];
                if compression != 0 || filter != 0 {
                    return Err(MediaReject::invalid("PNG 压缩/filter 方法非法"));
                }
                if interlace != 0 {
                    return Err(MediaReject::unsupported("不支持隔行扫描（interlaced）PNG"));
                }
                info = Some(ImageInfo { width, height });
                header = Some((bit_depth, color_type));
            }
            b"IDAT" => idat.extend_from_slice(data),
            b"acTL" => saw_animation = true,
            b"IEND" => {
                saw_iend = true;
                break;
            }
            _ => {}
        }
        offset = chunk_end + 4;
    }
    if saw_animation {
        return Err(MediaReject::unsupported("不支持 APNG 动画"));
    }
    let info = info.ok_or_else(|| MediaReject::invalid("PNG 缺少 IHDR"))?;
    let (bit_depth, color_type) = header.ok_or_else(|| MediaReject::invalid("PNG 缺少 IHDR"))?;
    if !saw_iend {
        return Err(MediaReject::invalid("PNG 缺少 IEND"));
    }
    let channels = match color_type {
        0 => 1u64,
        2 => 3,
        4 => 2,
        6 => 4,
        _ => return Err(MediaReject::unsupported("不支持调色板或未知 PNG 颜色类型")),
    };
    if !matches!(bit_depth, 8 | 16) {
        return Err(MediaReject::unsupported("只支持 8/16 位深度的 PNG"));
    }
    let bpp = channels * u64::from(bit_depth) / 8;
    let expected = u64::from(info.height)
        .checked_mul(
            1u64.checked_add(u64::from(info.width).saturating_mul(bpp))
                .ok_or_else(|| MediaReject::invalid("PNG 尺寸溢出"))?,
        )
        .ok_or_else(|| MediaReject::invalid("PNG 尺寸溢出"))?;
    validate_png_zlib(&idat, expected)?;
    Ok(info)
}

/// PNG 的 IDAT 是 zlib 流：流式解压并核对解压后行字节数，等价于证明压缩数据可解。
fn validate_png_zlib(idat: &[u8], expected: u64) -> Result<(), MediaReject> {
    if idat.is_empty() {
        return Err(MediaReject::invalid("PNG 缺少 IDAT"));
    }
    let mut decoder = flate2::read::ZlibDecoder::new(idat);
    let mut buffer = [0u8; 64 * 1024];
    let deadline = Instant::now() + Duration::from_secs(3);
    let mut total = 0u64;
    loop {
        let read = decoder
            .read(&mut buffer)
            .map_err(|_| MediaReject::invalid("PNG 压缩数据无法解码"))?;
        if read == 0 {
            break;
        }
        total += read as u64;
        if total > expected {
            return Err(MediaReject::invalid("PNG 解压后大小超出声明"));
        }
        if Instant::now() > deadline {
            return Err(MediaReject::unsupported("PNG 解码超时"));
        }
    }
    if total != expected {
        return Err(MediaReject::invalid("PNG 解压后大小与尺寸不符"));
    }
    Ok(())
}

fn validate_jpeg(bytes: &[u8]) -> Result<ImageInfo, MediaReject> {
    if bytes.len() < 4 || bytes[0] != 0xFF || bytes[1] != 0xD8 {
        return Err(MediaReject::invalid("文件不是有效的 JPEG（缺少 SOI）"));
    }
    let mut offset = 2usize;
    let mut info: Option<ImageInfo> = None;
    let mut saw_scan = false;
    while offset + 1 < bytes.len() {
        if bytes[offset] != 0xFF {
            return Err(MediaReject::invalid("JPEG 段边界损坏"));
        }
        let marker = bytes[offset + 1];
        offset += 2;
        if marker == 0x00 || marker == 0x01 || (0xD0..=0xD7).contains(&marker) {
            continue;
        }
        if marker == 0xD8 {
            return Err(MediaReject::invalid("JPEG 出现嵌套 SOI"));
        }
        if marker == 0xD9 {
            break;
        }
        if offset + 2 > bytes.len() {
            return Err(MediaReject::invalid("JPEG 段长度缺失"));
        }
        let segment_length = u16::from_be_bytes([bytes[offset], bytes[offset + 1]]) as usize;
        if segment_length < 2 || offset + segment_length > bytes.len() {
            return Err(MediaReject::invalid("JPEG 段超出文件范围"));
        }
        let data = &bytes[offset + 2..offset + segment_length];
        match marker {
            0xC0..=0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF => {
                if data.len() < 6 {
                    return Err(MediaReject::invalid("JPEG SOF 长度错误"));
                }
                let height = u16::from_be_bytes([data[1], data[2]]) as u32;
                let width = u16::from_be_bytes([data[3], data[4]]) as u32;
                info = Some(ImageInfo { width, height });
            }
            0xDA => {
                saw_scan = true;
                let rest = &bytes[offset + segment_length..];
                if !rest.windows(2).any(|window| window == [0xFF, 0xD9]) {
                    return Err(MediaReject::invalid("JPEG 缺少 EOI"));
                }
                break;
            }
            _ => {}
        }
        offset += segment_length;
    }
    if !saw_scan {
        return Err(MediaReject::invalid("JPEG 缺少扫描数据（SOS）"));
    }
    info.ok_or_else(|| MediaReject::invalid("JPEG 缺少 SOF 尺寸"))
}

fn read_u24_le(bytes: &[u8], offset: usize) -> Option<u32> {
    let chunk = bytes.get(offset..offset + 3)?;
    Some(u32::from(chunk[0]) | (u32::from(chunk[1]) << 8) | (u32::from(chunk[2]) << 16))
}

fn validate_webp(bytes: &[u8]) -> Result<ImageInfo, MediaReject> {
    if bytes.len() < 16 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WEBP" {
        return Err(MediaReject::invalid("文件不是有效的 WebP（RIFF 头不符）"));
    }
    let riff_size = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]) as usize;
    if riff_size + 8 > bytes.len() {
        return Err(MediaReject::invalid("WebP RIFF 长度超出文件"));
    }
    let mut offset = 12usize;
    let mut canvas: Option<ImageInfo> = None;
    let mut image: Option<ImageInfo> = None;
    let end = (riff_size + 8).min(bytes.len());
    while offset + 8 <= end {
        let fourcc = &bytes[offset..offset + 4];
        let size = u32::from_le_bytes([
            bytes[offset + 4],
            bytes[offset + 5],
            bytes[offset + 6],
            bytes[offset + 7],
        ]) as usize;
        let data_start = offset + 8;
        let data_end = data_start
            .checked_add(size)
            .ok_or_else(|| MediaReject::invalid("WebP 块长度溢出"))?;
        if data_end > end {
            return Err(MediaReject::invalid("WebP 块超出文件范围"));
        }
        let data = &bytes[data_start..data_end];
        match fourcc {
            b"VP8X" => {
                if data.len() < 10 {
                    return Err(MediaReject::invalid("WebP VP8X 长度错误"));
                }
                if data[0] & 0x02 != 0 {
                    return Err(MediaReject::unsupported("不支持动画 WebP"));
                }
                let width = read_u24_le(data, 4).unwrap_or(0) + 1;
                let height = read_u24_le(data, 7).unwrap_or(0) + 1;
                canvas = Some(ImageInfo { width, height });
            }
            b"ANIM" | b"ANMF" => {
                return Err(MediaReject::unsupported("不支持动画 WebP"));
            }
            b"VP8 " => {
                if data.len() < 10 || data[3..6] != [0x9d, 0x01, 0x2a] {
                    return Err(MediaReject::invalid("WebP VP8 头不完整"));
                }
                let width = u16::from_le_bytes([data[6], data[7]]) & 0x3FFF;
                let height = u16::from_le_bytes([data[8], data[9]]) & 0x3FFF;
                image = Some(ImageInfo {
                    width: u32::from(width),
                    height: u32::from(height),
                });
            }
            b"VP8L" => {
                if data.len() < 5 || data[0] != 0x2F {
                    return Err(MediaReject::invalid("WebP VP8L 头不完整"));
                }
                let bits = u32::from_le_bytes([data[1], data[2], data[3], data[4]]);
                let width = (bits & 0x3FFF) + 1;
                let height = ((bits >> 14) & 0x3FFF) + 1;
                image = Some(ImageInfo { width, height });
            }
            _ => {}
        }
        // RIFF 块按偶数字节对齐。
        offset = data_end + (size & 1);
    }
    let info = image
        .or(canvas)
        .ok_or_else(|| MediaReject::invalid("WebP 缺少图像数据块"))?;
    if let Some(canvas) = canvas {
        if canvas.width < info.width || canvas.height < info.height {
            return Err(MediaReject::invalid("WebP 画布小于图像尺寸"));
        }
    }
    Ok(info)
}

/// 文本日志：声明 utf-8 时做流式校验；gb18030/gbk 无解码器，原字节保留，
/// 预览失败在 P3 可见（不静默替换）。
pub(crate) fn validate_text_file(path: &Path, encoding: &str) -> Result<(), String> {
    if encoding != "utf-8" {
        return Ok(());
    }
    let mut file =
        std::fs::File::open(path).map_err(|error| format!("无法读取日志文件：{error}"))?;
    let mut buffer = [0u8; 64 * 1024];
    let mut carry: Vec<u8> = Vec::new();
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| format!("无法读取日志文件：{error}"))?;
        if read == 0 {
            break;
        }
        carry.extend_from_slice(&buffer[..read]);
        match std::str::from_utf8(&carry) {
            Ok(_) => carry.clear(),
            Err(error) => {
                if error.error_len().is_some() {
                    return Err("日志内容不是有效的 UTF-8".into());
                }
                let valid = error.valid_up_to();
                carry.drain(..valid);
            }
        }
        if carry.len() > 3 {
            return Err("日志内容不是有效的 UTF-8".into());
        }
    }
    if !carry.is_empty() {
        return Err("日志内容不是有效的 UTF-8（结尾不完整）".into());
    }
    Ok(())
}
