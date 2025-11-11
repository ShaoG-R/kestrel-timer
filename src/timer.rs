pub mod handle;

use crate::config::{BatchConfig, ServiceConfig, WheelConfig};
use crate::task::{CallbackWrapper, TaskHandle, TaskId};
use crate::wheel::Wheel;
use handle::{BatchHandle, BatchHandleWithCompletion, TimerHandle, TimerHandleWithCompletion};
use parking_lot::Mutex;
use std::sync::Arc;
use std::time::Duration;
use tokio::task::JoinHandle;

/// Timing Wheel Timer Manager
///
/// 时间轮定时器管理器
pub struct TimerWheel {
    /// Timing wheel instance, wrapped in Arc<Mutex> for multi-threaded access
    ///
    /// 时间轮实例，包装在 Arc<Mutex> 中以支持多线程访问
    wheel: Arc<Mutex<Wheel>>,

    /// Background tick loop task handle
    ///
    /// 后台 tick 循环任务句柄
    tick_handle: Option<JoinHandle<()>>,
}

impl TimerWheel {
    /// Create a new timer manager
    ///
    /// # Parameters
    /// - `config`: Timing wheel configuration, already validated
    /// - `batch_config`: Batch operation configuration
    ///
    /// 创建新的定时器管理器
    ///
    /// # 参数
    /// - `config`: 时间轮配置，已验证
    /// - `batch_config`: 批量操作配置
    ///
    /// # Examples (示例)
    /// ```no_run
    /// use kestrel_timer::{TimerWheel, config::WheelConfig, TimerTask, config::BatchConfig};
    /// use std::time::Duration;
    ///
    /// #[tokio::main]
    /// async fn main() {
    ///     let config = WheelConfig::builder()
    ///         .l0_tick_duration(Duration::from_millis(10))
    ///         .l0_slot_count(512)
    ///         .l1_tick_duration(Duration::from_secs(1))
    ///         .l1_slot_count(64)
    ///         .build()
    ///         .unwrap();
    ///     let timer = TimerWheel::new(config, BatchConfig::default());
    ///     
    ///     // Use two-step API: allocate handle first, then register
    ///     // 使用两步 API：先分配 handle，再注册
    ///     let handle = timer.allocate_handle();
    ///     let task = TimerTask::new_oneshot(Duration::from_secs(1), None);
    ///     let _timer_handle = timer.register(handle, task);
    /// }
    /// ```
    pub fn new(config: WheelConfig, batch_config: BatchConfig) -> Self {
        let tick_duration = config.l0_tick_duration;
        let wheel = Wheel::new(config, batch_config);
        let wheel = Arc::new(Mutex::new(wheel));
        let wheel_clone = wheel.clone();

        // Start background tick loop
        // 启动后台 tick 循环
        let tick_handle = tokio::spawn(async move {
            Self::tick_loop(wheel_clone, tick_duration).await;
        });

        Self {
            wheel,
            tick_handle: Some(tick_handle),
        }
    }

    /// Create timer manager with default configuration, hierarchical mode
    /// - L0 layer tick duration: 10ms, slot count: 512
    /// - L1 layer tick duration: 1s, slot count: 64
    ///
    /// # Returns
    /// Timer manager instance
    ///
    /// 使用默认配置创建定时器管理器，分层模式
    /// - L0 层 tick 持续时间：10ms，槽数量：512
    /// - L1 层 tick 持续时间：1s，槽数量：64
    ///
    /// # 返回值
    /// 定时器管理器实例
    ///
    /// # Examples (示例)
    /// ```no_run
    /// use kestrel_timer::TimerWheel;
    ///
    /// #[tokio::main]
    /// async fn main() {
    ///     let timer = TimerWheel::with_defaults();
    /// }
    /// ```
    pub fn with_defaults() -> Self {
        Self::new(WheelConfig::default(), BatchConfig::default())
    }

