//! 回环桥的**凭据落盘**：把地址与令牌写进每个已物化插件的根目录，让插件脚本在
//! 环境变量那条路不通时仍能拿到凭据。
//!
//! ## 为什么需要第二条路（2026-10-12 真机实测）
//!
//! 桥把地址与令牌注进**内核进程**的环境；但 `/ponytail-update` 是**模型**在工作台里
//! 用 shell 工具执行的，那条链**不继承内核的环境** —— 用户实测该脚本稳定报「找不到
//! 桌面壳的同步桥」，而同一时刻 `ps eww <内核 pid>` 里两个变量都在、桥也活着
//! （直接打 `/v1/ping` 返 200，打 `/v1/builtin/sync` 返 `{"version":"5.1.0","error":""}`）。
//!
//! **这不放宽威胁模型**：令牌原本就在环境变量里，而 macOS 上同用户本来就能用
//! `ps eww` 读到它；改成 0600 文件后仍然是同用户可读。变的只是传递通道，不是
//! 能读到它的人 —— 真要挡住同用户进程，环境变量那条路本来就挡不住。
//!
//! ## 为什么写在插件自己的物化根下
//!
//! `adapter/update.mjs` 就在那棵树下面，路径由脚本自己的位置推出（`adapter/../`），
//! **不依赖任何环境变量去猜 `DSH_HOME`** —— 而 env 正是这条路上不可靠的那一环，
//! 多穿一层就多一处会失效的假设。
//!
//! ## 为什么逐个物化目录都写
//!
//! 物化目录带内核指纹，同一个实例里可能躺着上一代指纹的产物，而插件脚本是按**自己
//! 所在那棵树**找凭据的。少写一个，在那个目录里跑的脚本就会退回「找不到桥」。
//!
//! ## 写失败不阻断启动
//!
//! 这个文件只是 env 那条路不通时的兜底，env 仍然照写。起不来一个文件不该让工作台
//! 起不来——与 [`super::bridge::launch`] 的同一条纪律。

use std::fs;
use std::path::{Path, PathBuf};

/// 物化目录里的凭据文件名（插件侧按同样的相对位置找它）。
pub(crate) const CREDENTIAL_FILE: &str = ".xlink-bridge.json";

/// 把地址与令牌写进每个已物化插件的根目录，返回写成功的那些路径。
///
/// 只有**自带 `adapter/update.mjs`** 的插件才写：给别的内置插件物化目录里多写一份
/// 同用户可读的令牌副本，没有任何用途。
pub(crate) fn write_all(dsh_home: &Path, url: &str, token: &str) -> Vec<PathBuf> {
    let base = dsh_home.join("extensions").join("builtin");
    let mut written = Vec::new();
    for entry in super::materialize::find_materialized(&base) {
        if !entry.dir.join("adapter").join("update.mjs").is_file() {
            continue;
        }
        let path = entry.dir.join(CREDENTIAL_FILE);
        if write_one(&path, url, token).is_ok() {
            written.push(path);
        }
    }
    written
}

/// 原子写一份凭据文件，并在 Unix 上把权限收紧到 0600。
///
/// 先写临时文件再 rename：崩在中间留下的半截 JSON 会让脚本 `JSON.parse` 抛错，
/// 而那时的报错是「凭据格式不对」，比真实原因更难查。
fn write_one(path: &Path, url: &str, token: &str) -> Result<(), String> {
    let body = format!(
        "{{\n  \"url\": {},\n  \"token\": {},\n  \"pid\": {}\n}}\n",
        json_string(url),
        json_string(token),
        std::process::id()
    );
    let tmp = path.with_extension("json.tmp");
    fs::write(&tmp, body).map_err(|e| format!("写桥凭据失败（{}）：{e}", tmp.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&tmp, fs::Permissions::from_mode(0o600))
            .map_err(|e| format!("收紧桥凭据权限失败（{}）：{e}", tmp.display()))?;
    }
    fs::rename(&tmp, path).map_err(|e| {
        let _ = fs::remove_file(&tmp);
        format!("落桥凭据失败（{}）：{e}", path.display())
    })
}

/// 最小 JSON 字符串转义。这里只有 URL 与十六进制令牌，但**仍然转义**：
/// 手写一个「值里不会出现引号和反斜杠」的假设，代价是一次静默的坏 JSON。
fn json_string(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 2);
    out.push('"');
    for ch in value.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("xlink-bridge-cred-{}-{}", std::process::id(), tag));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// 建一个带指定文件名的目录（`find_materialized` 靠 `host/index.js` 认根）。
    fn mkdir_with(dir: &Path, file: &str) {
        fs::create_dir_all(dir).expect("建目录");
        fs::write(dir.join(file), "//").expect("写文件");
    }

    /// JSON 转义不是装饰：URL 与令牌里出现 `\` 或引号时，不转义写出来的就是
    /// 一份坏 JSON，而脚本侧的报错是「凭据格式不对」——指向完全错误的方向。
    #[test]
    fn json_string_escapes_what_would_break_the_file() {
        assert_eq!(json_string("http://127.0.0.1:1"), "\"http://127.0.0.1:1\"");
        assert_eq!(json_string(r#"a"b\c"#), r#""a\"b\\c""#);
        assert_eq!(json_string("a\nb"), r#""a\nb""#);
        // 控制字符必须转成 6 位十六进制转义，否则落盘的就不是合法 JSON —— 脚本侧那句
        // 报错是「凭据格式不对」，指向完全错误的方向。
        assert_eq!(json_string("\u{1}"), "\"\\u0001\"");
        // 中文与十六进制令牌原样通过：它们是正常值，转义会把可读性弄没。
        assert_eq!(json_string("令牌"), "\"令牌\"");
    }

    /// 凭据文件要能被脚本的 `JSON.parse` 读出来，且 Unix 上只有本人可读。
    #[test]
    fn credential_file_is_readable_json_and_owner_only() {
        let dir = temp_dir("basic");
        let path = dir.join(CREDENTIAL_FILE);
        write_one(&path, "http://127.0.0.1:59599", "abc123").expect("写凭据");
        let raw = fs::read_to_string(&path).expect("读凭据");
        assert!(raw.contains("\"url\""), "字段名要稳，脚本按它读：{raw}");
        assert!(raw.contains("http://127.0.0.1:59599"));
        // 不留临时文件：崩在中间的那个半截文件正是「凭据格式不对」的来源。
        let leftovers: Vec<_> = fs::read_dir(&dir)
            .expect("列目录")
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().contains("tmp"))
            .collect();
        assert!(leftovers.is_empty(), "残留了临时文件：{leftovers:?}");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&path).expect("stat").permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "凭据文件必须是 0600，实际 {mode:o}");
        }
    }

    /// 只有**自带 update 脚本**的插件才该拿到凭据：给别的内置插件物化目录里
    /// 多写一份同用户可读的令牌副本，没有任何用途。
    #[test]
    fn skips_plugins_without_an_update_script() {
        let dir = temp_dir("scope");
        let ext = dir.join("extensions").join("builtin");
        let with = ext.join("ponytail").join("0.1.0").join("fp");
        mkdir_with(&with.join("host"), "index.js");
        mkdir_with(&with.join("adapter"), "update.mjs");
        let without = ext.join("other").join("0.1.0").join("fp");
        mkdir_with(&without.join("host"), "index.js");

        let written = write_all(&dir, "http://127.0.0.1:1", "t");
        assert_eq!(written, vec![with.join(CREDENTIAL_FILE)]);
        assert!(!without.join(CREDENTIAL_FILE).exists());
    }
}
