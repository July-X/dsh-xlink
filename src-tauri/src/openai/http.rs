//! 桥接与回调监听共用的极简 HTTP 读写（仅本组内部使用）。
//!
//! 端点固定、客户端只有内核 Host 插件与本机浏览器重定向，不引框架；
//! `httparse` 已在依赖树。**请求必须读到头结束**：TCP 无消息边界，单次
//! read 可能只拿到「GET 」前几个字节（bridge 实测踩过），循环读到
//! CRLFCRLF 为止。

use std::collections::HashMap;
use std::io::{Read, Write};
use std::net::TcpStream;

pub(crate) struct RequestHead {
    pub(crate) method: String,
    /// 路径含查询串（如 `/callback?code=..&state=..`）。
    pub(crate) path: String,
    pub(crate) headers: HashMap<String, String>,
    /// 头结束符之后已经读进缓冲的字节（请求体前缀）。TCP 无消息边界，
    /// 一次 read 会把头和体一起带来；不带它，读体的一方会去 socket 上
    /// 等「已经消费掉的字节」——双方互等死锁（mock 模拟器实测踩过）。
    pub(crate) body_prefix: Vec<u8>,
}

/// 读一个请求头（不支持请求体；本组端点全是 GET）。连接关闭或超长返回 Err。
pub(crate) fn read_request(stream: &mut TcpStream) -> Result<RequestHead, String> {
    let mut buf = [0u8; 8192];
    let mut read = 0usize;
    while read < buf.len() {
        let n = stream
            .read(&mut buf[read..])
            .map_err(|e| format!("读取请求失败：{e}"))?;
        if n == 0 {
            break;
        }
        read += n;
        if buf[..read].windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
    }
    if read == 0 {
        return Err("连接在发送请求前关闭".into());
    }
    // 找头结束符位置，保留其后的字节作为体前缀。
    let mut body_prefix: Vec<u8> = Vec::new();
    if let Some(position) = buf[..read].windows(4).position(|w| w == b"\r\n\r\n") {
        body_prefix = buf[position + 4..read].to_vec();
    }
    let mut headers = [httparse::EMPTY_HEADER; 48];
    let mut parsed = httparse::Request::new(&mut headers);
    match parsed.parse(&buf[..read]) {
        Ok(httparse::Status::Complete(_)) => {}
        Ok(httparse::Status::Partial) => return Err("请求头不完整（超过 8KB 或未终止）".into()),
        Err(error) => return Err(format!("请求解析失败：{error}")),
    }
    let header_map = parsed
        .headers
        .iter()
        .map(|header| {
            (
                header.name.to_ascii_lowercase(),
                String::from_utf8_lossy(header.value).trim().to_string(),
            )
        })
        .collect();
    Ok(RequestHead {
        method: parsed.method.unwrap_or_default().to_string(),
        path: parsed.path.unwrap_or_default().to_string(),
        headers: header_map,
        body_prefix,
    })
}

/// 读请求体（按 `content-length`），**写死上限**。
///
/// 现役桥原来只读请求头、不读请求体（`bridge.rs` 的路由全是 GET）⇒ 协议
/// §5.2 第 1 条要求「**必须 POST ＋ 支持读请求体**」时，这一层是配套的另一半。
///
/// ## 为什么要上限
///
/// TCP 无消息边界：对端可以只声明一个巨大的 `content-length` 就把内存吃光。
/// 上限是**调用方给的常量**（不是这里定的）——每个路由的上限不一样，
/// 写死在这里会让「这条路由能收多大」这件事看不出来。
///
/// ## 头与体可能分两次到达
///
/// `read_request` 已经把「头结束符之后已经读进缓冲的字节」放进
/// `body_prefix` ⇒ 这里先接上它，再从 socket 上补齐剩下的。
///
/// @param stream - 连接。
/// @param head - 已读到的请求头。
/// @param max_bytes - 请求体上限（字节）。
/// @returns 请求体字节；没有 `content-length` 时是空。
pub(crate) fn read_body(
    stream: &mut TcpStream,
    head: &RequestHead,
    max_bytes: usize,
) -> Result<Vec<u8>, String> {
    let Some(raw) = head.headers.get("content-length") else {
        // 没有声明长度 ⇒ 没有体（这是 GET 的常态）。
        return Ok(Vec::new());
    };
    let length: usize = raw
        .trim()
        .parse()
        .map_err(|_| format!("Content-Length 不是数字：{raw}"))?;
    if length > max_bytes {
        return Err(format!(
            "请求体超限（{length} 字节 > 上限 {max_bytes} 字节）"
        ));
    }
    let mut body = head.body_prefix.clone();
    if body.len() > length {
        body.truncate(length);
    }
    let mut chunk = [0u8; 4096];
    while body.len() < length {
        let want = std::cmp::min(chunk.len(), length - body.len());
        let read = stream
            .read(&mut chunk[..want])
            .map_err(|e| format!("读取请求体失败：{e}"))?;
        if read == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..read]);
    }
    if body.len() < length {
        return Err(format!(
            "请求体不完整（收到 {} 字节，声明 {length} 字节）",
            body.len()
        ));
    }
    Ok(body)
}

/// 写流式响应头（无 Content-Length，靠连接关闭定界），随后由调用方逐行
/// 写并 flush。推理流的下发形状是 NDJSON（每行一个 JSON 事件）。
pub(crate) fn write_stream_start(
    stream: &mut TcpStream,
    content_type: &str,
) -> std::io::Result<()> {
    write!(
        stream,
        "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nConnection: close\r\n\r\n"
    )
}

/// 写一个 `Connection: close` 的响应并返回。状态码只列本组用到的。
pub(crate) fn write_response(
    stream: &mut TcpStream,
    status: u16,
    content_type: &str,
    body: &str,
) -> std::io::Result<()> {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        _ => "Error",
    };
    write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    )
}