    /// Create TimerService bound to this timing wheel with default configuration
    ///
    /// # Parameters
    /// - `service_config`: Service configuration
    ///
    /// # Returns
    /// TimerService instance bound to this timing wheel
    ///
    /// 创建绑定到此时间轮的 TimerService，使用默认配置
    ///
    /// # 参数
    /// - `service_config`: 服务配置
    ///
    /// # 返回值
    /// 绑定到此时间轮的 TimerService 实例
    ///
    /// # Examples (示例)
    /// ```no_run
    /// use kestrel_timer::{TimerWheel, TimerService, CallbackWrapper, config::ServiceConfig};
    /// use std::time::Duration;
    ///
    ///
    /// #[tokio::main]
    /// async fn main() {
    ///     let timer = TimerWheel::with_defaults();
    ///     let mut service = timer.create_service(ServiceConfig::default());
    ///     
    ///     // Use two-step API to batch schedule timers through service
    ///     // 使用两步 API 通过服务批量调度定时器
    ///     // Step 1: Allocate handles
    ///     let handles = service.allocate_handles(5);
    ///     
    ///     // Step 2: Create tasks
    ///     let tasks: Vec<_> = (0..5)
    ///         .map(|_| {
    ///             use kestrel_timer::TimerTask;
    ///             TimerTask::new_oneshot(Duration::from_millis(100), Some(CallbackWrapper::new(|| async {})))
    ///         })
    ///         .collect();
    ///     
    ///     // Step 3: Register batch
    ///     service.register_batch(handles, tasks).unwrap();
    ///     
    ///     // Receive timeout notifications
    ///     // 接收超时通知
    ///     let mut rx = service.take_receiver().unwrap();
    ///     while let Some(task_id) = rx.recv().await {
    ///         println!("Task {:?} completed", task_id);
    ///     }
    /// }
    /// ```
    pub fn create_service(&self, service_config: ServiceConfig) -> crate::service::TimerService {
        crate::service::TimerService::new(self.wheel.clone(), service_config)
    }

    /// Create TimerService bound to this timing wheel with custom configuration
    ///
    /// # Parameters
    /// - `config`: Service configuration
    ///
    /// # Returns
    /// TimerService instance bound to this timing wheel
    ///
    /// 创建绑定到此时间轮的 TimerService，使用自定义配置
    ///
    /// # 参数
    /// - `config`: 服务配置
    ///
    /// # 返回值
    /// 绑定到此时间轮的 TimerService 实例
    ///
    /// # Examples (示例)
    /// ```no_run
    /// use kestrel_timer::{TimerWheel, config::ServiceConfig, TimerTask};
    /// use std::num::NonZeroUsize;
    ///
    /// #[tokio::main]
    /// async fn main() {
    ///     let timer = TimerWheel::with_defaults();
    ///     let config = ServiceConfig::builder()
    ///         .command_channel_capacity(NonZeroUsize::new(1024).unwrap())
    ///         .timeout_channel_capacity(NonZeroUsize::new(2000).unwrap())
    ///         .build();
    ///     let service = timer.create_service_with_config(config);
    /// }
    /// ```
    pub fn create_service_with_config(
        &self,
        config: ServiceConfig,
    ) -> crate::service::TimerService {
        crate::service::TimerService::new(self.wheel.clone(), config)
    }

    /// Allocate a handle from DeferredMap
    ///
    /// # Returns
    /// A unique handle for later insertion
    ///
    /// # 返回值
    /// 用于后续插入的唯一 handle
    pub fn allocate_handle(&self) -> TaskHandle {
        self.wheel.lock().allocate_handle()
    }

    /// Batch allocate handles from DeferredMap
    ///
    /// # Parameters
    /// - `count`: Number of handles to allocate
    ///
    /// # Returns
    /// Vector of unique handles for later batch insertion
    ///
    /// # 参数
    /// - `count`: 要分配的 handle 数量
    ///
    /// # 返回值
    /// 用于后续批量插入的唯一 handles 向量
    pub fn allocate_handles(&self, count: usize) -> Vec<TaskHandle> {
        self.wheel.lock().allocate_handles(count)
    }

