use crate::config::ServiceConfig;
use crate::error::TimerError;
use crate::task::{CallbackWrapper, CompletionReceiver, TaskCompletion, TaskId};
use crate::wheel::Wheel;
use crate::{BatchHandle, TimerHandle};
use futures::future::BoxFuture;
use futures::stream::{FuturesUnordered, StreamExt};
use lite_sync::{
    oneshot::lite::{Receiver, Sender, channel},
    spsc,
};
use parking_lot::Mutex;
use std::sync::Arc;
use std::time::Duration;
use tokio::task::JoinHandle;

/// Task notification type for distinguishing between one-shot and periodic tasks
///
/// 任务通知类型，用于区分一次性任务和周期性任务
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskNotification {
    /// One-shot task expired notification
    ///
    /// 一次性任务过期通知
    OneShot(TaskId),
    /// Periodic task called notification
    ///
    /// 周期性任务被调用通知
    Periodic(TaskId),
}

impl TaskNotification {
    /// Get the task ID from the notification
    ///
    /// 从通知中获取任务 ID
    pub fn task_id(&self) -> TaskId {
        match self {
            TaskNotification::OneShot(id) => *id,
            TaskNotification::Periodic(id) => *id,
        }
    }

    /// Check if this is a one-shot task notification
    ///
    /// 检查是否为一次性任务通知
    pub fn is_oneshot(&self) -> bool {
        matches!(self, TaskNotification::OneShot(_))
    }

    /// Check if this is a periodic task notification
    ///
    /// 检查是否为周期性任务通知
    pub fn is_periodic(&self) -> bool {
        matches!(self, TaskNotification::Periodic(_))
    }
}

/// Service command type
///
/// 服务命令类型
enum ServiceCommand {
    /// Add batch timer handle, only contains necessary data: task_ids and completion_rxs
    ///
    /// 添加批量定时器句柄，仅包含必要数据：task_ids 和 completion_rxs
    AddBatchHandle {
        task_ids: Vec<TaskId>,
        completion_rxs: Vec<CompletionReceiver>,
    },
    /// Add single timer handle, only contains necessary data: task_id and completion_rx
    ///
    /// 添加单个定时器句柄，仅包含必要数据：task_id 和 completion_rx
    AddTimerHandle {
        task_id: TaskId,
        completion_rx: CompletionReceiver,
    },
}

/// TimerService - timer service based on Actor pattern
/// Manages multiple timer handles, listens to all timeout events, and aggregates notifications to be forwarded to the user.
/// # Features
/// - Automatically listens to all added timer handles' timeout events
/// - Automatically removes one-shot tasks from internal management after timeout
/// - Continuously monitors periodic tasks and forwards each invocation
/// - Aggregates notifications (both one-shot and periodic) to be forwarded to the user's unified channel
/// - Supports dynamic addition of BatchHandle and TimerHandle
///
///
/// # 定时器服务，基于 Actor 模式管理多个定时器句柄，监听所有超时事件，并将通知聚合转发给用户
/// - 自动监听所有添加的定时器句柄的超时事件
/// - 自动在一次性任务超时后从内部管理中移除任务
/// - 持续监听周期性任务并转发每次调用通知
/// - 将通知（一次性和周期性）聚合转发给用户
/// - 支持动态添加 BatchHandle 和 TimerHandle
///
/// # Examples (示例)
/// ```no_run
/// use kestrel_timer::{TimerWheel, TimerService, CallbackWrapper, TaskNotification, config::ServiceConfig};
/// use std::time::Duration;
///
/// #[tokio::main]
/// async fn main() {
///     let timer = TimerWheel::with_defaults();
///     let mut service = timer.create_service(ServiceConfig::default());
///     
///     // Register one-shot tasks (注册一次性任务)
///     use kestrel_timer::TimerTask;
///     let handles = service.allocate_handles(3);
///     let tasks: Vec<_> = (0..3)
///         .map(|i| {
///             let callback = Some(CallbackWrapper::new(move || async move {
///                 println!("One-shot timer {} fired!", i);
///             }));
///             TimerTask::new_oneshot(Duration::from_millis(100), callback)
///         })
///         .collect();
///     service.register_batch(handles, tasks).unwrap();
///     
///     // Register periodic tasks (注册周期性任务)
///     let handle = service.allocate_handle();
///     let periodic_task = TimerTask::new_periodic(
///         Duration::from_millis(100),
///         Duration::from_millis(50),
///         Some(CallbackWrapper::new(|| async { println!("Periodic timer fired!"); })),
///         None
///     );
///     service.register(handle, periodic_task).unwrap();
///     
///     // Receive notifications (接收通知)
///     let rx = service.take_receiver().unwrap();
///     while let Some(notification) = rx.recv().await {
///         match notification {
///             TaskNotification::OneShot(task_id) => {
///                 println!("One-shot task {:?} expired", task_id);
///             }
///             TaskNotification::Periodic(task_id) => {
///                 println!("Periodic task {:?} called", task_id);
///             }
///         }
///     }
/// }
/// ```
pub struct TimerService {
    /// Command sender
    ///
    /// 命令发送器
    command_tx: spsc::Sender<ServiceCommand, 32>,
    /// Timeout receiver (supports both one-shot and periodic task notifications)
    ///
    /// 超时接收器（支持一次性和周期性任务通知）
    timeout_rx: Option<spsc::Receiver<TaskNotification, 32>>,
    /// Actor task handle
    ///
    /// Actor 任务句柄
    actor_handle: Option<JoinHandle<()>>,
    /// Timing wheel reference (for direct scheduling of timers)
    ///
    /// 时间轮引用（用于直接调度定时器）
    wheel: Arc<Mutex<Wheel>>,
    /// Actor shutdown signal sender
    ///
    /// Actor 关闭信号发送器
    shutdown_tx: Option<Sender<()>>,
}

