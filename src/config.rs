//! 定时器配置模块 (Timer Configuration Module)
//!
//! 提供分层的配置结构和 Builder 模式，用于配置时间轮、服务和批处理行为。
//! (Provides hierarchical configuration structure and Builder pattern for configuring timing wheel, service, and batch processing behavior)

use crate::error::TimerError;
use std::time::Duration;

/// 时间轮配置 (Timing Wheel Configuration)
///
/// 用于配置分层时间轮的参数。系统仅支持分层模式。
/// (Configuration parameters for hierarchical timing wheel. The system only supports hierarchical mode)
///
/// # 示例 (Examples)
/// ```no_run
/// use kestrel_timer::WheelConfig;
/// use std::time::Duration;
///
/// // 使用默认配置（分层模式）
/// //    (Use default configuration, hierarchical mode)
/// let config = WheelConfig::default();
///
/// // 使用 Builder 自定义配置
/// //    (Use Builder to customize configuration)
/// let config = WheelConfig::builder()
///     .l0_tick_duration(Duration::from_millis(20))
///     .l0_slot_count(1024)
///     .l1_tick_duration(Duration::from_secs(2))
///     .l1_slot_count(128)
///     .build()
///     .unwrap();
/// ```
#[derive(Debug, Clone)]
pub struct WheelConfig {
    /// L0 层（底层）每个 tick 的时间长度
    /// (Duration of each tick in L0 layer, bottom layer)
    pub l0_tick_duration: Duration,
    /// L0 层槽位数量（必须是 2 的幂次方）
    /// (Number of slots in L0 layer, must be power of 2)
    pub l0_slot_count: usize,
    
    /// L1 层（高层）每个 tick 的时间长度
    /// (Duration of each tick in L1 layer, upper layer)
    pub l1_tick_duration: Duration,
    /// L1 层槽位数量（必须是 2 的幂次方）
    /// (Number of slots in L1 layer, must be power of 2)
    pub l1_slot_count: usize,
}

impl Default for WheelConfig {
    fn default() -> Self {
        Self {
            l0_tick_duration: Duration::from_millis(10),
            l0_slot_count: 512,
            l1_tick_duration: Duration::from_secs(1),
            l1_slot_count: 64,
        }
    }
}

impl WheelConfig {
    /// 创建配置构建器 (Create configuration builder)
    pub fn builder() -> WheelConfigBuilder {
        WheelConfigBuilder::default()
    }
}

/// 时间轮配置构建器 (Timing Wheel Configuration Builder)
#[derive(Debug, Clone)]
pub struct WheelConfigBuilder {
    l0_tick_duration: Duration,
    l0_slot_count: usize,
    l1_tick_duration: Duration,
    l1_slot_count: usize,
}

impl Default for WheelConfigBuilder {
    fn default() -> Self {
        Self {
            l0_tick_duration: Duration::from_millis(10),
            l0_slot_count: 512,
            l1_tick_duration: Duration::from_secs(1),
            l1_slot_count: 64,
        }
    }
}

impl WheelConfigBuilder {
    /// 设置 L0 层 tick 时长 (Set L0 layer tick duration)
    pub fn l0_tick_duration(mut self, duration: Duration) -> Self {
        self.l0_tick_duration = duration;
        self
    }

    /// 设置 L0 层槽位数量 (Set L0 layer slot count)
    pub fn l0_slot_count(mut self, count: usize) -> Self {
        self.l0_slot_count = count;
        self
    }

    /// 设置 L1 层 tick 时长 (Set L1 layer tick duration)
    pub fn l1_tick_duration(mut self, duration: Duration) -> Self {
        self.l1_tick_duration = duration;
        self
    }

    /// 设置 L1 层槽位数量 (Set L1 layer slot count)
    pub fn l1_slot_count(mut self, count: usize) -> Self {
        self.l1_slot_count = count;
        self
    }