    /// Register timer task to timing wheel (registration phase)
    ///
    /// # Parameters
    /// - `task`: Task created via `create_task()`
    ///
    /// # Returns
    /// Return timer handle with completion receiver that can be used to cancel timer and receive completion notifications
    ///
    /// 注册定时器任务到时间轮 (注册阶段)
    ///
    /// # 参数
    /// - `task`: 通过 `create_task()` 创建的任务
    ///
    /// # 返回值
    /// 返回包含完成通知接收器的定时器句柄，可用于取消定时器和接收完成通知
    ///
    /// # Examples (示例)
    /// ```no_run
    /// use kestrel_timer::{TimerWheel, TimerTask, CallbackWrapper};
    ///
    /// use std::time::Duration;
    ///
    /// #[tokio::main]
    /// async fn main() {
    ///     let timer = TimerWheel::with_defaults();
    ///     
    ///     // Step 1: Allocate handle
    ///     let allocated_handle = timer.allocate_handle();
    ///     let task_id = allocated_handle.task_id();
    ///     
    ///     // Step 2: Create task
    ///     let task = TimerTask::new_oneshot(Duration::from_secs(1), Some(CallbackWrapper::new(|| async {
    ///         println!("Timer fired!");
    ///     })));
    ///     
    ///     // Step 3: Register task
    ///     let handle = timer.register(allocated_handle, task);
    ///     
    ///     // Wait for timer completion
    ///     // 等待定时器完成
    ///     use kestrel_timer::CompletionReceiver;
    ///     let (rx, _handle) = handle.into_parts();
    ///     match rx {
    ///         CompletionReceiver::OneShot(receiver) => {
    ///             receiver.recv().await.unwrap();
    ///         },
    ///         _ => {}
    ///     }
    /// }
    /// ```
    #[inline]
    pub fn register(
        &self,
        handle: TaskHandle,
        task: crate::task::TimerTask,
    ) -> TimerHandleWithCompletion {
        let task_id = handle.task_id();

        let (task, completion_rx) =
            crate::task::TimerTaskWithCompletionNotifier::from_timer_task(task);

        // Single lock to complete all operations
        // 单次加锁完成所有操作
        let mut wheel_guard = self.wheel.lock();
        wheel_guard.insert(handle, task);

        TimerHandleWithCompletion::new(TimerHandle::new(task_id, self.wheel.clone()), completion_rx)
    }

    /// Batch register timer tasks to timing wheel (registration phase)
    ///
    /// # Parameters
    /// - `handles`: Pre-allocated handles for tasks
    /// - `tasks`: List of timer tasks
    ///
    /// # Returns
    /// - `Ok(BatchHandleWithCompletion)` if all tasks are successfully registered
    /// - `Err(TimerError::BatchLengthMismatch)` if handles and tasks lengths don't match
    ///
    /// 批量注册定时器任务到时间轮 (注册阶段)
    ///
    /// # 参数
    /// - `handles`: 任务的预分配 handles
    /// - `tasks`: 定时器任务列表
    ///
    /// # 返回值
    /// - `Ok(BatchHandleWithCompletion)` 如果所有任务成功注册
    /// - `Err(TimerError::BatchLengthMismatch)` 如果 handles 和 tasks 长度不匹配
    ///
    /// # Examples (示例)
    /// ```no_run
    /// use kestrel_timer::{TimerWheel, TimerTask};
    /// use std::time::Duration;
    ///
    /// #[tokio::main]
    /// async fn main() {
    ///     let timer = TimerWheel::with_defaults();
    ///     
    ///     // Step 1: Allocate handles
    ///     let handles = timer.allocate_handles(3);
    ///     
    ///     // Step 2: Create tasks
    ///     let tasks: Vec<_> = (0..3)
    ///         .map(|_| TimerTask::new_oneshot(Duration::from_secs(1), None))
    ///         .collect();
    ///     
    ///     // Step 3: Batch register
    ///     let batch = timer.register_batch(handles, tasks)
    ///         .expect("register_batch should succeed");
    ///     println!("Registered {} timers", batch.len());
    /// }
    /// ```
    #[inline]
    pub fn register_batch(
        &self,
        handles: Vec<TaskHandle>,
        tasks: Vec<crate::task::TimerTask>,
    ) -> Result<BatchHandleWithCompletion, crate::error::TimerError> {
        // Validate lengths match
        if handles.len() != tasks.len() {
            return Err(crate::error::TimerError::BatchLengthMismatch {
                handles_len: handles.len(),
                tasks_len: tasks.len(),
            });
        }

        let task_count = tasks.len();
        let mut completion_rxs = Vec::with_capacity(task_count);
        let mut task_ids = Vec::with_capacity(task_count);
        let mut prepared_handles = Vec::with_capacity(task_count);
        let mut prepared_tasks = Vec::with_capacity(task_count);

        // Step 1: Prepare all channels and notifiers
        for (handle, task) in handles.into_iter().zip(tasks.into_iter()) {
            let task_id = handle.task_id();
            let (task, completion_rx) =
                crate::task::TimerTaskWithCompletionNotifier::from_timer_task(task);
            task_ids.push(task_id);
            completion_rxs.push(completion_rx);
            prepared_handles.push(handle);
            prepared_tasks.push(task);
        }

        // Step 2: Single lock, batch insert
        {
            let mut wheel_guard = self.wheel.lock();
            wheel_guard.insert_batch(prepared_handles, prepared_tasks)?;
        }

        Ok(BatchHandleWithCompletion::new(
            BatchHandle::new(task_ids, self.wheel.clone()),
            completion_rxs,
        ))
    }