impl TimerService {
    /// Allocate a handle from DeferredMap
    ///
    /// # Returns
    /// A unique handle for later insertion
    ///
    /// # 返回值
    /// 用于后续插入的唯一 handle
    ///
    /// # Examples (示例)
    /// ```no_run
    /// # use kestrel_timer::{TimerWheel, config::ServiceConfig};
    /// # #[tokio::main]
    /// # async fn main() {
    /// let timer = TimerWheel::with_defaults();
    /// let mut service = timer.create_service(ServiceConfig::default());
    ///
    /// // Allocate handle first    
    /// // 先分配handle
    /// let handle = service.allocate_handle();
    /// # }
    /// ```
    pub fn allocate_handle(&self) -> crate::task::TaskHandle {
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
    ///
    /// # Examples (示例)
    /// ```no_run
    /// # use kestrel_timer::{TimerWheel, config::ServiceConfig};
    /// # #[tokio::main]
    /// # async fn main() {
    /// let timer = TimerWheel::with_defaults();
    /// let service = timer.create_service(ServiceConfig::default());
    ///
    /// // Batch allocate handles
    /// // 批量分配 handles
    /// let handles = service.allocate_handles(10);
    /// assert_eq!(handles.len(), 10);
    /// # }
    /// ```
    pub fn allocate_handles(&self, count: usize) -> Vec<crate::task::TaskHandle> {
        self.wheel.lock().allocate_handles(count)
    }

    /// Create new TimerService
    ///
    /// # Parameters
    /// - `wheel`: Timing wheel reference
    /// - `config`: Service configuration
    ///
    /// # Notes
    /// Typically not called directly, but used to create through `TimerWheel::create_service()`
    ///
    /// 创建新的 TimerService
    ///
    /// # 参数
    /// - `wheel`: 时间轮引用
    /// - `config`: 服务配置
    ///
    /// # 注意
    /// 通常不直接调用，而是通过 `TimerWheel::create_service()` 创建
    ///
    pub(crate) fn new(wheel: Arc<Mutex<Wheel>>, config: ServiceConfig) -> Self {
        let (command_tx, command_rx) = spsc::channel(config.command_channel_capacity);
        let (timeout_tx, timeout_rx) = spsc::channel(config.timeout_channel_capacity);

        let (shutdown_tx, shutdown_rx) = channel::<()>();
        let actor = ServiceActor::new(command_rx, timeout_tx, shutdown_rx);
        let actor_handle = tokio::spawn(async move {
            actor.run().await;
        });

        Self {
            command_tx,
            timeout_rx: Some(timeout_rx),
            actor_handle: Some(actor_handle),
            wheel,
            shutdown_tx: Some(shutdown_tx),
        }
    }

    /// Get timeout receiver (transfer ownership)
    ///
    /// # Returns
    /// Timeout notification receiver, if already taken, returns None
    ///
    /// # Notes
    /// This method can only be called once, because it transfers ownership of the receiver
    /// The receiver will receive both one-shot task expired notifications and periodic task called notifications
    ///
    /// 获取超时通知接收器(转移所有权)
    ///
    /// # 返回值
    /// 超时通知接收器，如果已经取走，返回 None
    ///
    /// # 注意
    /// 此方法只能调用一次，因为它转移了接收器的所有权
    /// 接收器将接收一次性任务过期通知和周期性任务被调用通知
    ///
    /// # Examples (示例)
    /// ```no_run
    /// # use kestrel_timer::{TimerWheel, config::ServiceConfig, TaskNotification};
    /// # #[tokio::main]
    /// # async fn main() {
    /// let timer = TimerWheel::with_defaults();
    /// let mut service = timer.create_service(ServiceConfig::default());
    ///
    /// let rx = service.take_receiver().unwrap();
    /// while let Some(notification) = rx.recv().await {
    ///     match notification {
    ///         TaskNotification::OneShot(task_id) => {
    ///             println!("One-shot task {:?} expired", task_id);
    ///         }
    ///         TaskNotification::Periodic(task_id) => {
    ///             println!("Periodic task {:?} called", task_id);
    ///         }
    ///     }
    /// }
    /// # }
    /// ```
    pub fn take_receiver(&mut self) -> Option<spsc::Receiver<TaskNotification, 32>> {
        self.timeout_rx.take()
    }

    /// Cancel specified task
    ///
    /// # Parameters
    /// - `task_id`: Task ID to cancel
    ///
    /// # Returns
    /// - `true`: Task exists and cancellation is successful
    /// - `false`: Task does not exist or cancellation fails
    ///
    /// 取消指定任务
    ///
    /// # 参数
    /// - `task_id`: 任务 ID
    ///
    /// # 返回值
    /// - `true`: 任务存在且取消成功
    /// - `false`: 任务不存在或取消失败
    ///
    /// # Examples (示例)
    /// ```no_run
    /// # use kestrel_timer::{TimerWheel, TimerService, CallbackWrapper, TimerTask, config::ServiceConfig};
    /// # use std::time::Duration;
    /// #
    /// # #[tokio::main]
    /// # async fn main() {
    /// let timer = TimerWheel::with_defaults();
    /// let service = timer.create_service(ServiceConfig::default());
    ///
    /// // Use two-step API to schedule timers
    /// let handle = service.allocate_handle();
    /// let task_id = handle.task_id();
    /// let callback = Some(CallbackWrapper::new(|| async move {
    ///     println!("Timer fired!"); // 定时器触发
    /// }));
    /// let task = TimerTask::new_oneshot(Duration::from_secs(10), callback);
    /// service.register(handle, task).unwrap(); // 注册定时器
    ///
    /// // Cancel task
    /// let cancelled = service.cancel_task(task_id);
    /// println!("Task cancelled: {}", cancelled); // 任务取消
    /// # }
    /// ```
    #[inline]
    pub fn cancel_task(&self, task_id: TaskId) -> bool {
        // Direct cancellation, no need to notify Actor
        // FuturesUnordered will automatically clean up when tasks are cancelled
        // 直接取消，无需通知 Actor
        // FuturesUnordered 将在任务取消时自动清理
        let mut wheel = self.wheel.lock();
        wheel.cancel(task_id)
    }

    /// Batch cancel tasks
    ///
    /// Use underlying batch cancellation operation to cancel multiple tasks at once, performance is better than calling cancel_task repeatedly.
    ///
    /// # Parameters
    /// - `task_ids`: List of task IDs to cancel
    ///
    /// # Returns
    /// Number of successfully cancelled tasks
    ///
    /// 批量取消任务
    ///
    /// # 参数
    /// - `task_ids`: 任务 ID 列表
    ///
    /// # 返回值
    /// 成功取消的任务数量
    ///
    /// # Examples (示例)
    /// ```no_run
    /// # use kestrel_timer::{TimerWheel, TimerService, CallbackWrapper, TimerTask, config::ServiceConfig};
    /// # use std::time::Duration;
    /// #
    /// # #[tokio::main]
    /// # async fn main() {
    /// let timer = TimerWheel::with_defaults();
    /// let service = timer.create_service(ServiceConfig::default());
    ///
    /// let handles = service.allocate_handles(10);
    /// let task_ids: Vec<_> = handles.iter().map(|h| h.task_id()).collect();
    /// let tasks: Vec<_> = (0..10)
    ///     .map(|i| {
    ///         let callback = Some(CallbackWrapper::new(move || async move {
    ///             println!("Timer {} fired!", i); // 定时器触发
    ///         }));
    ///         TimerTask::new_oneshot(Duration::from_secs(10), callback)
    ///     })
    ///     .collect();
    /// service.register_batch(handles, tasks).unwrap(); // 注册定时器
    ///
    /// // Batch cancel
    /// let cancelled = service.cancel_batch(&task_ids);
    /// println!("Cancelled {} tasks", cancelled); // 任务取消
    /// # }
    /// ```
    #[inline]
    pub fn cancel_batch(&self, task_ids: &[TaskId]) -> usize {
        if task_ids.is_empty() {
            return 0;
        }

        // Direct batch cancellation, no need to notify Actor
        // FuturesUnordered will automatically clean up when tasks are cancelled
        // 直接批量取消，无需通知 Actor
        // FuturesUnordered 将在任务取消时自动清理
        let mut wheel = self.wheel.lock();
        wheel.cancel_batch(task_ids)
    }

    /// Postpone task (optionally replace callback)
    ///
    /// # Parameters
    /// - `task_id`: Task ID to postpone
    /// - `new_delay`: New delay time (recalculated from current time point)
    /// - `callback`: New callback function (if `None`, keeps the original callback)
    ///
    /// # Returns
    /// - `true`: Task exists and is successfully postponed
    /// - `false`: Task does not exist or postponement fails
    ///
    /// # Notes
    /// - Task ID remains unchanged after postponement
    /// - Original timeout notification remains valid
    /// - If callback is `Some`, it will replace the original callback
    /// - If callback is `None`, the original callback is preserved
    ///
    /// 推迟任务 (可选替换回调)
    ///
    /// # 参数
    /// - `task_id`: 任务 ID
    /// - `new_delay`: 新的延迟时间 (从当前时间点重新计算)
    /// - `callback`: 新的回调函数 (如果为 `None`，则保留原回调)
    ///
    /// # 返回值
    /// - `true`: 任务存在且延期成功
    /// - `false`: 任务不存在或延期失败
    ///
    /// # 注意
    /// - 任务 ID 在延期后保持不变
    /// - 原始超时通知保持有效
    /// - 如果 callback 为 `Some`，将替换原始回调
    /// - 如果 callback 为 `None`，保留原始回调
    ///
    /// # Examples (示例)
    /// ```no_run
    /// # use kestrel_timer::{TimerWheel, TimerService, CallbackWrapper, TimerTask, config::ServiceConfig};
    /// # use std::time::Duration;
    /// #
    /// # #[tokio::main]
    /// # async fn main() {
    /// let timer = TimerWheel::with_defaults();
    /// let service = timer.create_service(ServiceConfig::default());
    ///
    /// let handle = service.allocate_handle();
    /// let task_id = handle.task_id();
    /// let callback = Some(CallbackWrapper::new(|| async {
    ///     println!("Original callback"); // 原始回调
    /// }));
    /// let task = TimerTask::new_oneshot(Duration::from_secs(5), callback);
    /// service.register(handle, task).unwrap(); // 注册定时器
    ///
    /// // Postpone and replace callback (延期并替换回调)
    /// let new_callback = Some(CallbackWrapper::new(|| async { println!("New callback!"); }));
    /// let success = service.postpone(
    ///     task_id,
    ///     Duration::from_secs(10),
    ///     new_callback
    /// );
    /// println!("Postponed successfully: {}", success);
    /// # }
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

    /// Batch postpone tasks (keep original callbacks)
    ///
    /// # Parameters
    /// - `updates`: List of tuples of (task ID, new delay)
    ///
    /// # Returns
    /// Number of successfully postponed tasks
    ///
    /// 批量延期任务 (保持原始回调)
    ///
    /// # 参数
    /// - `updates`: (任务 ID, 新延迟) 元组列表
    ///
    /// # 返回值
    /// 成功延期的任务数量
    ///
    /// # Examples (示例)
    /// ```no_run
    /// # use kestrel_timer::{TimerWheel, TimerService, CallbackWrapper, TimerTask, config::ServiceConfig};
    /// # use std::time::Duration;
    /// #
    /// # #[tokio::main]
    /// # async fn main() {
    /// let timer = TimerWheel::with_defaults();
    /// let service = timer.create_service(ServiceConfig::default());
    ///
    /// let handles = service.allocate_handles(3);
    /// let task_ids: Vec<_> = handles.iter().map(|h| h.task_id()).collect();
    /// let tasks: Vec<_> = (0..3)
    ///     .map(|i| {
    ///         let callback = Some(CallbackWrapper::new(move || async move {
    ///             println!("Timer {} fired!", i);
    ///         }));
    ///         TimerTask::new_oneshot(Duration::from_secs(5), callback)
    ///     })
    ///     .collect();
    /// service.register_batch(handles, tasks).unwrap();
    ///
    /// // Batch postpone (keep original callbacks)
    /// // 批量延期任务 (保持原始回调)
    /// let updates: Vec<_> = task_ids
    ///     .into_iter()
    ///     .map(|id| (id, Duration::from_secs(10)))
    ///     .collect();
    /// let postponed = service.postpone_batch(updates);
    /// println!("Postponed {} tasks", postponed);
    /// # }
    /// ```
    #[inline]
    pub fn postpone_batch(&self, updates: Vec<(TaskId, Duration)>) -> usize {
        if updates.is_empty() {
            return 0;
        }

        let mut wheel = self.wheel.lock();
        wheel.postpone_batch(updates)
    }

    /// Batch postpone tasks (replace callbacks)
    ///
    /// # Parameters
    /// - `updates`: List of tuples of (task ID, new delay, new callback)
    ///
    /// # Returns
    /// Number of successfully postponed tasks
    ///
    /// 批量延期任务 (替换回调)
    ///
    /// # 参数
    /// - `updates`: (任务 ID, 新延迟, 新回调) 元组列表
    ///
    /// # 返回值
    /// 成功延期的任务数量
    ///
    /// # Examples (示例)
    /// ```no_run
    /// # use kestrel_timer::{TimerWheel, TimerService, CallbackWrapper, config::ServiceConfig};
    /// # use std::time::Duration;
    /// #
    /// # #[tokio::main]
    /// # async fn main() {
    /// # use kestrel_timer::TimerTask;
    /// let timer = TimerWheel::with_defaults();
    /// let service = timer.create_service(ServiceConfig::default());
    ///
    /// // Create 3 tasks, initially no callbacks
    /// // 创建 3 个任务，最初没有回调
    /// let handles = service.allocate_handles(3);
    /// let task_ids: Vec<_> = handles.iter().map(|h| h.task_id()).collect();
    /// let tasks: Vec<_> = (0..3)
    ///     .map(|_| {
    ///         TimerTask::new_oneshot(Duration::from_secs(5), None)
    ///     })
    ///     .collect();
    /// service.register_batch(handles, tasks).unwrap();
    ///
    /// // Batch postpone and add new callbacks
    /// // 批量延期并添加新的回调
    /// let updates: Vec<_> = task_ids
    ///     .into_iter()
    ///     .enumerate()
    ///     .map(|(i, id)| {
    ///         let callback = Some(CallbackWrapper::new(move || async move {
    ///             println!("New callback {}", i);
    ///         }));
    ///         (id, Duration::from_secs(10), callback)
    ///     })
    ///     .collect();
    /// let postponed = service.postpone_batch_with_callbacks(updates);
    /// println!("Postponed {} tasks", postponed);
    /// # }
    /// ```
    #[inline]
    pub fn postpone_batch_with_callbacks(
        &self,
        updates: Vec<(TaskId, Duration, Option<CallbackWrapper>)>,
    ) -> usize {
        if updates.is_empty() {
            return 0;
        }

        let mut wheel = self.wheel.lock();
        wheel.postpone_batch_with_callbacks(updates)
    }

    /// Register timer task to service (registration phase)
    ///
    /// # Parameters
    /// - `handle`: Handle allocated via `allocate_handle()`
    /// - `task`: Task created via `TimerTask::new_oneshot()` or `TimerTask::new_periodic()`
    ///
    /// # Returns
    /// - `Ok(TimerHandle)`: Register successfully
    /// - `Err(TimerError::RegisterFailed)`: Register failed (internal channel is full or closed)
    ///
    /// 注册定时器任务到服务 (注册阶段)
    /// # 参数
    /// - `handle`: 通过 `allocate_handle()` 分配的 handle
    /// - `task`: 通过 `TimerTask::new_oneshot()` 或 `TimerTask::new_periodic()` 创建的任务
    ///
    /// # 返回值
    /// - `Ok(TimerHandle)`: 注册成功
    /// - `Err(TimerError::RegisterFailed)`: 注册失败 (内部通道已满或关闭)
    ///
    /// # Examples (示例)
    /// ```no_run
    /// # use kestrel_timer::{TimerWheel, TimerService, CallbackWrapper, config::ServiceConfig, TimerTask};
    /// # use std::time::Duration;
    /// #
    /// # #[tokio::main]
    /// # async fn main() {
    /// let timer = TimerWheel::with_defaults();
    /// let service = timer.create_service(ServiceConfig::default());
    ///
    /// // Step 1: allocate handle
    /// // 分配 handle
    /// let handle = service.allocate_handle();
    /// let task_id = handle.task_id();
    ///
    /// // Step 2: create task
    /// // 创建任务
    /// let callback = Some(CallbackWrapper::new(|| async move {
    ///     println!("Timer fired!");
    /// }));
    /// let task = TimerTask::new_oneshot(Duration::from_millis(100), callback);
    ///
    /// // Step 3: register task
    /// // 注册任务
    /// service.register(handle, task).unwrap();
    /// # }
    /// ```
    #[inline]
    pub fn register(
        &self,
        handle: crate::task::TaskHandle,
        task: crate::task::TimerTask,
    ) -> Result<TimerHandle, TimerError> {
        let task_id = handle.task_id();

        let (task, completion_rx) =
            crate::task::TimerTaskWithCompletionNotifier::from_timer_task(task);

        // Single lock, complete all operations
        // 单次锁定，完成所有操作
        {
            let mut wheel_guard = self.wheel.lock();
            wheel_guard.insert(handle, task);
        }

        // Add to service management (only send necessary data)
        // 添加到服务管理（只发送必要数据）
        self.command_tx
            .try_send(ServiceCommand::AddTimerHandle {
                task_id,
                completion_rx,
            })
            .map_err(|_| TimerError::RegisterFailed)?;

        Ok(TimerHandle::new(task_id, self.wheel.clone()))
    }

    /// Batch register timer tasks to service (registration phase)
    ///
    /// # Parameters
    /// - `handles`: Pre-allocated handles for tasks
    /// - `tasks`: List of timer tasks
    ///
    /// # Returns
    /// - `Ok(BatchHandle)`: Register successfully
    /// - `Err(TimerError::RegisterFailed)`: Register failed (internal channel is full or closed)
    /// - `Err(TimerError::BatchLengthMismatch)`: Handles and tasks lengths don't match
    ///
    /// 批量注册定时器任务到服务 (注册阶段)
    /// # 参数
    /// - `handles`: 任务的预分配 handles
    /// - `tasks`: 定时器任务列表
    ///
    /// # 返回值
    /// - `Ok(BatchHandle)`: 注册成功
    /// - `Err(TimerError::RegisterFailed)`: 注册失败 (内部通道已满或关闭)
    /// - `Err(TimerError::BatchLengthMismatch)`: handles 和 tasks 长度不匹配
    ///
    /// # Examples (示例)
    /// ```no_run
    /// # use kestrel_timer::{TimerWheel, TimerService, CallbackWrapper, config::ServiceConfig, TimerTask};
    /// # use std::time::Duration;
    /// #
    /// # #[tokio::main]
    /// # async fn main() {
    /// # use kestrel_timer::TimerTask;
    /// let timer = TimerWheel::with_defaults();
    /// let service = timer.create_service(ServiceConfig::default());
    ///
    /// // Step 1: batch allocate handles
    /// // 批量分配 handles
    /// let handles = service.allocate_handles(3);
    ///
    /// // Step 2: create tasks
    /// // 创建任务
    /// let tasks: Vec<_> = (0..3)
    ///     .map(|i| {
    ///         let callback = Some(CallbackWrapper::new(move || async move {
    ///             println!("Timer {} fired!", i);
    ///         }));
    ///         TimerTask::new_oneshot(Duration::from_secs(1), callback)
    ///     })
    ///     .collect();
    ///
    /// // Step 3: register batch
    /// // 注册批量任务
    /// service.register_batch(handles, tasks).unwrap();
    /// # }
    /// ```
    #[inline]
    pub fn register_batch(
        &self,
        handles: Vec<crate::task::TaskHandle>,
        tasks: Vec<crate::task::TimerTask>,
    ) -> Result<BatchHandle, TimerError> {
        // Validate lengths match
        if handles.len() != tasks.len() {
            return Err(TimerError::BatchLengthMismatch {
                handles_len: handles.len(),
                tasks_len: tasks.len(),
            });
        }

        let task_count = tasks.len();
        let mut completion_rxs = Vec::with_capacity(task_count);
        let mut task_ids = Vec::with_capacity(task_count);
        let mut prepared_handles = Vec::with_capacity(task_count);
        let mut prepared_tasks = Vec::with_capacity(task_count);

        // Step 1: prepare all channels and notifiers (no lock)
        // 步骤 1: 准备所有通道和通知器（无锁）
        for (handle, task) in handles.into_iter().zip(tasks.into_iter()) {
            let task_id = handle.task_id();
            let (task, completion_rx) =
                crate::task::TimerTaskWithCompletionNotifier::from_timer_task(task);
            task_ids.push(task_id);
            completion_rxs.push(completion_rx);
            prepared_handles.push(handle);
            prepared_tasks.push(task);
        }

        // Step 2: single lock, batch insert
        // 步骤 2: 单次锁定，批量插入
        {
            let mut wheel_guard = self.wheel.lock();
            wheel_guard.insert_batch(prepared_handles, prepared_tasks)?;
        }

        // Add to service management (only send necessary data)
        // 添加到服务管理（只发送必要数据）
        self.command_tx
            .try_send(ServiceCommand::AddBatchHandle {
                task_ids: task_ids.clone(),
                completion_rxs,
            })
            .map_err(|_| TimerError::RegisterFailed)?;

        Ok(BatchHandle::new(task_ids, self.wheel.clone()))
    }

    /// Graceful shutdown of TimerService
    ///
    /// 优雅关闭 TimerService
    ///
    /// # Examples (示例)
    /// ```no_run
    /// # use kestrel_timer::{TimerWheel, config::ServiceConfig};
    /// # #[tokio::main]
    /// # async fn main() {
    /// let timer = TimerWheel::with_defaults();
    /// let mut service = timer.create_service(ServiceConfig::default());
    ///
    /// // Use service... (使用服务...)
    ///
    /// service.shutdown().await;
    /// # }
    /// ```
    pub async fn shutdown(mut self) {
        if let Some(shutdown_tx) = self.shutdown_tx.take() {
            shutdown_tx.notify(());
        }
        if let Some(handle) = self.actor_handle.take() {
            let _ = handle.await;
        }
    }
}

impl Drop for TimerService {
    fn drop(&mut self) {
        if let Some(handle) = self.actor_handle.take() {
            handle.abort();
        }
    }
}

/// ServiceActor - internal Actor implementation
///
/// ServiceActor - 内部 Actor 实现
struct ServiceActor {
    /// Command receiver
    ///
    /// 命令接收器
    command_rx: spsc::Receiver<ServiceCommand, 32>,
    /// Timeout sender (supports both one-shot and periodic task notifications)
    ///
    /// 超时发送器（支持一次性和周期性任务通知）
    timeout_tx: spsc::Sender<TaskNotification, 32>,
    /// Actor shutdown signal receiver
    ///
    /// Actor 关闭信号接收器
    shutdown_rx: Receiver<()>,
}

impl ServiceActor {
    /// Create new ServiceActor
    ///
    /// 创建新的 ServiceActor
    fn new(
        command_rx: spsc::Receiver<ServiceCommand, 32>,
        timeout_tx: spsc::Sender<TaskNotification, 32>,
        shutdown_rx: Receiver<()>,
    ) -> Self {
        Self {
            command_rx,
            timeout_tx,
            shutdown_rx,
        }
    }

