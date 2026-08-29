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
    #[error("桌面端更新错误：{0}")]
    Update(String),
}

impl From<std::io::Error> for AppError {
    fn from(value: std::io::Error) -> Self {
        AppError::Io(value.to_string())
    }
}