    /// Cancel timer
    ///
    /// # Parameters
    /// - `task_id`: Task ID
    ///
    /// # Returns
    /// Returns true if task exists and is successfully cancelled, otherwise false
    ///
    /// 取消定时器
    ///
    /// # 参数
    /// - `task_id`: 任务 ID
    ///
    /// # 返回值
    /// 如果任务存在且成功取消则返回 true，否则返回 false
    ///
    /// # Examples (示例)
    /// ```no_run
    /// use kestrel_timer::{TimerWheel, TimerTask, CallbackWrapper};
    ///
    /// use std::time::Duration;
    ///
    /// #[tokio::main]
    /// async fn main() {
    ///     let timer = TimerWheel::with_defaults();
    ///     
    ///     // Step 1: Allocate handle
    ///     let allocated_handle = timer.allocate_handle();
    ///     let task_id = allocated_handle.task_id();
    ///     
    ///     // Step 2: Create and register task
    ///     let task = TimerTask::new_oneshot(Duration::from_secs(10), Some(CallbackWrapper::new(|| async {
    ///         println!("Timer fired!");
    ///     })));
    ///     let _handle = timer.register(allocated_handle, task);
    ///     
    ///     // Cancel task using task ID
    ///     // 使用任务 ID 取消任务
    ///     let cancelled = timer.cancel(task_id);
    ///     println!("Canceled successfully: {}", cancelled);
    /// }
    /// ```
    #[inline]
    pub fn cancel(&self, task_id: TaskId) -> bool {
        let mut wheel = self.wheel.lock();
        wheel.cancel(task_id)
    }

    /// Batch cancel timers
    ///
    /// # Parameters
    /// - `task_ids`: List of task IDs to cancel
    ///
    /// # Returns
    /// Number of successfully cancelled tasks
    ///
    /// 批量取消定时器
    ///
    /// # 参数
    /// - `task_ids`: 要取消的任务 ID 列表
    ///
    /// # 返回值
    /// 成功取消的任务数量
    ///
    /// # Performance Advantages
    /// - Batch processing reduces lock contention
    /// - Internally optimized batch cancellation operation
    ///
    /// # Examples (示例)
    /// ```no_run
    /// use kestrel_timer::{TimerWheel, TimerTask};
    /// use std::time::Duration;
    ///
    /// #[tokio::main]
    /// async fn main() {
    ///     let timer = TimerWheel::with_defaults();
    ///     
    ///     // Create multiple timers
    ///     // 创建多个定时器
    ///     let task1 = TimerTask::new_oneshot(Duration::from_secs(10), None);
    ///     let task2 = TimerTask::new_oneshot(Duration::from_secs(10), None);
    ///     let task3 = TimerTask::new_oneshot(Duration::from_secs(10), None);
    ///     
    ///     // Allocate handles and get task IDs
    ///     let h1 = timer.allocate_handle();
    ///     let h2 = timer.allocate_handle();
    ///     let h3 = timer.allocate_handle();
    ///     let task_ids = vec![h1.task_id(), h2.task_id(), h3.task_id()];
    ///     
    ///     let _h1 = timer.register(h1, task1);
    ///     let _h2 = timer.register(h2, task2);
    ///     let _h3 = timer.register(h3, task3);
    ///     
    ///     // Batch cancel
    ///     // 批量取消
    ///     let cancelled = timer.cancel_batch(&task_ids);
    ///     println!("Canceled {} timers", cancelled);
    /// }
    /// ```
    #[inline]
    pub fn cancel_batch(&self, task_ids: &[TaskId]) -> usize {
        let mut wheel = self.wheel.lock();
        wheel.cancel_batch(task_ids)
    }