    /// 构建配置并进行验证
    ///      (Build and validate configuration)
    ///
    /// # 返回 (Returns)
    /// - `Ok(WheelConfig)`: 配置有效
    ///      (Configuration is valid)
    /// - `Err(TimerError)`: 配置验证失败
    ///      (Configuration validation failed)
    ///
    /// # 验证规则 (Validation Rules)
    /// - L0 tick 时长必须大于 0 
    ///      (L0 tick duration must be greater than 0)
    /// - L1 tick 时长必须大于 0 
    ///      (L1 tick duration must be greater than 0)
    /// - L0 槽位数量必须大于 0 且是 2 的幂次方
    ///      (L0 slot count must be greater than 0 and power of 2)
    /// - L1 槽位数量必须大于 0 且是 2 的幂次方
    ///      (L1 slot count must be greater than 0 and power of 2)
    /// - L1 tick 必须是 L0 tick 的整数倍
    ///      (L1 tick must be an integer multiple of L0 tick)
    pub fn build(self) -> Result<WheelConfig, TimerError> {
        // 验证 L0 层配置
        if self.l0_tick_duration.is_zero() {
            return Err(TimerError::InvalidConfiguration {
                field: "l0_tick_duration".to_string(),
                reason: "L0 层 tick 时长必须大于 0".to_string(),
            });
        }

        if self.l0_slot_count == 0 {
            return Err(TimerError::InvalidSlotCount {
                slot_count: self.l0_slot_count,
                reason: "L0 层槽位数量必须大于 0",
            });
        }

        if !self.l0_slot_count.is_power_of_two() {
            return Err(TimerError::InvalidSlotCount {
                slot_count: self.l0_slot_count,
                reason: "L0 层槽位数量必须是 2 的幂次方",
            });
        }

        // 验证 L1 层配置
        if self.l1_tick_duration.is_zero() {
            return Err(TimerError::InvalidConfiguration {
                field: "l1_tick_duration".to_string(),
                reason: "L1 层 tick 时长必须大于 0".to_string(),
            });
        }

        if self.l1_slot_count == 0 {
            return Err(TimerError::InvalidSlotCount {
                slot_count: self.l1_slot_count,
                reason: "L1 层槽位数量必须大于 0",
            });
        }

        if !self.l1_slot_count.is_power_of_two() {
            return Err(TimerError::InvalidSlotCount {
                slot_count: self.l1_slot_count,
                reason: "L1 层槽位数量必须是 2 的幂次方",
            });
        }

        // 验证 L1 tick 是 L0 tick 的整数倍
        let l0_ms = self.l0_tick_duration.as_millis() as u64;
        let l1_ms = self.l1_tick_duration.as_millis() as u64;
        if l1_ms % l0_ms != 0 {
            return Err(TimerError::InvalidConfiguration {
                field: "l1_tick_duration".to_string(),
                reason: format!(
                    "L1 tick 时长 ({} ms) 必须是 L0 tick 时长 ({} ms) 的整数倍",
                    l1_ms, l0_ms
                ),
            });
        }

        Ok(WheelConfig {
            l0_tick_duration: self.l0_tick_duration,
            l0_slot_count: self.l0_slot_count,
            l1_tick_duration: self.l1_tick_duration,
            l1_slot_count: self.l1_slot_count,
        })
    }
}

/// 服务配置 (Service Configuration)
///
/// 用于配置 TimerService 的通道容量。
/// (Configuration for TimerService channel capacities)
///
/// # 示例 (Examples)
/// ```no_run
/// use kestrel_timer::ServiceConfig;
///
/// // 使用默认配置 (Use default configuration)
/// let config = ServiceConfig::default();
///
/// // 使用 Builder 自定义配置 (Use Builder to customize configuration)
/// let config = ServiceConfig::builder()
///     .command_channel_capacity(1024)
///     .timeout_channel_capacity(2000)
///     .build()
///     .unwrap();
/// ```
#[derive(Debug, Clone)]
pub struct ServiceConfig {
    /// 命令通道容量 (Command channel capacity)
    pub command_channel_capacity: usize,
    /// 超时通道容量 (Timeout channel capacity)
    pub timeout_channel_capacity: usize,
}

impl Default for ServiceConfig {
    fn default() -> Self {
        Self {
            command_channel_capacity: 512,
            timeout_channel_capacity: 1000,
        }
    }
}

impl ServiceConfig {
    /// 创建配置构建器 (Create configuration builder)
    pub fn builder() -> ServiceConfigBuilder {
        ServiceConfigBuilder::default()
    }
}

/// 服务配置构建器 (Service Configuration Builder)
#[derive(Debug, Clone)]
pub struct ServiceConfigBuilder {
    command_channel_capacity: usize,
    timeout_channel_capacity: usize,
}

impl Default for ServiceConfigBuilder {
    fn default() -> Self {
        let config = ServiceConfig::default();
        Self {
            command_channel_capacity: config.command_channel_capacity,
            timeout_channel_capacity: config.timeout_channel_capacity,
        }
    }
}

