//! 桌面外壳各模块共用的错误类型。

/// 当某一步无法在没有诊断信息的情况下继续时，由外壳抛出。
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("GitHub 请求失败：{0}")]
    GitHub(String),
    #[error("I/O 错误：{0}")]
    Io(String),
    #[error("内核错误：{0}")]
    Kernel(String),
    #[error("插件错误：{0}")]
    Plugin(String),
    #[error("技能错误：{0}")]
    Skill(String),
    #[error("记忆错误：{0}")]
    Memory(String),
    #[error("模型用量统计错误：{0}")]
    Usage(String),
    #[error("订阅用量查询错误：{0}")]
    Subscription(String),
    #[error("桌面端更新错误：{0}")]
    Update(String),
}

/// 只按系统错误给出排查方向；拒绝访问本身不能证明杀毒软件误报。
pub(crate) fn io_error_guidance(error: &std::io::Error) -> &'static str {
    #[cfg(windows)]
    match error.raw_os_error() {
        Some(225 | 226) => {
            return "Windows 报告文件被安全防护拦截或移除，请核对防护历史或隔离区的检测名称、文件路径和处置时间；确认来源与完整性后再通过应用重新安装对应组件";
        }
        Some(32 | 33) => {
            return "文件正被其他进程占用，请等待扫描或文件操作完成后重试；若持续失败，请按上述路径检查占用进程和安全软件防护记录";
        }
        _ => {}
    }
    match error.kind() {
        std::io::ErrorKind::PermissionDenied => {
            "请检查当前用户对上述路径的访问权限，并核对安全软件防护记录；仅凭拒绝访问不能判断为误报"
        }
        std::io::ErrorKind::NotFound => {
            "请确认上述路径是否存在；若文件此前存在，请核对安全软件隔离记录，确认来源与完整性后通过应用重新安装对应组件"
        }
        _ => "请根据原始错误检查上述路径，处理后重试，仍失败时查看日志",
    }
}

impl From<std::io::Error> for AppError {
    fn from(value: std::io::Error) -> Self {
        AppError::Io(value.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Error, ErrorKind};

    #[test]
    fn access_denied_does_not_claim_an_antivirus_detection() {
        let hint = io_error_guidance(&Error::from(ErrorKind::PermissionDenied));
        assert!(hint.contains("访问权限"));
        assert!(hint.contains("防护记录"));
        assert!(hint.contains("不能判断为误报"));
    }

    #[test]
    fn missing_file_guidance_checks_the_path_before_reinstallation() {
        let hint = io_error_guidance(&Error::from(ErrorKind::NotFound));
        assert!(hint.contains("路径是否存在"));
        assert!(hint.contains("隔离记录"));
        assert!(hint.contains("确认来源与完整性"));
    }

    #[cfg(windows)]
    #[test]
    fn windows_distinguishes_file_locks_from_security_blocks() {
        for code in [32, 33] {
            assert!(io_error_guidance(&Error::from_raw_os_error(code)).contains("占用"));
        }
        for code in [225, 226] {
            let hint = io_error_guidance(&Error::from_raw_os_error(code));
            assert!(hint.contains("Windows 报告"));
            assert!(hint.contains("检测名称"));
            assert!(!hint.contains("误报"));
        }
        assert!(io_error_guidance(&Error::from_raw_os_error(5)).contains("访问权限"));
        assert!(!io_error_guidance(&Error::from_raw_os_error(193)).contains("安全防护拦截"));
    }
}