    /// Run Actor event loop
    ///
    /// 运行 Actor 事件循环
    async fn run(self) {
        // Use separate FuturesUnordered for one-shot and periodic tasks
        // 为一次性任务和周期性任务使用独立的 FuturesUnordered

        // One-shot futures: each future returns (TaskId, TaskCompletion)
        // 一次性任务futures：每个future 返回 (TaskId, TaskCompletion)
        let mut oneshot_futures: FuturesUnordered<BoxFuture<'static, (TaskId, TaskCompletion)>> =
            FuturesUnordered::new();

        // Periodic futures: each future returns (TaskId, Option<PeriodicTaskCompletion>, mpsc::Receiver)
        // The receiver is returned so we can continue listening for next event
        // 周期性任务futures：每个future 返回 (TaskId, Option<PeriodicTaskCompletion>, mpsc::Receiver)
        // 返回接收器以便我们可以继续监听下一个事件
        type PeriodicFutureResult = (
            TaskId,
            Option<TaskCompletion>,
            crate::task::PeriodicCompletionReceiver,
        );
        let mut periodic_futures: FuturesUnordered<BoxFuture<'static, PeriodicFutureResult>> =
            FuturesUnordered::new();

        // Move shutdown_rx out of self, so it can be used in select! with &mut
        // 将 shutdown_rx 从 self 中移出，以便在 select! 中使用 &mut
        let mut shutdown_rx = self.shutdown_rx;

        loop {
            tokio::select! {
                // Listen to high-priority shutdown signal
                // 监听高优先级关闭信号
                _ = &mut shutdown_rx => {
                    // Receive shutdown signal, exit loop immediately
                    // 接收到关闭信号，立即退出循环
                    break;
                }

                // Listen to one-shot task timeout events
                // 监听一次性任务超时事件
                Some((task_id, completion)) = oneshot_futures.next() => {
                    // Check completion reason, only forward Called events, do not forward Cancelled events
                    // 检查完成原因，只转发 Called 事件，不转发 Cancelled 事件
                    if completion == TaskCompletion::Called {
                        let _ = self.timeout_tx.send(TaskNotification::OneShot(task_id)).await;
                    }
                    // Task will be automatically removed from FuturesUnordered
                    // 任务将自动从 FuturesUnordered 中移除
                }

                // Listen to periodic task events
                // 监听周期性任务事件
                Some((task_id, reason, mut receiver)) = periodic_futures.next() => {
                    // Check completion reason, only forward Called events, do not forward Cancelled events
                    // 检查完成原因，只转发 Called 事件，不转发 Cancelled 事件
                    if let Some(TaskCompletion::Called) = reason {
                        let _ = self.timeout_tx.send(TaskNotification::Periodic(task_id)).await;

                        // Re-add the receiver to continue listening for next periodic event
                        // 重新添加接收器以继续监听下一个周期性事件
                        let future: BoxFuture<'static, PeriodicFutureResult> = Box::pin(async move {
                            let reason = receiver.recv().await;
                            (task_id, reason, receiver)
                        });
                        periodic_futures.push(future);
                    }
                    // If Cancelled or None, do not re-add the future (task is done)
                    // 如果 Cancelled 或 None，不重新添加 future（任务结束）
                }

                // Listen to commands
                // 监听命令
                Some(cmd) = self.command_rx.recv() => {
                    match cmd {
                        ServiceCommand::AddBatchHandle { task_ids, completion_rxs } => {
                            // Add all tasks to appropriate futures
                            // 将所有任务添加到相应 futures
                            for (task_id, rx) in task_ids.into_iter().zip(completion_rxs.into_iter()) {
                                match rx {
                                    crate::task::CompletionReceiver::OneShot(receiver) => {
                                        let future: BoxFuture<'static, (TaskId, TaskCompletion)> = Box::pin(async move {
                                            // unwrap() is safe here: the sender is held by the task and will send
                                            // before being dropped. If the sender is dropped without sending,
                                            // it's a logic error in the task implementation.
                                            // unwrap() 在这里是安全的：发送器由任务持有，在被丢弃前会发送。
                                            // 如果发送器在未发送的情况下被丢弃，这是任务实现中的逻辑错误。
                                            (task_id, receiver.recv().await.unwrap())
                                        });
                                        oneshot_futures.push(future);
                                    },
                                    crate::task::CompletionReceiver::Periodic(mut receiver) => {
                                        let future: BoxFuture<'static, PeriodicFutureResult> = Box::pin(async move {
                                            let reason = receiver.recv().await;
                                            (task_id, reason, receiver)
                                        });
                                        periodic_futures.push(future);
                                    }
                                }
                            }
                        }
                        ServiceCommand::AddTimerHandle { task_id, completion_rx } => {
                            // Add to appropriate futures
                            // 添加到相应的 futures
                            match completion_rx {
                                crate::task::CompletionReceiver::OneShot(receiver) => {
                                    let future: BoxFuture<'static, (TaskId, TaskCompletion)> = Box::pin(async move {
                                        // unwrap() is safe here: the sender is held by the task and will send
                                        // before being dropped. If the sender is dropped without sending,
                                        // it's a logic error in the task implementation.
                                        // unwrap() 在这里是安全的：发送器由任务持有，在被丢弃前会发送。
                                        // 如果发送器在未发送的情况下被丢弃，这是任务实现中的逻辑错误。
                                        (task_id, receiver.recv().await.unwrap())
                                    });
                                    oneshot_futures.push(future);
                                },
                                crate::task::CompletionReceiver::Periodic(mut receiver) => {
                                    let future: BoxFuture<'static, PeriodicFutureResult> = Box::pin(async move {
                                        let reason = receiver.recv().await;
                                        (task_id, reason, receiver)
                                    });
                                    periodic_futures.push(future);
                                }
                            }
                        }
                    }
                }

                // If no futures and command channel is closed, exit loop
                // 如果没有 futures 且命令通道关闭，退出循环
                else => {
                    break;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{TimerTask, TimerWheel};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::time::Duration;

    #[tokio::test]
    async fn test_service_creation() {
        let timer = TimerWheel::with_defaults();
        let _service = timer.create_service(ServiceConfig::default());
    }

    #[tokio::test]
    async fn test_add_timer_handle_and_receive_timeout() {
        let timer = TimerWheel::with_defaults();
        let mut service = timer.create_service(ServiceConfig::default());

        // Allocate handle (分配 handle)
        let handle = service.allocate_handle();
        let task_id = handle.task_id();

        // Create single timer (创建单个定时器)
        let task = TimerTask::new_oneshot(
            Duration::from_millis(50),
            Some(CallbackWrapper::new(|| async {})),
        );

        // Register to service (注册到服务)
        service.register(handle, task).unwrap();

        // Receive timeout notification (接收超时通知)
        let rx = service.take_receiver().unwrap();
        let received_notification = tokio::time::timeout(Duration::from_millis(200), rx.recv())
            .await
            .expect("Should receive timeout notification")
            .expect("Should receive Some value");

        assert_eq!(received_notification, TaskNotification::OneShot(task_id));
    }

    #[tokio::test]
    async fn test_shutdown() {
        let timer = TimerWheel::with_defaults();
        let service = timer.create_service(ServiceConfig::default());

        // Add some timers (添加一些定时器)
        let handle1 = service.allocate_handle();
        let handle2 = service.allocate_handle();
        let task1 = TimerTask::new_oneshot(Duration::from_secs(10), None);
        let task2 = TimerTask::new_oneshot(Duration::from_secs(10), None);
        service.register(handle1, task1).unwrap();
        service.register(handle2, task2).unwrap();

        // Immediately shutdown (without waiting for timers to trigger) (立即关闭（不等待定时器触发）)
        service.shutdown().await;
    }

    #[tokio::test]
    async fn test_schedule_once_direct() {
        let timer = TimerWheel::with_defaults();
        let mut service = timer.create_service(ServiceConfig::default());
        let counter = Arc::new(AtomicU32::new(0));

        // Schedule timer directly through service
        // 直接通过服务调度定时器
        let counter_clone = Arc::clone(&counter);
        let handle = service.allocate_handle();
        let task_id = handle.task_id();
        let task = TimerTask::new_oneshot(
            Duration::from_millis(50),
            Some(CallbackWrapper::new(move || {
                let counter = Arc::clone(&counter_clone);
                async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                }
            })),
        );
        service.register(handle, task).unwrap();

        // Wait for timer to trigger
        // 等待定时器触发
        let rx = service.take_receiver().unwrap();
        let received_notification = tokio::time::timeout(Duration::from_millis(200), rx.recv())
            .await
            .expect("Should receive timeout notification")
            .expect("Should receive Some value");

        assert_eq!(received_notification, TaskNotification::OneShot(task_id));

        // Wait for callback to execute
        // 等待回调执行
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_schedule_once_notify_direct() {
        let timer = TimerWheel::with_defaults();
        let mut service = timer.create_service(ServiceConfig::default());

        // Schedule only notification timer directly through service (no callback)
        // 直接通过服务调度通知定时器（没有回调函数）
        let handle = service.allocate_handle();
        let task_id = handle.task_id();
        let task = TimerTask::new_oneshot(Duration::from_millis(50), None);
        service.register(handle, task).unwrap();

        // Receive timeout notification
        // 接收超时通知
        let rx = service.take_receiver().unwrap();
        let received_notification = tokio::time::timeout(Duration::from_millis(200), rx.recv())
            .await
            .expect("Should receive timeout notification")
            .expect("Should receive Some value");

        assert_eq!(received_notification, TaskNotification::OneShot(task_id));
    }

    #[tokio::test]
    async fn test_task_timeout_cleans_up_task_sender() {
        let timer = TimerWheel::with_defaults();
        let mut service = timer.create_service(ServiceConfig::default());

        // Add a short-term timer (添加短期定时器)
        let handle = service.allocate_handle();
        let task_id = handle.task_id();
        let task = TimerTask::new_oneshot(Duration::from_millis(50), None);

        service.register(handle, task).unwrap();

        // Wait for task timeout (等待任务超时)
        let rx = service.take_receiver().unwrap();
        let received_notification = tokio::time::timeout(Duration::from_millis(200), rx.recv())
            .await
            .expect("Should receive timeout notification")
            .expect("Should receive Some value");

        assert_eq!(received_notification, TaskNotification::OneShot(task_id));

        // Wait a moment to ensure internal cleanup is complete (等待片刻以确保内部清理完成)
        tokio::time::sleep(Duration::from_millis(10)).await;

        // Try to cancel the timed-out task, should return false (尝试取消超时任务，应返回 false)
        let cancelled = service.cancel_task(task_id);
        assert!(!cancelled, "Timed out task should not exist anymore");
    }

    #[tokio::test]
    async fn test_take_receiver_twice() {
        let timer = TimerWheel::with_defaults();
        let mut service = timer.create_service(ServiceConfig::default());

        // First call should return Some
        // 第一次调用应该返回 Some
        let rx1 = service.take_receiver();
        assert!(rx1.is_some(), "First take_receiver should return Some");

        // Second call should return None
        // 第二次调用应该返回 None
        let rx2 = service.take_receiver();
        assert!(rx2.is_none(), "Second take_receiver should return None");
    }
}
