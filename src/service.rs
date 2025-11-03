use crate::{BatchHandle, TimerHandle};
use crate::config::ServiceConfig;
use crate::error::TimerError;
use crate::task::{CallbackWrapper,  CompletionReceiver, TaskCompletion, TaskId};
use crate::wheel::Wheel;
use futures::stream::{FuturesUnordered, StreamExt};
use futures::future::BoxFuture;
use parking_lot::Mutex;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
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
    /// 添加批量定时器句柄，仅包含必要数据: task_ids 和 completion_rxs
    AddBatchHandle {
        task_ids: Vec<TaskId>,
        completion_rxs: Vec<CompletionReceiver>,
    },
    /// Add single timer handle, only contains necessary data: task_id and completion_rx
    /// 
    /// 添加单个定时器句柄，仅包含必要数据: task_id 和 completion_rx
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
/// # 定时器服务，基于 Actor 模式管理多个定时器句柄，监听所有超时事件，并将通知聚合转发给用户。
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
///     let oneshot_tasks: Vec<_> = (0..3)
///         .map(|i| {
///             let callback = Some(CallbackWrapper::new(move || async move {
///                 println!("One-shot timer {} fired!", i);
///             }));
///             TimerTask::new_oneshot(Duration::from_millis(100), callback)
///         })
///         .collect();
///     service.register_batch(oneshot_tasks).unwrap();
///     
///     // Register periodic tasks (注册周期性任务)
///     let periodic_task = TimerTask::new_periodic(
///         Duration::from_millis(100),
///         Duration::from_millis(50),
///         Some(CallbackWrapper::new(|| async { println!("Periodic timer fired!"); })),
///         None
///     );
///     service.register(periodic_task).unwrap();
///     
///     // Receive notifications (接收通知)
///     let mut rx = service.take_receiver().unwrap();
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
    command_tx: mpsc::Sender<ServiceCommand>,
    /// Timeout receiver (supports both one-shot and periodic task notifications)
    /// 
    /// 超时接收器（支持一次性和周期性任务通知）
    timeout_rx: Option<mpsc::Receiver<TaskNotification>>,
    /// Actor task handle
    /// 
    /// Actor 任务句柄
    actor_handle: Option<JoinHandle<()>>,
    /// Timing wheel reference (for direct scheduling of timers)
    /// 
    /// 时间轮引用 (用于直接调度定时器)
    wheel: Arc<Mutex<Wheel>>,
    /// Actor shutdown signal sender
    /// 
    /// Actor 关闭信号发送器
    shutdown_tx: Option<tokio::sync::oneshot::Sender<()>>,
}