impl ServiceConfigBuilder {
    /// 设置命令通道容量 (Set command channel capacity)
    pub fn command_channel_capacity(mut self, capacity: usize) -> Self {
        self.command_channel_capacity = capacity;
        self
    }

    /// 设置超时通道容量 (Set timeout channel capacity)
    pub fn timeout_channel_capacity(mut self, capacity: usize) -> Self {
        self.timeout_channel_capacity = capacity;
        self
    }

    /// 构建配置并进行验证
    ///      (Build and validate configuration)
    ///
    /// # 返回 (Returns)
    /// - `Ok(ServiceConfig)`: 配置有效
    ///      (Configuration is valid)
    /// - `Err(TimerError)`: 配置验证失败
    ///      (Configuration validation failed)
    ///
    /// # 验证规则 (Validation Rules)
    /// - 所有通道容量必须大于 0
    ///      (All channel capacities must be greater than 0)
    pub fn build(self) -> Result<ServiceConfig, TimerError> {
        if self.command_channel_capacity == 0 {
            return Err(TimerError::InvalidConfiguration {
                field: "command_channel_capacity".to_string(),
                reason: "命令通道容量必须大于 0".to_string(),
            });
        }

        if self.timeout_channel_capacity == 0 {
            return Err(TimerError::InvalidConfiguration {
                field: "timeout_channel_capacity".to_string(),
                reason: "超时通道容量必须大于 0".to_string(),
            });
        }

        Ok(ServiceConfig {
            command_channel_capacity: self.command_channel_capacity,
            timeout_channel_capacity: self.timeout_channel_capacity,
        })
    }
}

/// 批处理配置 (Batch Processing Configuration)
///
/// 用于配置批量操作的优化参数。
/// (Configuration parameters for batch operation optimization)
///
/// # 示例 (Examples)
/// ```no_run
/// use kestrel_timer::BatchConfig;
///
/// // 使用默认配置 (Use default configuration)
/// let config = BatchConfig::default();
///
/// // 自定义配置 (Custom configuration)
/// let config = BatchConfig {
///     small_batch_threshold: 20,
/// };
/// ```
#[derive(Debug, Clone)]
pub struct BatchConfig {
    /// 小批量阈值，用于批量取消优化
    /// (Small batch threshold for batch cancellation optimization)
    /// 
    /// 当批量取消的任务数量小于等于此值时，直接逐个取消而不进行分组排序
    /// (When batch cancellation count is less than or equal to this value, cancel individually without grouping and sorting)
    pub small_batch_threshold: usize,
}

impl Default for BatchConfig {
    fn default() -> Self {
        Self {
            small_batch_threshold: 10,
        }
    }
}

/// 顶层定时器配置 (Top-level Timer Configuration)
///
/// 组合所有子配置，提供完整的定时器系统配置。
/// (Combines all sub-configurations to provide complete timer system configuration)
///
/// # 示例 (Examples)
/// ```no_run
/// use kestrel_timer::TimerConfig;
///
/// // 使用默认配置 (Use default configuration)
/// let config = TimerConfig::default();
///
/// // 使用 Builder 自定义配置（仅配置服务参数）(Use Builder to customize configuration, service parameters only)
/// let config = TimerConfig::builder()
///     .command_channel_capacity(1024)
///     .timeout_channel_capacity(2000)
///     .build()
///     .unwrap();
/// ```
#[derive(Debug, Clone)]
pub struct TimerConfig {
    /// 时间轮配置 (Timing wheel configuration)
    pub wheel: WheelConfig,
    /// 服务配置 (Service configuration)
    pub service: ServiceConfig,
    /// 批处理配置 (Batch processing configuration)
    pub batch: BatchConfig,
}

impl Default for TimerConfig {
    fn default() -> Self {
        Self {
            wheel: WheelConfig::default(),
            service: ServiceConfig::default(),
            batch: BatchConfig::default(),
        }
    }
}

impl TimerConfig {
    /// 创建配置构建器 (Create configuration builder)
    pub fn builder() -> TimerConfigBuilder {
        TimerConfigBuilder::default()
    }
}

/// 顶层定时器配置构建器 (Top-level Timer Configuration Builder)
#[derive(Debug)]
pub struct TimerConfigBuilder {
    wheel_builder: WheelConfigBuilder,
    service_builder: ServiceConfigBuilder,
    batch_config: BatchConfig,
}

