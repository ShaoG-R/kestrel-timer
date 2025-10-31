use std::fmt;

/// 定时器错误类型 (Timer Error Type)
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TimerError {
    /// 槽位数量无效（必须是 2 的幂次方且大于 0）
    /// Invalid slot count (must be a power of 2 and greater than 0)
    InvalidSlotCount { 
        slot_count: usize,
        reason: &'static str,
    },
    
    /// 配置验证失败 (Configuration validation failed)
    InvalidConfiguration {
        field: String,
        reason: String,
    },
    
    /// 注册失败（内部通道已满或已关闭）
    /// Registration failed (internal channel is full or closed)
    RegisterFailed,
}

impl fmt::Display for TimerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TimerError::InvalidSlotCount { slot_count, reason } => {
                write!(f, "Invalid slot count {}: {}", slot_count, reason)
            }
            TimerError::InvalidConfiguration { field, reason } => {
                write!(f, "Configuration validation failed ({}): {}", field, reason)
            }
            TimerError::RegisterFailed => {
                write!(f, "Registration failed: internal channel is full or closed")
            }
        }
    }
}

impl std::error::Error for TimerError {}