impl TimerService {
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
        let (command_tx, command_rx) = mpsc::channel(config.command_channel_capacity);
        let (timeout_tx, timeout_rx) = mpsc::channel::<TaskNotification>(config.timeout_channel_capacity);

        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel();
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
    /// 获取超时通知接收器 (转移所有权)
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
    /// let mut rx = service.take_receiver().unwrap();
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
    pub fn take_receiver(&mut self) -> Option<mpsc::Receiver<TaskNotification>> {
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
    /// let callback = Some(CallbackWrapper::new(|| async move {
    ///     println!("Timer fired!"); // 定时器触发
    /// }));
    /// let task = TimerTask::new_oneshot(Duration::from_secs(10), callback);
    /// let task_id = task.get_id();
    /// service.register(task).unwrap(); // 注册定时器
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
    /// let tasks: Vec<_> = (0..10)
    ///     .map(|i| {
    ///         let callback = Some(CallbackWrapper::new(move || async move {
    ///             println!("Timer {} fired!", i); // 定时器触发
    ///         }));
    ///         TimerTask::new_oneshot(Duration::from_secs(10), callback)
    ///     })
    ///     .collect();
    /// let task_ids: Vec<_> = tasks.iter().map(|t| t.get_id()).collect();
    /// service.register_batch(tasks).unwrap(); // 注册定时器
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

    /// Postpone task (replace callback)
    /// 
    /// # Parameters
    /// - `task_id`: Task ID to postpone
    /// - `new_delay`: New delay time (recalculated from current time point)
    /// - `callback`: New callback function
    ///
    /// # Returns
    /// - `true`: Task exists and is successfully postponed
    /// - `false`: Task does not exist or postponement fails
    ///
    /// # Notes
    /// - Task ID remains unchanged after postponement
    /// - Original timeout notification remains valid
    /// - Callback function will be replaced with new callback
    ///
    /// 推迟任务 (替换回调)
    /// 
    /// # 参数
    /// - `task_id`: 任务 ID
    /// - `new_delay`: 新的延迟时间 (从当前时间点重新计算)
    /// - `callback`: 新的回调函数
    ///
    /// # 返回值
    /// - `true`: 任务存在且延期成功
    /// - `false`: 任务不存在或延期失败
    ///
    /// # 注意
    /// - 任务 ID 在延期后保持不变
    /// - 原始超时通知保持有效
    /// - 回调函数将被新的回调函数替换
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
    /// let callback = Some(CallbackWrapper::new(|| async {
    ///     println!("Original callback"); // 原始回调
    /// }));
    /// let task = TimerTask::new_oneshot(Duration::from_secs(5), callback);
    /// let task_id = task.get_id();
    /// service.register(task).unwrap(); // 注册定时器
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
    pub fn postpone(&self, task_id: TaskId, new_delay: Duration, callback: Option<CallbackWrapper>) -> bool {
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
    /// let tasks: Vec<_> = (0..3)
    ///     .map(|i| {
    ///         let callback = Some(CallbackWrapper::new(move || async move {
    ///             println!("Timer {} fired!", i);
    ///         }));
    ///         TimerTask::new_oneshot(Duration::from_secs(5), callback)
    ///     })
    ///     .collect();
    /// let task_ids: Vec<_> = tasks.iter().map(|t| t.get_id()).collect();
    /// service.register_batch(tasks).unwrap();
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
    /// let tasks: Vec<_> = (0..3)
    ///     .map(|_| TimerTask::new_oneshot(Duration::from_secs(5), None))
    ///     .collect();
    /// let task_ids: Vec<_> = tasks.iter().map(|t| t.get_id()).collect();
    /// service.register_batch(tasks).unwrap();
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
    /// - `task`: Task created via `TimerTask::new_oneshot()`
    /// 
    /// # Returns
    /// - `Ok(TimerHandle)`: Register successfully
    /// - `Err(TimerError::RegisterFailed)`: Register failed (internal channel is full or closed)
    /// 
    /// 注册定时器任务到服务 (注册阶段)
    /// # 参数
    /// - `task`: 通过 `TimerTask::new_oneshot()` 创建的任务
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
    /// // Step 1: create task
    /// // 创建任务
    /// let callback = Some(CallbackWrapper::new(|| async move {
    ///     println!("Timer fired!");
    /// }));
    /// let task = TimerTask::new_oneshot(Duration::from_millis(100), callback);
    /// let task_id = task.get_id();
    /// 
    /// // Step 2: register task
    /// // 注册任务
    /// service.register(task).unwrap();
    /// # }
    /// ```
    #[inline]
    pub fn register(&self, task: crate::task::TimerTask) -> Result<TimerHandle, TimerError> {
        
        let (task, completion_rx) = crate::task::TimerTaskWithCompletionNotifier::from_timer_task(task);

        let task_id = task.id;
        
        // Single lock, complete all operations
        // 单次锁定，完成所有操作
        {
            let mut wheel_guard = self.wheel.lock();
            wheel_guard.insert(task);
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
    /// - `tasks`: List of tasks created via `TimerTask::new_oneshot()`
    /// 
    /// # Returns
    /// - `Ok(BatchHandle)`: Register successfully
    /// - `Err(TimerError::RegisterFailed)`: Register failed (internal channel is full or closed)
    /// 
    /// 批量注册定时器任务到服务 (注册阶段)
    /// # 参数
    /// - `tasks`: 通过 `TimerTask::new_oneshot()` 创建的任务列表
    ///
    /// # 返回值
    /// - `Ok(BatchHandle)`: 注册成功
    /// - `Err(TimerError::RegisterFailed)`: 注册失败 (内部通道已满或关闭)
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
    /// // Step 1: create batch of tasks with callbacks
    /// // 创建批量任务，带有回调
    /// let tasks: Vec<TimerTask> = (0..3)
    ///     .map(|i| {
    ///         let callback = Some(CallbackWrapper::new(move || async move {
    ///             println!("Timer {} fired!", i);
    ///         }));
    ///         TimerTask::new_oneshot(Duration::from_secs(1), callback)
    ///     })
    ///     .collect();
    /// 
    /// // Step 2: register batch of tasks with callbacks
    /// // 注册批量任务，带有回调
    /// service.register_batch(tasks).unwrap();
    /// # }
    /// ```
    #[inline]
    pub fn register_batch(&self, tasks: Vec<crate::task::TimerTask>) -> Result<BatchHandle, TimerError> {
        let task_count = tasks.len();
        let mut completion_rxs = Vec::with_capacity(task_count);
        let mut task_ids = Vec::with_capacity(task_count);
        let mut prepared_tasks = Vec::with_capacity(task_count);
        
        // Step 1: prepare all channels and notifiers (no lock)
        // 步骤 1: 准备所有通道和通知器（无锁）
        for task in tasks {
            let (task, completion_rx) = crate::task::TimerTaskWithCompletionNotifier::from_timer_task(task);
            task_ids.push(task.id);
            completion_rxs.push(completion_rx);
            prepared_tasks.push(task);
        }
        
        // Step 2: single lock, batch insert
        // 步骤 2: 单次锁定，批量插入
        {
            let mut wheel_guard = self.wheel.lock();
            wheel_guard.insert_batch(prepared_tasks);
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
            let _ = shutdown_tx.send(());
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
    command_rx: mpsc::Receiver<ServiceCommand>,
    /// Timeout sender (supports both one-shot and periodic task notifications)
    /// 
    /// 超时发送器（支持一次性和周期性任务通知）
    timeout_tx: mpsc::Sender<TaskNotification>,
    /// Actor shutdown signal receiver
    /// 
    /// Actor 关闭信号接收器
    shutdown_rx: tokio::sync::oneshot::Receiver<()>,
}

impl ServiceActor {
    /// Create new ServiceActor
    /// 
    /// 创建新的 ServiceActor
    fn new(command_rx: mpsc::Receiver<ServiceCommand>, timeout_tx: mpsc::Sender<TaskNotification>, shutdown_rx: tokio::sync::oneshot::Receiver<()>) -> Self {
        Self {
            command_rx,
            timeout_tx,
            shutdown_rx,
        }
    }

    /// Run Actor event loop
    /// 
    /// 运行 Actor 事件循环
    async fn run(mut self) {
        // Use separate FuturesUnordered for one-shot and periodic tasks
        // 为一次性任务和周期性任务使用独立的 FuturesUnordered
        
        // One-shot futures: each future returns (TaskId, Result<OneShotTaskCompletion>)
        // 一次性任务 futures：每个 future 返回 (TaskId, Result<OneShotTaskCompletion>)
        let mut oneshot_futures: FuturesUnordered<BoxFuture<'static, (TaskId, Result<TaskCompletion, tokio::sync::oneshot::error::RecvError>)>> = FuturesUnordered::new();

        // Periodic futures: each future returns (TaskId, Option<PeriodicTaskCompletion>, mpsc::Receiver)
        // The receiver is returned so we can continue listening for next event
        // 周期性任务 futures：每个 future 返回 (TaskId, Option<PeriodicTaskCompletion>, mpsc::Receiver)
        // 返回接收器以便我们可以继续监听下一个事件
        type PeriodicFutureResult = (TaskId, Option<TaskCompletion>, crate::task::PeriodicCompletionReceiver);
        let mut periodic_futures: FuturesUnordered<BoxFuture<'static, PeriodicFutureResult>> = FuturesUnordered::new();

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
                Some((task_id, result)) = oneshot_futures.next() => {
                    // Check completion reason, only forward Called events, do not forward Cancelled events
                    // 检查完成原因，只转发 Called 事件，不转发 Cancelled 事件
                    if let Ok(TaskCompletion::Called) = result {
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
                    // 如果是 Cancelled 或 None，不重新添加 future（任务结束）
                }
                
                // Listen to commands
                // 监听命令
                Some(cmd) = self.command_rx.recv() => {
                    match cmd {
                        ServiceCommand::AddBatchHandle { task_ids, completion_rxs } => {
                            // Add all tasks to appropriate futures
                            // 将所有任务添加到相应的 futures
                            for (task_id, rx) in task_ids.into_iter().zip(completion_rxs.into_iter()) {
                                match rx {
                                    crate::task::CompletionReceiver::OneShot(receiver) => {
                                        let future: BoxFuture<'static, (TaskId, Result<TaskCompletion, tokio::sync::oneshot::error::RecvError>)> = Box::pin(async move {
                                            (task_id, receiver.0.await)
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
                                    let future: BoxFuture<'static, (TaskId, Result<TaskCompletion, tokio::sync::oneshot::error::RecvError>)> = Box::pin(async move {
                                        (task_id, receiver.0.await)
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
    use crate::{TimerWheel, TimerTask};
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;
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

        // Create single timer (创建单个定时器)
        let task = TimerTask::new_oneshot(Duration::from_millis(50), Some(CallbackWrapper::new(|| async {})));
        let task_id = task.get_id();
        
        // Register to service (注册到服务)
        service.register(task).unwrap();

        // Receive timeout notification (接收超时通知)
        let mut rx = service.take_receiver().unwrap();
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
        let task1 = TimerTask::new_oneshot(Duration::from_secs(10), None);
        let task2 = TimerTask::new_oneshot(Duration::from_secs(10), None);
        service.register(task1).unwrap();
        service.register(task2).unwrap();

        // Immediately shutdown (without waiting for timers to trigger) (立即关闭（不等待定时器触发）)
        service.shutdown().await;
    }



    #[tokio::test]
    async fn test_cancel_task() {
        let timer = TimerWheel::with_defaults();
        let service = timer.create_service(ServiceConfig::default());

        // Add a long-term timer (添加一个长期定时器)
        let task = TimerTask::new_oneshot(Duration::from_secs(10), None);
        let task_id = task.get_id();
        
        service.register(task).unwrap();

        // Cancel task (取消任务)
        let cancelled = service.cancel_task(task_id);
        assert!(cancelled, "Task should be cancelled successfully");

        // Try to cancel the same task again, should return false (再次尝试取消同一任务，应返回 false)
        let cancelled_again = service.cancel_task(task_id);
        assert!(!cancelled_again, "Task should not exist anymore");
    }

    #[tokio::test]
    async fn test_cancel_nonexistent_task() {
        let timer = TimerWheel::with_defaults();
        let service = timer.create_service(ServiceConfig::default());

        // Add a timer to initialize service (添加定时器以初始化服务)
        let task = TimerTask::new_oneshot(Duration::from_millis(50), None);
        service.register(task).unwrap();

        // Try to cancel a nonexistent task (create a task ID that will not actually be registered)
        // 尝试取消不存在的任务（创建一个实际不会注册的任务 ID）
        let fake_task = TimerTask::new_oneshot(Duration::from_millis(50), None);
        let fake_task_id = fake_task.get_id();
        // Do not register fake_task (不注册 fake_task)
        let cancelled = service.cancel_task(fake_task_id);
        assert!(!cancelled, "Nonexistent task should not be cancelled");
    }


    #[tokio::test]
    async fn test_task_timeout_cleans_up_task_sender() {
        let timer = TimerWheel::with_defaults();
        let mut service = timer.create_service(ServiceConfig::default());

        // Add a short-term timer (添加短期定时器)
        let task = TimerTask::new_oneshot(Duration::from_millis(50), None);
        let task_id = task.get_id();
        
        service.register(task).unwrap();

        // Wait for task timeout (等待任务超时)
        let mut rx = service.take_receiver().unwrap();
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
    async fn test_cancel_task_spawns_background_task() {
        let timer = TimerWheel::with_defaults();
        let service = timer.create_service(ServiceConfig::default());
        let counter = Arc::new(AtomicU32::new(0));

        // Create a timer (创建定时器)
        let counter_clone = Arc::clone(&counter);
        let task = TimerTask::new_oneshot(
            Duration::from_secs(10),
            Some(CallbackWrapper::new(move || {
                let counter = Arc::clone(&counter_clone);
                async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                }
            })),
        );
        let task_id = task.get_id();
        
        service.register(task).unwrap();

        // Use cancel_task (will wait for result, but processed in background coroutine)
        // 使用 cancel_task（将等待结果，但在后台协程中处理）
        let cancelled = service.cancel_task(task_id);
        assert!(cancelled, "Task should be cancelled successfully");

        // Wait long enough to ensure callback is not executed (等待足够长时间以确保回调未执行)
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(counter.load(Ordering::SeqCst), 0, "Callback should not have been executed");

        // Verify task has been removed from active_tasks (验证任务已从 active_tasks 中移除)
        let cancelled_again = service.cancel_task(task_id);
        assert!(!cancelled_again, "Task should have been removed from active_tasks");
    }

    #[tokio::test]
    async fn test_schedule_once_direct() {
        let timer = TimerWheel::with_defaults();
        let mut service = timer.create_service(ServiceConfig::default());
        let counter = Arc::new(AtomicU32::new(0));

        // Schedule timer directly through service
        // 直接通过服务调度定时器
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
        let task_id = task.get_id();
        service.register(task).unwrap();

        // Wait for timer to trigger
        // 等待定时器触发
        let mut rx = service.take_receiver().unwrap();
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
    async fn test_schedule_once_batch_direct() {
        let timer = TimerWheel::with_defaults();
        let mut service = timer.create_service(ServiceConfig::default());
        let counter = Arc::new(AtomicU32::new(0));

        // Schedule timers directly through service
        // 直接通过服务调度定时器
        let tasks: Vec<_> = (0..3)
            .map(|_| {
                let counter = Arc::clone(&counter);
                TimerTask::new_oneshot(Duration::from_millis(50), Some(CallbackWrapper::new(move || {
                    let counter = Arc::clone(&counter);
                    async move {
                        counter.fetch_add(1, Ordering::SeqCst);
                    }
                    })))
            })
            .collect();

        assert_eq!(tasks.len(), 3);
        service.register_batch(tasks).unwrap();

        // Receive all timeout notifications
        // 接收所有超时通知
        let mut received_count = 0;
        let mut rx = service.take_receiver().unwrap();
        
        while received_count < 3 {
            match tokio::time::timeout(Duration::from_millis(200), rx.recv()).await {
                Ok(Some(_task_id)) => {
                    received_count += 1;
                }
                Ok(None) => break,
                Err(_) => break,
            }
        }

        assert_eq!(received_count, 3);
        
        // Wait for callback to execute
        // 等待回调执行
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(counter.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn test_schedule_once_notify_direct() {
        let timer = TimerWheel::with_defaults();
        let mut service = timer.create_service(ServiceConfig::default());

        // Schedule only notification timer directly through service (no callback)
        // 直接通过服务调度通知定时器（没有回调）
        let task = TimerTask::new_oneshot(Duration::from_millis(50), None);
        let task_id = task.get_id();
        service.register(task).unwrap();

        // Receive timeout notification
        // 接收超时通知
        let mut rx = service.take_receiver().unwrap();
        let received_notification = tokio::time::timeout(Duration::from_millis(200), rx.recv())
            .await
            .expect("Should receive timeout notification")
            .expect("Should receive Some value");

        assert_eq!(received_notification, TaskNotification::OneShot(task_id));
    }

    #[tokio::test]
    async fn test_schedule_and_cancel_direct() {
        let timer = TimerWheel::with_defaults();
        let service = timer.create_service(ServiceConfig::default());
        let counter = Arc::new(AtomicU32::new(0));

        // Schedule timer directly
        // 直接调度定时器
        let counter_clone = Arc::clone(&counter);
        let task = TimerTask::new_oneshot(
            Duration::from_secs(10),
            Some(CallbackWrapper::new(move || {
                let counter = Arc::clone(&counter_clone);
                async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                }
            })),
        );
        let task_id = task.get_id();
        service.register(task).unwrap();

        // Immediately cancel
        // 立即取消
        let cancelled = service.cancel_task(task_id);
        assert!(cancelled, "Task should be cancelled successfully");

        // Wait to ensure callback is not executed
        // 等待确保回调未执行
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(counter.load(Ordering::SeqCst), 0, "Callback should not have been executed");
    }

    #[tokio::test]
    async fn test_cancel_batch_direct() {
        let timer = TimerWheel::with_defaults();
        let service = timer.create_service(ServiceConfig::default());
        let counter = Arc::new(AtomicU32::new(0));

        // Batch schedule timers
        // 批量调度定时器
        let tasks: Vec<_> = (0..10)
            .map(|_| {
                let counter = Arc::clone(&counter);
                TimerTask::new_oneshot(Duration::from_secs(10), Some(CallbackWrapper::new(move || {
                    let counter = Arc::clone(&counter);
                    async move {
                        counter.fetch_add(1, Ordering::SeqCst);
                    }
                    })))
            })
            .collect();

        let task_ids: Vec<_> = tasks.iter().map(|t| t.get_id()).collect();
        assert_eq!(task_ids.len(), 10);
        service.register_batch(tasks).unwrap();

        // Batch cancel all tasks
        // 批量取消所有任务
        let cancelled = service.cancel_batch(&task_ids);
        assert_eq!(cancelled, 10, "All 10 tasks should be cancelled");

        // Wait to ensure callback is not executed
        // 等待确保回调未执行
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(counter.load(Ordering::SeqCst), 0, "No callbacks should have been executed");
    }

    #[tokio::test]
    async fn test_cancel_batch_partial() {
        let timer = TimerWheel::with_defaults();
        let service = timer.create_service(ServiceConfig::default());
        let counter = Arc::new(AtomicU32::new(0));

        // Batch schedule timers
        // 批量调度定时器
        let tasks: Vec<_> = (0..10)
            .map(|_| {
                let counter = Arc::clone(&counter);
                TimerTask::new_oneshot(Duration::from_secs(10), Some(CallbackWrapper::new(move || {
                    let counter = Arc::clone(&counter);
                    async move {
                        counter.fetch_add(1, Ordering::SeqCst);
                    }
                    })))
            })
            .collect();

        let task_ids: Vec<_> = tasks.iter().map(|t| t.get_id()).collect();
        service.register_batch(tasks).unwrap();

        // Only cancel first 5 tasks
        // 只取消前 5 个任务
        let to_cancel: Vec<_> = task_ids.iter().take(5).copied().collect();
        let cancelled = service.cancel_batch(&to_cancel);
        assert_eq!(cancelled, 5, "5 tasks should be cancelled");

        // Wait to ensure first 5 callbacks are not executed
        // 等待确保前 5 个回调未执行
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(counter.load(Ordering::SeqCst), 0, "Cancelled tasks should not execute");
    }

    #[tokio::test]
    async fn test_cancel_batch_empty() {
        let timer = TimerWheel::with_defaults();
        let service = timer.create_service(ServiceConfig::default());

        // Cancel empty list
        // 取消空列表
        let empty: Vec<TaskId> = vec![];
        let cancelled = service.cancel_batch(&empty);
        assert_eq!(cancelled, 0, "No tasks should be cancelled");
    }

    #[tokio::test]
    async fn test_postpone() {
        let timer = TimerWheel::with_defaults();
        let mut service = timer.create_service(ServiceConfig::default());
        let counter = Arc::new(AtomicU32::new(0));

        // Register a task, original callback increases 1
        // 注册一个任务，原始回调增加 1
        let counter_clone1 = Arc::clone(&counter);
        let task = TimerTask::new_oneshot(
            Duration::from_millis(50),
            Some(CallbackWrapper::new(move || {
                let counter = Arc::clone(&counter_clone1);
                async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                }
                })),
        );
        let task_id = task.get_id();
        service.register(task).unwrap();

        // Postpone task and replace callback, new callback increases 10
        // 延期任务并替换回调，新回调增加 10
        let counter_clone2 = Arc::clone(&counter);
        let postponed = service.postpone(
            task_id,
            Duration::from_millis(100),
            Some(CallbackWrapper::new(move || {
                let counter = Arc::clone(&counter_clone2);
                async move {
                    counter.fetch_add(10, Ordering::SeqCst);
                }
            }))
        );
        assert!(postponed, "Task should be postponed successfully");

        // Receive timeout notification (after postponing, need to wait 100ms, plus margin)
        // 接收超时通知（延期后，需要等待 100ms，加上余量）
        let mut rx = service.take_receiver().unwrap();
        let received_task_id = tokio::time::timeout(Duration::from_millis(200), rx.recv())
            .await
            .expect("Should receive timeout notification")
            .expect("Should receive Some value");

        assert_eq!(received_task_id, TaskNotification::OneShot(task_id));
        
        // Wait for callback to execute
        // 等待回调执行
        tokio::time::sleep(Duration::from_millis(20)).await;
        
        // Verify new callback is executed (increased 10 instead of 1)
        // 验证新回调已执行（增加 10 而不是 1）
        assert_eq!(counter.load(Ordering::SeqCst), 10);
    }

    #[tokio::test]
    async fn test_postpone_nonexistent_task() {
        let timer = TimerWheel::with_defaults();
        let service = timer.create_service(ServiceConfig::default());

        // Try to postpone a nonexistent task
        // 尝试延期一个不存在的任务
        let fake_task = TimerTask::new_oneshot(Duration::from_millis(50), None);
        let fake_task_id = fake_task.get_id();
        // Do not register this task
        // 不注册这个任务
        let postponed = service.postpone(fake_task_id, Duration::from_millis(100), None);
        assert!(!postponed, "Nonexistent task should not be postponed");
    }

    #[tokio::test]
    async fn test_postpone_batch() {
        let timer = TimerWheel::with_defaults();
        let mut service = timer.create_service(ServiceConfig::default());
        let counter = Arc::new(AtomicU32::new(0));

        // Register 3 tasks
        // 注册 3 个任务
        let mut task_ids = Vec::new();
        for _ in 0..3 {
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
            task_ids.push((task.get_id(), Duration::from_millis(150), None));
            service.register(task).unwrap();
        }

        // Batch postpone
        // 批量延期
        let postponed = service.postpone_batch_with_callbacks(task_ids);
        assert_eq!(postponed, 3, "All 3 tasks should be postponed");

        // Wait for original time 50ms, task should not trigger
        // 等待原始时间 50ms，任务应该不触发
        tokio::time::sleep(Duration::from_millis(70)).await;
        assert_eq!(counter.load(Ordering::SeqCst), 0);

        // Receive all timeout notifications
        // 接收所有超时通知
        let mut received_count = 0;
        let mut rx = service.take_receiver().unwrap();
        
        while received_count < 3 {
            match tokio::time::timeout(Duration::from_millis(200), rx.recv()).await {
                Ok(Some(_task_id)) => {
                    received_count += 1;
                }
                Ok(None) => break,
                Err(_) => break,
            }
        }

        assert_eq!(received_count, 3);
        
        // Wait for callback to execute
        // 等待回调执行
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert_eq!(counter.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn test_postpone_batch_with_callbacks() {
        let timer = TimerWheel::with_defaults();
        let mut service = timer.create_service(ServiceConfig::default());
        let counter = Arc::new(AtomicU32::new(0));

        // Register 3 tasks
        // 注册 3 个任务
        let mut task_ids = Vec::new();
        for _ in 0..3 {
            let task = TimerTask::new_oneshot(
                Duration::from_millis(50),
                None,
            );
            task_ids.push(task.get_id());
            service.register(task).unwrap();
        }

        // Batch postpone and replace callback
        // 批量延期并替换回调
        let updates: Vec<_> = task_ids
            .into_iter()
            .map(|id| {
                let counter_clone = Arc::clone(&counter);
                (id, Duration::from_millis(150), Some(CallbackWrapper::new(move || {
                    let counter = Arc::clone(&counter_clone);
                    async move {
                        counter.fetch_add(1, Ordering::SeqCst);
                    }
                    })))
            })
            .collect();

        let postponed = service.postpone_batch_with_callbacks(updates);
        assert_eq!(postponed, 3, "All 3 tasks should be postponed");

        // Wait for original time 50ms, task should not trigger
        // 等待原始时间 50ms，任务应该不触发
        tokio::time::sleep(Duration::from_millis(70)).await;
        assert_eq!(counter.load(Ordering::SeqCst), 0);

        // Receive all timeout notifications
        // 接收所有超时通知
        let mut received_count = 0;
        let mut rx = service.take_receiver().unwrap();
        
        while received_count < 3 {
            match tokio::time::timeout(Duration::from_millis(200), rx.recv()).await {
                Ok(Some(_task_id)) => {
                    received_count += 1;
                }
                Ok(None) => break,
                Err(_) => break,
            }
        }

        assert_eq!(received_count, 3);
        
        // Wait for callback to execute
        // 等待回调执行
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert_eq!(counter.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn test_postpone_batch_empty() {
        let timer = TimerWheel::with_defaults();
        let service = timer.create_service(ServiceConfig::default());

        // Postpone empty list
        let empty: Vec<(TaskId, Duration, Option<CallbackWrapper>)> = vec![];
        let postponed = service.postpone_batch_with_callbacks(empty);
        assert_eq!(postponed, 0, "No tasks should be postponed");
    }

    #[tokio::test]
    async fn test_postpone_keeps_timeout_notification_valid() {
        let timer = TimerWheel::with_defaults();
        let mut service = timer.create_service(ServiceConfig::default());
        let counter = Arc::new(AtomicU32::new(0));

        // Register a task
        // 注册一个任务
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
        let task_id = task.get_id();
        service.register(task).unwrap();

        // Postpone task
        // 延期任务
        service.postpone(task_id, Duration::from_millis(100), None);

        // Verify timeout notification is still valid (after postponing, need to wait 100ms, plus margin)
        // 验证超时通知是否仍然有效（延期后，需要等待 100ms，加上余量）
        let mut rx = service.take_receiver().unwrap();
        let received_task_id = tokio::time::timeout(Duration::from_millis(200), rx.recv())
            .await
            .expect("Should receive timeout notification")
            .expect("Should receive Some value");

        assert_eq!(received_task_id, TaskNotification::OneShot(task_id), "Timeout notification should still work after postpone");
        
        // Wait for callback to execute
        // 等待回调执行
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_cancelled_task_not_forwarded_to_timeout_rx() {
        let timer = TimerWheel::with_defaults();
        let mut service = timer.create_service(ServiceConfig::default());

        // Register two tasks: one will be cancelled, one will expire normally
        // 注册两个任务：一个将被取消，一个将正常过期
        let task1 = TimerTask::new_oneshot(Duration::from_secs(10), None);
        let task1_id = task1.get_id();
        service.register(task1).unwrap();

        let task2 = TimerTask::new_oneshot(Duration::from_millis(50), None);
        let task2_id = task2.get_id();
        service.register(task2).unwrap();

        // Cancel first task
        // 取消第一个任务
        let cancelled = service.cancel_task(task1_id);
        assert!(cancelled, "Task should be cancelled");

        // Wait for second task to expire
        // 等待第二个任务过期
        let mut rx = service.take_receiver().unwrap();
        let received_notification = tokio::time::timeout(Duration::from_millis(200), rx.recv())
            .await
            .expect("Should receive timeout notification")
            .expect("Should receive Some value");

        // Should only receive notification for second task (expired), not for first task (cancelled)
        assert_eq!(received_notification, TaskNotification::OneShot(task2_id), "Should only receive expired task notification");

        // Verify no other notifications (especially cancelled tasks should not have notifications)
        let no_more = tokio::time::timeout(Duration::from_millis(50), rx.recv()).await;
        assert!(no_more.is_err(), "Should not receive any more notifications");
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

    #[tokio::test]
    async fn test_postpone_batch_without_callbacks() {
        let timer = TimerWheel::with_defaults();
        let mut service = timer.create_service(ServiceConfig::default());
        let counter = Arc::new(AtomicU32::new(0));

        // Register 3 tasks, with original callback
        // 注册 3 个任务，带有原始回调
        let mut task_ids = Vec::new();
        for _ in 0..3 {
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
            task_ids.push(task.get_id());
            service.register(task).unwrap();
        }

        // Batch postpone, without replacing callback
        // 批量延期，不替换回调
        let updates: Vec<_> = task_ids
            .iter()
            .map(|&id| (id, Duration::from_millis(150)))
            .collect();
        let postponed = service.postpone_batch(updates);
        assert_eq!(postponed, 3, "All 3 tasks should be postponed");

        // Wait for original time 50ms, task should not trigger
        // 等待原始时间 50ms，任务应该不触发
        tokio::time::sleep(Duration::from_millis(70)).await;
        assert_eq!(counter.load(Ordering::SeqCst), 0, "Callbacks should not fire yet");

        // Receive all timeout notifications
        // 接收所有超时通知
        let mut received_count = 0;
        let mut rx = service.take_receiver().unwrap();
        
        while received_count < 3 {
            match tokio::time::timeout(Duration::from_millis(200), rx.recv()).await {
                Ok(Some(_task_id)) => {
                    received_count += 1;
                }
                Ok(None) => break,
                Err(_) => break,
            }
        }

        assert_eq!(received_count, 3, "Should receive 3 timeout notifications");
        
        // Wait for callback to execute
        // 等待回调执行
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert_eq!(counter.load(Ordering::SeqCst), 3, "All callbacks should execute");
    }

    #[tokio::test]
    async fn test_periodic_task_basic() {
        let timer = TimerWheel::with_defaults();
        let mut service = timer.create_service(ServiceConfig::default());
        let counter = Arc::new(AtomicU32::new(0));

        // Register a periodic task with 50ms interval (注册一个 50ms 间隔的周期性任务)
        let counter_clone = Arc::clone(&counter);
        let task = TimerTask::new_periodic(
            Duration::from_millis(30),  // initial delay (初始延迟)
            Duration::from_millis(50),  // interval (间隔)
            Some(CallbackWrapper::new(move || {
                let counter = Arc::clone(&counter_clone);
                async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                }
            })),
            None,
        );
        let task_id = task.get_id();
        service.register(task).unwrap();

        // Receive periodic notifications (接收周期性通知)
        let mut rx = service.take_receiver().unwrap();
        let mut notification_count = 0;

        // Receive first 3 notifications (接收前 3 个通知)
        while notification_count < 3 {
            match tokio::time::timeout(Duration::from_millis(200), rx.recv()).await {
                Ok(Some(notification)) => {
                    match notification {
                        TaskNotification::Periodic(id) => {
                            assert_eq!(id, task_id, "Should receive notification for correct task");
                            notification_count += 1;
                        }
                        _ => panic!("Expected periodic notification"),
                    }
                }
                Ok(None) => break,
                Err(_) => panic!("Timeout waiting for periodic notification"),
            }
        }

        assert_eq!(notification_count, 3, "Should receive 3 periodic notifications");
        
        // Wait for callback to execute
        // 等待回调执行
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert_eq!(counter.load(Ordering::SeqCst), 3, "Callback should execute 3 times");

        // Cancel the periodic task (取消周期性任务)
        let cancelled = service.cancel_task(task_id);
        assert!(cancelled, "Should be able to cancel periodic task");
    }

    #[tokio::test]
    async fn test_periodic_task_cancel_no_notification() {
        let timer = TimerWheel::with_defaults();
        let mut service = timer.create_service(ServiceConfig::default());

        // Register a periodic task (注册周期性任务)
        let task = TimerTask::new_periodic(
            Duration::from_millis(30),
            Duration::from_millis(50),
            None,
            None,
        );
        let task_id = task.get_id();
        service.register(task).unwrap();

        // Wait for first notification (等待第一个通知)
        let mut rx = service.take_receiver().unwrap();
        let notification = tokio::time::timeout(Duration::from_millis(100), rx.recv())
            .await
            .expect("Should receive first notification")
            .expect("Should receive Some value");
        
        assert_eq!(notification, TaskNotification::Periodic(task_id));

        // Cancel the task (取消任务)
        let cancelled = service.cancel_task(task_id);
        assert!(cancelled, "Should be able to cancel task");

        // Should not receive cancelled notification (不应该接收到取消通知)
        match tokio::time::timeout(Duration::from_millis(100), rx.recv()).await {
            Ok(Some(_)) => panic!("Should not receive cancelled notification"),
            Ok(None) | Err(_) => {} // Expected: timeout or channel closed
        }
    }

    #[tokio::test]
    async fn test_mixed_oneshot_and_periodic_tasks() {
        let timer = TimerWheel::with_defaults();
        let mut service = timer.create_service(ServiceConfig::default());

        // Register one-shot tasks (注册一次性任务)
        let oneshot_task = TimerTask::new_oneshot(Duration::from_millis(50), None);
        let oneshot_id = oneshot_task.get_id();
        service.register(oneshot_task).unwrap();

        // Register periodic task (注册周期性任务)
        let periodic_task = TimerTask::new_periodic(
            Duration::from_millis(30),
            Duration::from_millis(40),
            None,
            None,
        );
        let periodic_id = periodic_task.get_id();
        service.register(periodic_task).unwrap();

        // Receive notifications (接收通知)
        let mut rx = service.take_receiver().unwrap();
        let mut oneshot_received = false;
        let mut periodic_count = 0;

        // Receive notifications for a while (接收一段时间的通知)
        let start = tokio::time::Instant::now();
        while start.elapsed() < Duration::from_millis(200) {
            match tokio::time::timeout(Duration::from_millis(100), rx.recv()).await {
                Ok(Some(notification)) => {
                    match notification {
                        TaskNotification::OneShot(id) => {
                            assert_eq!(id, oneshot_id, "Should be one-shot task");
                            oneshot_received = true;
                        }
                        TaskNotification::Periodic(id) => {
                            assert_eq!(id, periodic_id, "Should be periodic task");
                            periodic_count += 1;
                        }
                    }
                }
                Ok(None) => break,
                Err(_) => break,
            }
        }

        assert!(oneshot_received, "Should receive one-shot notification");
        assert!(periodic_count >= 2, "Should receive at least 2 periodic notifications");

        // Cancel periodic task (取消周期性任务)
        service.cancel_task(periodic_id);
    }

    #[tokio::test]
    async fn test_periodic_task_batch_register() {
        let timer = TimerWheel::with_defaults();
        let mut service = timer.create_service(ServiceConfig::default());
        let counter = Arc::new(AtomicU32::new(0));

        // Register multiple periodic tasks in batch (批量注册多个周期性任务)
        let tasks: Vec<_> = (0..3)
            .map(|_| {
                let counter = Arc::clone(&counter);
                TimerTask::new_periodic(
                    Duration::from_millis(30),
                    Duration::from_millis(50),
                    Some(CallbackWrapper::new(move || {
                        let counter = Arc::clone(&counter);
                        async move {
                            counter.fetch_add(1, Ordering::SeqCst);
                        }
                    })),
                    None,
                )
            })
            .collect();
        
        let task_ids: Vec<_> = tasks.iter().map(|t| t.get_id()).collect();
        service.register_batch(tasks).unwrap();

        // Receive notifications (接收通知)
        let mut rx = service.take_receiver().unwrap();
        let mut notification_counts = std::collections::HashMap::new();

        // Receive notifications for a while (接收一段时间的通知)
        let start = tokio::time::Instant::now();
        while start.elapsed() < Duration::from_millis(180) {
            match tokio::time::timeout(Duration::from_millis(100), rx.recv()).await {
                Ok(Some(TaskNotification::Periodic(id))) => {
                    *notification_counts.entry(id).or_insert(0) += 1;
                }
                Ok(Some(_)) => panic!("Expected periodic notification"),
                Ok(None) => break,
                Err(_) => break,
            }
        }

        // Each task should receive at least 2 notifications (每个任务应该至少收到 2 个通知)
        for task_id in &task_ids {
            let count = notification_counts.get(task_id).copied().unwrap_or(0);
            assert!(count >= 2, "Task {:?} should receive at least 2 notifications, got {}", task_id, count);
        }

        // Wait for callbacks to execute
        // 等待回调执行
        tokio::time::sleep(Duration::from_millis(20)).await;
        let total_callbacks = counter.load(Ordering::SeqCst);
        assert!(total_callbacks >= 6, "Should have at least 6 callback executions (3 tasks * 2), got {}", total_callbacks);

        // Cancel all periodic tasks (取消所有周期性任务)
        let cancelled = service.cancel_batch(&task_ids);
        assert_eq!(cancelled, 3, "Should cancel all 3 tasks");
    }
}