    /// Postpone timer
    ///
    /// # Parameters
    /// - `task_id`: Task ID to postpone
    /// - `new_delay`: New delay duration, recalculated from current time
    /// - `callback`: New callback function, pass `None` to keep original callback, pass `Some` to replace with new callback
    ///
    /// # Returns
    /// Returns true if task exists and is successfully postponed, otherwise false
    ///
    /// 推迟定时器
    ///
    /// # 参数
    /// - `task_id`: 要推迟的任务 ID
    /// - `new_delay`: 新的延迟时间，从当前时间重新计算
    /// - `callback`: 新的回调函数，传递 `None` 保持原始回调，传递 `Some` 替换为新的回调
    ///
    /// # 返回值
    /// 如果任务存在且成功推迟则返回 true，否则返回 false
    ///
    /// # Note
    /// - Task ID remains unchanged after postponement
    /// - Original completion_receiver remains valid
    ///
    /// # 注意
    /// - 任务 ID 在推迟后保持不变
    /// - 原始 completion_receiver 保持有效
    ///
    /// # Examples (示例)
    ///
    /// ## Keep original callback (保持原始回调)
    /// ```no_run
    /// use kestrel_timer::{TimerWheel, TimerTask, CallbackWrapper};
    /// use std::time::Duration;
    ///
    ///
    /// #[tokio::main]
    /// async fn main() {
    ///     let timer = TimerWheel::with_defaults();
    ///     
    ///     // Allocate handle first
    ///     let allocated_handle = timer.allocate_handle();
    ///     let task_id = allocated_handle.task_id();
    ///     
    ///     let task = TimerTask::new_oneshot(Duration::from_secs(5), Some(CallbackWrapper::new(|| async {
    ///         println!("Timer fired!");
    ///     })));
    ///     let _handle = timer.register(allocated_handle, task);
    ///     
    ///     // Postpone to 10 seconds after triggering, and keep original callback
    ///     // 推迟到 10 秒后触发，并保持原始回调
    ///     let success = timer.postpone(task_id, Duration::from_secs(10), None);
    ///     println!("Postponed successfully: {}", success);
    /// }
    /// ```
    ///
    /// ## Replace with new callback (替换为新的回调)
    /// ```no_run
    /// use kestrel_timer::{TimerWheel, TimerTask, CallbackWrapper};
    /// use std::time::Duration;
    ///
    /// #[tokio::main]
    /// async fn main() {
    ///     let timer = TimerWheel::with_defaults();
    ///     
    ///     // Allocate handle first
    ///     let allocated_handle = timer.allocate_handle();
    ///     let task_id = allocated_handle.task_id();
    ///     
    ///     let task = TimerTask::new_oneshot(Duration::from_secs(5), Some(CallbackWrapper::new(|| async {
    ///         println!("Original callback!");
    ///     })));
    ///     let _handle = timer.register(allocated_handle, task);
    ///     
    ///     // Postpone to 10 seconds after triggering, and replace with new callback
    ///     // 推迟到 10 秒后触发，并替换为新的回调
    ///     let success = timer.postpone(task_id, Duration::from_secs(10), Some(CallbackWrapper::new(|| async {
    ///         println!("New callback!");
    ///     })));
    ///     println!("Postponed successfully: {}", success);
    /// }
    /// ```
    #[inline]
    pub fn postpone(
        &self,
        task_id: TaskId,
        new_delay: Duration,
        callback: Option<CallbackWrapper>,
    ) -> bool {
        let mut wheel = self.wheel.lock();
        wheel.postpone(task_id, new_delay, callback)
    }