impl Default for TimerConfigBuilder {
    fn default() -> Self {
        Self {
            wheel_builder: WheelConfigBuilder::default(),
            service_builder: ServiceConfigBuilder::default(),
            batch_config: BatchConfig::default(),
        }
    }
}

impl TimerConfigBuilder {
    /// 设置命令通道容量 (Set command channel capacity)
    pub fn command_channel_capacity(mut self, capacity: usize) -> Self {
        self.service_builder = self.service_builder.command_channel_capacity(capacity);
        self
    }

    /// 设置超时通道容量 (Set timeout channel capacity)
    pub fn timeout_channel_capacity(mut self, capacity: usize) -> Self {
        self.service_builder = self.service_builder.timeout_channel_capacity(capacity);
        self
    }

    /// 设置小批量阈值 (Set small batch threshold)
    pub fn small_batch_threshold(mut self, threshold: usize) -> Self {
        self.batch_config.small_batch_threshold = threshold;
        self
    }

    /// 构建配置并进行验证
    ///      (Build and validate configuration)
    ///
    /// # 返回 (Returns)
    /// - `Ok(TimerConfig)`: 配置有效
    ///      (Configuration is valid)
    /// - `Err(TimerError)`: 配置验证失败
    ///      (Configuration validation failed)
    pub fn build(self) -> Result<TimerConfig, TimerError> {
        Ok(TimerConfig {
            wheel: self.wheel_builder.build()?,
            service: self.service_builder.build()?,
            batch: self.batch_config,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wheel_config_default() {
        let config = WheelConfig::default();
        assert_eq!(config.l0_tick_duration, Duration::from_millis(10));
        assert_eq!(config.l0_slot_count, 512);
        assert_eq!(config.l1_tick_duration, Duration::from_secs(1));
        assert_eq!(config.l1_slot_count, 64);
    }

    #[test]
    fn test_wheel_config_builder() {
        let config = WheelConfig::builder()
            .l0_tick_duration(Duration::from_millis(20))
            .l0_slot_count(1024)
            .l1_tick_duration(Duration::from_secs(2))
            .l1_slot_count(128)
            .build()
            .unwrap();

        assert_eq!(config.l0_tick_duration, Duration::from_millis(20));
        assert_eq!(config.l0_slot_count, 1024);
        assert_eq!(config.l1_tick_duration, Duration::from_secs(2));
        assert_eq!(config.l1_slot_count, 128);
    }

    #[test]
    fn test_wheel_config_validation_zero_tick() {
        let result = WheelConfig::builder()
            .l0_tick_duration(Duration::ZERO)
            .build();

        assert!(result.is_err());
    }

    #[test]
    fn test_wheel_config_validation_invalid_slot_count() {
        let result = WheelConfig::builder()
            .l0_slot_count(100)
            .build();

        assert!(result.is_err());
    }

    #[test]
    fn test_service_config_default() {
        let config = ServiceConfig::default();
        assert_eq!(config.command_channel_capacity, 512);
        assert_eq!(config.timeout_channel_capacity, 1000);
    }

    #[test]
    fn test_service_config_builder() {
        let config = ServiceConfig::builder()
            .command_channel_capacity(1024)
            .timeout_channel_capacity(2000)
            .build()
            .unwrap();

        assert_eq!(config.command_channel_capacity, 1024);
        assert_eq!(config.timeout_channel_capacity, 2000);
    }

    #[test]
    fn test_batch_config_default() {
        let config = BatchConfig::default();
        assert_eq!(config.small_batch_threshold, 10);
    }

    #[test]
    fn test_timer_config_default() {
        let config = TimerConfig::default();
        assert_eq!(config.wheel.l0_slot_count, 512);
        assert_eq!(config.service.command_channel_capacity, 512);
        assert_eq!(config.batch.small_batch_threshold, 10);
    }

    #[test]
    fn test_timer_config_builder() {
        let config = TimerConfig::builder()
            .command_channel_capacity(1024)
            .timeout_channel_capacity(2000)
            .small_batch_threshold(20)
            .build()
            .unwrap();

        assert_eq!(config.service.command_channel_capacity, 1024);
        assert_eq!(config.service.timeout_channel_capacity, 2000);
        assert_eq!(config.batch.small_batch_threshold, 20);
    }
}

