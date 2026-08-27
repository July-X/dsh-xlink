//! Error type shared across the desktop-shell modules.

/// Raised by the shell when a step cannot proceed without a diagnostic.
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
    #[error("桌面端更新错误：{0}")]
    Update(String),
}

impl From<std::io::Error> for AppError {
    fn from(value: std::io::Error) -> Self {
        AppError::Io(value.to_string())
    }
}