    /// Batch postpone timers (keep original callbacks)
    ///
    /// # Parameters
    /// - `updates`: List of tuples of (task ID, new delay)
    ///
    /// # Returns
    /// Number of successfully postponed tasks
    ///
    /// 批量推迟定时器 (保持原始回调)
    ///
    /// # 参数
    /// - `updates`: (任务 ID, 新延迟) 元组列表
    ///
    /// # 返回值
    /// 成功推迟的任务数量
    ///
    /// # Note
    /// - This method keeps all tasks' original callbacks unchanged
    /// - Use `postpone_batch_with_callbacks` if you need to replace callbacks
    ///
    /// # 注意
    /// - 此方法保持所有任务的原始回调不变
    /// - 如果需要替换回调，请使用 `postpone_batch_with_callbacks`
    ///
    /// # Performance Advantages
    /// - Batch processing reduces lock contention
    /// - Internally optimized batch postponement operation
    ///
    /// # Examples (示例)
    /// ```no_run
    /// use kestrel_timer::{TimerWheel, TimerTask, CallbackWrapper};
    /// use std::time::Duration;
    ///
    /// #[tokio::main]
    /// async fn main() {
    ///     let timer = TimerWheel::with_defaults();
    ///     
    ///     // Create multiple tasks with callbacks
    ///     // 创建多个带有回调的任务
    ///     let task1 = TimerTask::new_oneshot(Duration::from_secs(5), Some(CallbackWrapper::new(|| async {
    ///         println!("Task 1 fired!");
    ///     })));
    ///     let task2 = TimerTask::new_oneshot(Duration::from_secs(5), Some(CallbackWrapper::new(|| async {
    ///         println!("Task 2 fired!");
    ///     })));
    ///     let task3 = TimerTask::new_oneshot(Duration::from_secs(5), Some(CallbackWrapper::new(|| async {
    ///         println!("Task 3 fired!");
    ///     })));
    ///     
    ///     // Allocate handles and register
    ///     let h1 = timer.allocate_handle();
    ///     let h2 = timer.allocate_handle();
    ///     let h3 = timer.allocate_handle();
    ///     
    ///     let task_ids = vec![
    ///         (h1.task_id(), Duration::from_secs(10)),
    ///         (h2.task_id(), Duration::from_secs(15)),
    ///         (h3.task_id(), Duration::from_secs(20)),
    ///     ];
    ///     
    ///     timer.register(h1, task1);
    ///     timer.register(h2, task2);
    ///     timer.register(h3, task3);
    ///     
    ///     // Batch postpone (keep original callbacks)
    ///     // 批量推迟 (保持原始回调)
    ///     let postponed = timer.postpone_batch(task_ids);
    ///     println!("Postponed {} timers", postponed);
    /// }
    /// ```
    #[inline]
    pub fn postpone_batch(&self, updates: Vec<(TaskId, Duration)>) -> usize {
        let mut wheel = self.wheel.lock();
        wheel.postpone_batch(updates)
    }

    /// Batch postpone timers (replace callbacks)
    ///
    /// # Parameters
    /// - `updates`: List of tuples of (task ID, new delay, new callback)
    ///
    /// # Returns
    /// Number of successfully postponed tasks
    ///
    /// 批量推迟定时器 (替换回调)
    ///
    /// # 参数
    /// - `updates`: (任务 ID, 新延迟, 新回调) 元组列表
    ///
    /// # 返回值
    /// 成功推迟的任务数量
    ///
    /// # Performance Advantages
    /// - Batch processing reduces lock contention
    /// - Internally optimized batch postponement operation
    ///
    /// # Examples (示例)
    /// ```no_run
    /// use kestrel_timer::{TimerWheel, TimerTask, CallbackWrapper};
    /// use std::time::Duration;
    /// use std::sync::Arc;
    /// use std::sync::atomic::{AtomicU32, Ordering};
    ///
    /// #[tokio::main]
    /// async fn main() {
    ///     let timer = TimerWheel::with_defaults();
    ///     let counter = Arc::new(AtomicU32::new(0));
    ///     
    ///     // Create multiple timers
    ///     // 创建多个定时器
    ///     let task1 = TimerTask::new_oneshot(Duration::from_secs(5), None);
    ///     let task2 = TimerTask::new_oneshot(Duration::from_secs(5), None);
    ///     
    ///     // Allocate handles first
    ///     let h1 = timer.allocate_handle();
    ///     let h2 = timer.allocate_handle();
    ///     let id1 = h1.task_id();
    ///     let id2 = h2.task_id();
    ///     
    ///     timer.register(h1, task1);
    ///     timer.register(h2, task2);
    ///     
    ///     // Batch postpone and replace callbacks
    ///     // 批量推迟并替换回调
    ///     let updates: Vec<_> = vec![id1, id2]
    ///         .into_iter()
    ///         .map(|id| {
    ///             let counter = Arc::clone(&counter);
    ///             (id, Duration::from_secs(10), Some(CallbackWrapper::new(move || {
    ///                 let counter = Arc::clone(&counter);
    ///                 async move { counter.fetch_add(1, Ordering::SeqCst); }
    ///             })))
    ///         })
    ///         .collect();
    ///     let postponed = timer.postpone_batch_with_callbacks(updates);
    ///     println!("Postponed {} timers", postponed);
    /// }
    /// ```
    #[inline]
    pub fn postpone_batch_with_callbacks(
        &self,
        updates: Vec<(TaskId, Duration, Option<CallbackWrapper>)>,
    ) -> usize {
        let mut wheel = self.wheel.lock();
        wheel.postpone_batch_with_callbacks(updates.to_vec())
    }

    /// Core tick loop
    ///
    /// Background task that advances the timing wheel at regular intervals
    ///
    /// # Parameters
    /// - `wheel`: Shared timing wheel instance
    /// - `tick_duration`: Duration between ticks
    ///
    /// 核心 tick 循环
    ///
    /// 定期推进时间轮的后台任务
    ///
    /// # 参数
    /// - `wheel`: 共享的时间轮实例
    /// - `tick_duration`: tick 之间的持续时间
    async fn tick_loop(wheel: Arc<Mutex<Wheel>>, tick_duration: Duration) {
        let mut interval = tokio::time::interval(tick_duration);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            interval.tick().await;

            // Advance timing wheel and get expired tasks
            // Note: wheel.advance() already handles completion notifications
            let expired_tasks = {
                let mut wheel_guard = wheel.lock();
                wheel_guard.advance()
            };

            // Execute callbacks for expired tasks
            // Notifications have already been sent by wheel.advance()
            for task in expired_tasks {
                if let Some(callback) = task.callback {
                    // Spawn callback execution in a separate tokio task
                    tokio::spawn(async move {
                        let future = callback.call();
                        future.await;
                    });
                }
            }
        }
    }

    /// Graceful shutdown of TimerWheel
    ///
    /// 优雅关闭 TimerWheel
    ///
    /// # Examples (示例)
    /// ```no_run
    /// # use kestrel_timer::TimerWheel;
    /// # #[tokio::main]
    /// # async fn main() {
    /// let timer = TimerWheel::with_defaults();
    ///
    /// // Use timer... (使用定时器...)
    ///
    /// timer.shutdown().await;
    /// # }
    /// ```
    pub async fn shutdown(mut self) {
        if let Some(handle) = self.tick_handle.take() {
            handle.abort();
            let _ = handle.await;
        }
    }
}

/// Automatically abort the background tick task when TimerWheel is dropped
///
/// 当 TimerWheel 被销毁时自动中止后台 tick 任务
impl Drop for TimerWheel {
    fn drop(&mut self) {
        if let Some(handle) = self.tick_handle.take() {
            handle.abort();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task::TimerTask;
    use std::sync::atomic::{AtomicU32, Ordering};

    #[tokio::test]
    async fn test_timer_creation() {
        let _timer = TimerWheel::with_defaults();
    }

    #[tokio::test]
    async fn test_schedule_once() {
        use std::sync::Arc;
        let timer = TimerWheel::with_defaults();
        let counter = Arc::new(AtomicU32::new(0));
        let counter_clone = Arc::clone(&counter);

        let task = TimerTask::new_oneshot(
            Duration::from_millis(50),
            Some(CallbackWrapper::new(move || {
                let counter = Arc::clone(&counter_clone);
                async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                }
            })),
        );
        let allocate_handle = timer.allocate_handle();
        let _handle = timer.register(allocate_handle, task);

        // Wait for timer to trigger
        // 等待定时器触发
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }
}
