use crate::config::{BatchConfig, ServiceConfig, WheelConfig};
use crate::task::{CallbackWrapper, TaskId, TaskCompletionReason};
use crate::wheel::Wheel;
use parking_lot::Mutex;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

/// Completion receiver for receiving timer completion notifications
/// 
/// 完成通知接收器，用于接收定时器完成通知
pub struct CompletionReceiver(pub oneshot::Receiver<TaskCompletionReason>);

/// Timer handle for managing timer lifecycle
/// 
/// Note: This type does not implement Clone to prevent duplicate cancellation of the same timer. Each timer should have only one owner.
/// 
/// 定时器句柄，用于管理定时器生命周期
/// 
/// 注意：此类型未实现 Clone 以防止重复取消同一定时器。每个定时器应该只有一个所有者。
pub struct TimerHandle {
    pub(crate) task_id: TaskId,
    pub(crate) wheel: Arc<Mutex<Wheel>>,
    pub(crate) completion_rx: CompletionReceiver,
}

impl TimerHandle {
    pub(crate) fn new(task_id: TaskId, wheel: Arc<Mutex<Wheel>>, completion_rx: oneshot::Receiver<TaskCompletionReason>) -> Self {
        Self { task_id, wheel, completion_rx: CompletionReceiver(completion_rx) }
    }

    /// Cancel the timer
    ///
    /// # Returns
    /// Returns true if task exists and is successfully cancelled, otherwise false
    /// 
    /// 取消定时器
    ///
    /// # 返回值
    /// 如果任务存在且成功取消则返回 true，否则返回 false
    ///
    /// # Examples (示例)
    /// ```no_run
    /// # use kestrel_timer::{TimerWheel, CallbackWrapper};
    /// # use std::time::Duration;
    /// # 
    /// # #[tokio::main]
    /// # async fn main() {
    /// let timer = TimerWheel::with_defaults();
    /// let callback = Some(CallbackWrapper::new(|| async {}));
    /// let task = TimerWheel::create_task(Duration::from_secs(1), callback);
    /// let handle = timer.register(task);
    /// 
    /// // Cancel the timer
    /// let success = handle.cancel();
    /// println!("Canceled successfully: {}", success);
    /// # }
    /// ```
    pub fn cancel(&self) -> bool {
        let mut wheel = self.wheel.lock();
        wheel.cancel(self.task_id)
    }

    /// Get mutable reference to completion receiver
    /// 
    /// 获取完成通知接收器的可变引用
    ///
    /// # Examples (示例)
    /// ```no_run
    /// # use kestrel_timer::{TimerWheel, CallbackWrapper};
    /// # use std::time::Duration;
    /// # 
    /// # #[tokio::main]
    /// # async fn main() {
    /// let timer = TimerWheel::with_defaults();
    /// let callback = Some(CallbackWrapper::new(|| async {
    ///     println!("Timer fired!");
    /// }));
    /// let task = TimerWheel::create_task(Duration::from_secs(1), callback);
    /// let handle = timer.register(task);
    /// 
    /// // Wait for timer completion (consume handle using into_completion_receiver)
    /// // 等待定时器完成（使用 into_completion_receiver 消费句柄）
    /// handle.into_completion_receiver().0.await.ok();
    /// println!("Timer completed!");
    /// # }
    /// ```
    pub fn completion_receiver(&mut self) -> &mut CompletionReceiver {
        &mut self.completion_rx
    }

    /// Consume handle and return completion receiver
    /// 
    /// 消费句柄并返回完成通知接收器
    ///
    /// # Examples (示例)
    /// ```no_run
    /// # use kestrel_timer::{TimerWheel, CallbackWrapper};
    /// # use std::time::Duration;
    /// # 
    /// # #[tokio::main]
    /// # async fn main() {
    /// let timer = TimerWheel::with_defaults();
    /// let callback = Some(CallbackWrapper::new(|| async {
    ///     println!("Timer fired!");
    /// }));
    /// let task = TimerWheel::create_task(Duration::from_secs(1), callback);
    /// let handle = timer.register(task);
    /// 
    /// // Wait for timer completion
    /// handle.into_completion_receiver().0.await.ok();
    /// println!("Timer completed!");
    /// # }
    /// ```
    pub fn into_completion_receiver(self) -> CompletionReceiver {
        self.completion_rx
    }
}

/// Batch timer handle for managing batch-scheduled timers
/// 
/// Note: This type does not implement Clone to prevent duplicate cancellation of the same batch of timers. Use `into_iter()` or `into_handles()` to access individual timer handles.
/// 
/// 批量定时器句柄，用于管理批量调度的定时器
/// 
/// 注意：此类型未实现 Clone 以防止重复取消同一批定时器。使用 `into_iter()` 或 `into_handles()` 访问单个定时器句柄。
pub struct BatchHandle {
    pub(crate) task_ids: Vec<TaskId>,
    pub(crate) wheel: Arc<Mutex<Wheel>>,
    pub(crate) completion_rxs: Vec<oneshot::Receiver<TaskCompletionReason>>,
}

impl BatchHandle {
    pub(crate) fn new(task_ids: Vec<TaskId>, wheel: Arc<Mutex<Wheel>>, completion_rxs: Vec<oneshot::Receiver<TaskCompletionReason>>) -> Self {
        Self { task_ids, wheel, completion_rxs }
    }

    /// Cancel all timers in batch
    ///
    /// # Returns
    /// Number of successfully cancelled tasks
    /// 
    /// 批量取消所有定时器
    ///
    /// # 返回值
    /// 成功取消的任务数量
    ///
    /// # Examples (示例)
    /// ```no_run
    /// # use kestrel_timer::{TimerWheel, CallbackWrapper};
    /// # use std::time::Duration;
    /// # 
    /// # #[tokio::main]
    /// # async fn main() {
    /// let timer = TimerWheel::with_defaults();
    /// let delays: Vec<Duration> = (0..10)
    ///     .map(|_| Duration::from_secs(1))
    ///     .collect();
    /// let tasks = TimerWheel::create_batch(delays);
    /// let batch = timer.register_batch(tasks);
    /// 
    /// let cancelled = batch.cancel_all();
    /// println!("Canceled {} timers", cancelled);
    /// # }
    /// ```
    pub fn cancel_all(self) -> usize {
        let mut wheel = self.wheel.lock();
        wheel.cancel_batch(&self.task_ids)
    }

    /// Convert batch handle to Vec of individual timer handles
    ///
    /// Consumes BatchHandle and creates independent TimerHandle for each task
    /// 
    /// 将批量句柄转换为单个定时器句柄的 Vec
    ///
    /// 消费 BatchHandle 并为每个任务创建独立的 TimerHandle
    ///
    /// # Examples (示例)
    /// ```no_run
    /// # use kestrel_timer::{TimerWheel, CallbackWrapper};
    /// # use std::time::Duration;
    /// # 
    /// # #[tokio::main]
    /// # async fn main() {
    /// let timer = TimerWheel::with_defaults();
    /// let delays: Vec<Duration> = (0..3)
    ///     .map(|_| Duration::from_secs(1))
    ///     .collect();
    /// let tasks = TimerWheel::create_batch(delays);
    /// let batch = timer.register_batch(tasks);
    /// 
    /// // Convert to individual handles
    /// // 转换为单个句柄
    /// let handles = batch.into_handles();
    /// for handle in handles {
    ///     // Can operate each handle individually
    ///     // 可以单独操作每个句柄
    /// }
    /// # }
    /// ```
    pub fn into_handles(self) -> Vec<TimerHandle> {
        self.task_ids
            .into_iter()
            .zip(self.completion_rxs.into_iter())
            .map(|(task_id, rx)| {
                TimerHandle::new(task_id, self.wheel.clone(), rx)
            })
            .collect()
    }

    /// Get the number of batch tasks
    /// 
    /// 获取批量任务数量
    pub fn len(&self) -> usize {
        self.task_ids.len()
    }

    /// Check if batch tasks are empty
    /// 
    /// 检查批量任务是否为空
    pub fn is_empty(&self) -> bool {
        self.task_ids.is_empty()
    }

    /// Get reference to all task IDs
    /// 
    /// 获取所有任务 ID 的引用
    pub fn task_ids(&self) -> &[TaskId] {
        &self.task_ids
    }

    /// Get reference to all completion receivers
    ///
    /// # Returns
    /// Reference to list of completion receivers for all tasks
    /// 
    /// 获取所有完成通知接收器的引用
    ///
    /// # 返回值
    /// 所有任务完成通知接收器列表的引用
    pub fn completion_receivers(&mut self) -> &mut Vec<oneshot::Receiver<TaskCompletionReason>> {
        &mut self.completion_rxs
    }

    /// Consume handle and return all completion receivers
    ///
    /// # Returns
    /// List of completion receivers for all tasks
    /// 
    /// 消费句柄并返回所有完成通知接收器
    ///
    /// # 返回值
    /// 所有任务的完成通知接收器列表
    ///
    /// # Examples (示例)
    /// ```no_run
    /// # use kestrel_timer::{TimerWheel, CallbackWrapper};
    /// # use std::time::Duration;
    /// # 
    /// # #[tokio::main]
    /// # async fn main() {
    /// let timer = TimerWheel::with_defaults();
    /// let delays: Vec<Duration> = (0..3)
    ///     .map(|_| Duration::from_secs(1))
    ///     .collect();
    /// let tasks = TimerWheel::create_batch(delays);
    /// let batch = timer.register_batch(tasks);
    /// 
    /// // Get all completion receivers
    /// // 获取所有完成通知接收器
    /// let receivers = batch.into_completion_receivers();
    /// for rx in receivers {
    ///     tokio::spawn(async move {
    ///         if rx.await.is_ok() {
    ///             println!("A timer completed!");
    ///         }
    ///     });
    /// }
    /// # }
    /// ```
    pub fn into_completion_receivers(self) -> Vec<oneshot::Receiver<TaskCompletionReason>> {
        self.completion_rxs
    }
}

/// Implement IntoIterator to allow direct iteration over BatchHandle
/// 
/// 实现 IntoIterator 以允许直接迭代 BatchHandle
/// 
/// # Examples (示例)
/// ```no_run
/// # use kestrel_timer::{TimerWheel, CallbackWrapper};
/// # use std::time::Duration;
/// # 
/// # #[tokio::main]
/// # async fn main() {
/// let timer = TimerWheel::with_defaults();
/// let delays: Vec<Duration> = (0..3)
///     .map(|_| Duration::from_secs(1))
///     .collect();
/// let tasks = TimerWheel::create_batch(delays);
/// let batch = timer.register_batch(tasks);
/// 
/// // Iterate directly, each element is an independent TimerHandle
/// // 直接迭代，每个元素是一个独立的 TimerHandle
/// for handle in batch {
///     // Can operate each handle individually
///     // 可以单独操作每个句柄
/// }
/// # }
/// ```
impl IntoIterator for BatchHandle {
    type Item = TimerHandle;
    type IntoIter = BatchHandleIter;

    fn into_iter(self) -> Self::IntoIter {
        BatchHandleIter {
            task_ids: self.task_ids.into_iter(),
            completion_rxs: self.completion_rxs.into_iter(),
            wheel: self.wheel,
        }
    }
}

/// Iterator for BatchHandle
/// 
/// BatchHandle 的迭代器
pub struct BatchHandleIter {
    task_ids: std::vec::IntoIter<TaskId>,
    completion_rxs: std::vec::IntoIter<oneshot::Receiver<TaskCompletionReason>>,
    wheel: Arc<Mutex<Wheel>>,
}

impl Iterator for BatchHandleIter {
    type Item = TimerHandle;

    fn next(&mut self) -> Option<Self::Item> {
        match (self.task_ids.next(), self.completion_rxs.next()) {
            (Some(task_id), Some(rx)) => {
                Some(TimerHandle::new(task_id, self.wheel.clone(), rx))
            }
            _ => None,
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.task_ids.size_hint()
    }
}

impl ExactSizeIterator for BatchHandleIter {
    fn len(&self) -> usize {
        self.task_ids.len()
    }
}

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
    /// 
    /// 创建新的定时器管理器
    ///
    /// # 参数
    /// - `config`: 时间轮配置，已验证
    ///
    /// # Examples (示例)
    /// ```no_run
    /// use kestrel_timer::{TimerWheel, WheelConfig, TimerTask, BatchConfig};
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
    ///     // Use two-step API
    ///     // (Use two-step API)
    ///     let task = TimerWheel::create_task(Duration::from_secs(1), None);
    ///     let handle = timer.register(task);
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
    /// # Parameters
    /// - `config`: Timing wheel configuration, already validated
    ///
    /// # Returns
    /// Timer manager instance
    /// 
    /// 使用默认配置创建定时器管理器，分层模式
    /// - L0 层 tick 持续时间：10ms，槽数量：512
    /// - L1 层 tick 持续时间：1s，槽数量：64
    ///
    /// # 参数
    /// - `config`: 时间轮配置，已验证
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
    /// use kestrel_timer::{TimerWheel, TimerService, CallbackWrapper, ServiceConfig};
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
    ///     let callbacks: Vec<(Duration, Option<CallbackWrapper>)> = (0..5)
    ///         .map(|_| (Duration::from_millis(100), Some(CallbackWrapper::new(|| async {}))))
    ///         .collect();
    ///     let tasks = TimerService::create_batch_with_callbacks(callbacks);
    ///     service.register_batch(tasks).unwrap();
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
    /// use kestrel_timer::{TimerWheel, ServiceConfig};
    ///
    /// #[tokio::main]
    /// async fn main() {
    ///     let timer = TimerWheel::with_defaults();
    ///     let config = ServiceConfig::builder()
    ///         .command_channel_capacity(1024)
    ///         .timeout_channel_capacity(2000)
    ///         .build()
    ///         .unwrap();
    ///     let service = timer.create_service_with_config(config);
    /// }
    /// ```
    pub fn create_service_with_config(&self, config: ServiceConfig) -> crate::service::TimerService {
        crate::service::TimerService::new(self.wheel.clone(), config)
    }

    /// Create timer task (static method, apply stage)
    /// 
    /// # Parameters
    /// - `delay`: Delay duration
    /// - `callback`: Callback object implementing TimerCallback trait
    /// 
    /// # Returns
    /// Return TimerTask, needs to be registered through `register()`
    /// 
    /// 创建定时器任务 (静态方法，应用阶段)
    /// 
    /// # 参数
    /// - `delay`: 延迟时间
    /// - `callback`: 回调对象，实现 TimerCallback 特质
    /// 
    /// # 返回值
    /// 返回 TimerTask，需要通过 `register()` 注册
    /// 
    /// # Examples (示例)
    /// ```no_run
    /// use kestrel_timer::{TimerWheel, TimerTask, CallbackWrapper};
    /// use std::time::Duration;
    /// 
    /// 
    /// #[tokio::main]
    /// async fn main() {
    ///     let timer = TimerWheel::with_defaults();
    ///     
    ///     // Step 1: Create task
    ///     // 创建任务
    ///     let task = TimerWheel::create_task(Duration::from_secs(1), Some(CallbackWrapper::new(|| async {
    ///         println!("Timer fired!");
    ///     })));
    ///     
    ///     // Get task ID (获取任务 ID)
    ///     let task_id = task.get_id();
    ///     println!("Created task: {:?}", task_id);
    ///     
    ///     // Step 2: Register task
    ///     // 注册任务
    ///     let handle = timer.register(task);
    /// }
    /// ```
    #[inline]
    pub fn create_task(delay: Duration, callback: Option<CallbackWrapper>) -> crate::task::TimerTask {
        crate::task::TimerTask::new(delay, callback)
    }
    
    /// Create batch of timer tasks (static method, apply stage, no callbacks)
    /// 
    /// # Parameters
    /// - `delays`: List of delay times
    /// 
    /// # Returns
    /// Return TimerTask list, needs to be registered through `register_batch()`
    /// 
    /// 创建定时器任务 (静态方法，应用阶段，没有回调)
    /// 
    /// # 参数
    /// - `delays`: 延迟时间列表
    /// 
    /// # 返回值
    /// 返回 TimerTask 列表，需要通过 `register_batch()` 注册
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
    ///     // Step 1: Create batch of tasks
    ///     // 创建批量任务
    ///     let delays: Vec<Duration> = (0..3)
    ///         .map(|_| Duration::from_millis(100))
    ///         .collect();
    ///     
    ///     // Create batch of tasks
    ///     let tasks = TimerWheel::create_batch(delays);
    ///     println!("Created {} tasks", tasks.len());
    ///     
    ///     // Step 2: Register batch of tasks
    ///     // 注册批量任务
    ///     let batch = timer.register_batch(tasks);
    /// }
    /// ```
    #[inline]
    pub fn create_batch(delays: Vec<Duration>) -> Vec<crate::task::TimerTask>
    {
        delays
            .into_iter()
            .map(|delay| crate::task::TimerTask::new(delay, None))
            .collect()
    }

    /// Create batch of timer tasks (static method, apply stage, with callbacks)
    /// 
    /// # Parameters
    /// - `callbacks`: List of tuples of (delay time, callback)
    /// 
    /// # Returns
    /// Return TimerTask list, needs to be registered through `register_batch()`
    /// 
    /// 创建定时器任务 (静态方法，应用阶段，有回调)
    /// 
    /// # 参数
    /// - `callbacks`: (延迟时间, 回调) 元组列表
    /// 
    /// # 返回值
    /// 返回 TimerTask 列表，需要通过 `register_batch()` 注册
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
    ///     // Step 1: Create batch of tasks
    ///     // 创建批量任务
    ///     let delays: Vec<Duration> = (0..3)
    ///         .map(|_| Duration::from_millis(100))
    ///         .collect();
    ///     let callbacks: Vec<(Duration, Option<CallbackWrapper>)> = delays
    ///         .into_iter()
    ///         .map(|delay| {
    ///             let counter = Arc::clone(&counter);
    ///             let callback = Some(CallbackWrapper::new(move || {
    ///                 let counter = Arc::clone(&counter);
    ///                 async move {
    ///                     counter.fetch_add(1, Ordering::SeqCst);
    ///                 }
    ///             }));
    ///             (delay, callback)
    ///         })
    ///         .collect();
    ///     
    ///     // Create batch of tasks
    ///     let tasks = TimerWheel::create_batch_with_callbacks(callbacks);
    ///     println!("Created {} tasks", tasks.len());
    ///     
    ///     // Step 2: Register batch of tasks
    ///     // 注册批量任务
    ///     let batch = timer.register_batch(tasks);
    /// }
    /// ```
    #[inline]
    pub fn create_batch_with_callbacks(callbacks: Vec<(Duration, Option<CallbackWrapper>)>) -> Vec<crate::task::TimerTask>
    {
        callbacks
            .into_iter()
            .map(|(delay, callback)| crate::task::TimerTask::new(delay, callback))
            .collect()
    }
    
    /// Register timer task to timing wheel (registration phase)
    /// 
    /// # Parameters
    /// - `task`: Task created via `create_task()`
    /// 
    /// # Returns
    /// Return timer handle that can be used to cancel timer and receive completion notifications
    /// 
    /// 注册定时器任务到时间轮 (注册阶段)
    /// 
    /// # 参数
    /// - `task`: 通过 `create_task()` 创建的任务
    /// 
    /// # 返回值
    /// 返回定时器句柄，可用于取消定时器和接收完成通知
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
    ///     let task = TimerWheel::create_task(Duration::from_secs(1), Some(CallbackWrapper::new(|| async {
    ///         println!("Timer fired!");
    ///     })));
    ///     let task_id = task.get_id();
    ///     
    ///     // Register task
    ///     // 注册任务
    ///     let handle = timer.register(task);
    ///     
    ///     // Wait for timer completion
    ///     // 等待定时器完成
    ///     handle.into_completion_receiver().0.await.ok();
    /// }
    /// ```
    #[inline]
    pub fn register(&self, task: crate::task::TimerTask) -> TimerHandle {
        let (completion_tx, completion_rx) = oneshot::channel();
        let notifier = crate::task::CompletionNotifier(completion_tx);
        
        let task_id = task.id;
        
        // 单次加锁完成所有操作
        {
            let mut wheel_guard = self.wheel.lock();
            wheel_guard.insert(task, notifier);
        }
        
        TimerHandle::new(task_id, self.wheel.clone(), completion_rx)
    }
    
    /// Batch register timer tasks to timing wheel (registration phase)
    /// 
    /// # Parameters
    /// - `tasks`: List of tasks created via `create_batch()`
    /// 
    /// # Returns
    /// Return batch timer handle
    /// 
    /// 批量注册定时器任务到时间轮 (注册阶段)
    /// 
    /// # 参数
    /// - `tasks`: 通过 `create_batch()` 创建的任务列表
    /// 
    /// # 返回值
    /// 返回批量定时器句柄
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
    ///     let delays: Vec<Duration> = (0..3)
    ///         .map(|_| Duration::from_secs(1))
    ///         .collect();
    ///     let tasks = TimerWheel::create_batch(delays);
    ///     
    ///     let batch = timer.register_batch(tasks);
    ///     println!("Registered {} timers", batch.len());
    /// }
    /// ```
    #[inline]
    pub fn register_batch(&self, tasks: Vec<crate::task::TimerTask>) -> BatchHandle {
        let task_count = tasks.len();
        let mut completion_rxs = Vec::with_capacity(task_count);
        let mut task_ids = Vec::with_capacity(task_count);
        let mut prepared_tasks = Vec::with_capacity(task_count);
        
        // Step 1: Prepare all channels and notifiers
        for task in tasks {
            let (completion_tx, completion_rx) = oneshot::channel();
            let notifier = crate::task::CompletionNotifier(completion_tx);
            
            task_ids.push(task.id);
            completion_rxs.push(completion_rx);
            prepared_tasks.push((task, notifier));
        }
        
        // Step 2: Single lock, batch insert
        {
            let mut wheel_guard = self.wheel.lock();
            wheel_guard.insert_batch(prepared_tasks);
        }
        
        BatchHandle::new(task_ids, self.wheel.clone(), completion_rxs)
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
    ///     let task = TimerWheel::create_task(Duration::from_secs(10), Some(CallbackWrapper::new(|| async {
    ///         println!("Timer fired!");
    ///     })));
    ///     let task_id = task.get_id();
    ///     let _handle = timer.register(task);
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
    ///     let task1 = TimerWheel::create_task(Duration::from_secs(10), None);
    ///     let task2 = TimerWheel::create_task(Duration::from_secs(10), None);
    ///     let task3 = TimerWheel::create_task(Duration::from_secs(10), None);
    ///     
    ///     let task_ids = vec![task1.get_id(), task2.get_id(), task3.get_id()];
    ///     
    ///     let _h1 = timer.register(task1);
    ///     let _h2 = timer.register(task2);
    ///     let _h3 = timer.register(task3);
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
    ///     let task = TimerWheel::create_task(Duration::from_secs(5), Some(CallbackWrapper::new(|| async {
    ///         println!("Timer fired!");
    ///     })));
    ///     let task_id = task.get_id();
    ///     let _handle = timer.register(task);
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
    ///     let task = TimerWheel::create_task(Duration::from_secs(5), Some(CallbackWrapper::new(|| async {
    ///         println!("Original callback!");
    ///     })));
    ///     let task_id = task.get_id();
    ///     let _handle = timer.register(task);
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
    ///     let task1 = TimerWheel::create_task(Duration::from_secs(5), Some(CallbackWrapper::new(|| async {
    ///         println!("Task 1 fired!");
    ///     })));
    ///     let task2 = TimerWheel::create_task(Duration::from_secs(5), Some(CallbackWrapper::new(|| async {
    ///         println!("Task 2 fired!");
    ///     })));
    ///     let task3 = TimerWheel::create_task(Duration::from_secs(5), Some(CallbackWrapper::new(|| async {
    ///         println!("Task 3 fired!");
    ///     })));
    ///     
    ///     let task_ids = vec![
    ///         (task1.get_id(), Duration::from_secs(10)),
    ///         (task2.get_id(), Duration::from_secs(15)),
    ///         (task3.get_id(), Duration::from_secs(20)),
    ///     ];
    ///     
    ///     timer.register(task1);
    ///     timer.register(task2);
    ///     timer.register(task3);
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
    ///     let task1 = TimerWheel::create_task(Duration::from_secs(5), None);
    ///     let task2 = TimerWheel::create_task(Duration::from_secs(5), None);
    ///     
    ///     let id1 = task1.get_id();
    ///     let id2 = task2.get_id();
    ///     
    ///     timer.register(task1);
    ///     timer.register(task2);
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
    async fn tick_loop(wheel: Arc<Mutex<Wheel>>, tick_duration: Duration) {
        let mut interval = tokio::time::interval(tick_duration);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

        loop {
            interval.tick().await;

            // Advance timing wheel and get expired tasks
            let expired_tasks = {
                let mut wheel_guard = wheel.lock();
                wheel_guard.advance()
            };

            // Execute expired tasks
            for task in expired_tasks {
                let callback = task.get_callback();
                
                // Move task ownership to get completion_notifier
                let notifier = task.completion_notifier;
                
                // Only registered tasks have notifier
                if let Some(notifier) = notifier {
                    // Execute callback in a separate tokio task, and send notification after callback completion
                    if let Some(callback) = callback {
                        tokio::spawn(async move {
                            // Execute callback
                            let future = callback.call();
                            future.await;
                            
                            // Send notification after callback completion
                            let _ = notifier.0.send(TaskCompletionReason::Expired);
                        });
                    } else {
                        // Send notification immediately if no callback
                        let _ = notifier.0.send(TaskCompletionReason::Expired);
                    }
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

        let task = TimerWheel::create_task(
            Duration::from_millis(50),
            Some(CallbackWrapper::new(move || {
                let counter = Arc::clone(&counter_clone);
                async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                }
            })),
        );
        let _handle = timer.register(task);

        // Wait for timer to trigger
        // 等待定时器触发
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_cancel_timer() {
        use std::sync::Arc;
        let timer = TimerWheel::with_defaults();
        let counter = Arc::new(AtomicU32::new(0));
        let counter_clone = Arc::clone(&counter);

        let task = TimerWheel::create_task(
            Duration::from_millis(100),
            Some(CallbackWrapper::new(move || {
                let counter = Arc::clone(&counter_clone);
                async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                }
            })),
        );
        let handle = timer.register(task);

        // Immediately cancel
        // 立即取消
        let cancel_result = handle.cancel();
        assert!(cancel_result);

        // Wait for enough time to ensure timer does not trigger
        // 等待足够时间确保定时器不触发
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(counter.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn test_cancel_immediate() {
        use std::sync::Arc;
        let timer = TimerWheel::with_defaults();
        let counter = Arc::new(AtomicU32::new(0));
        let counter_clone = Arc::clone(&counter);

        let task = TimerWheel::create_task(
            Duration::from_millis(100),
            Some(CallbackWrapper::new(move || {
                let counter = Arc::clone(&counter_clone);
                async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                }
            })),
        );
        let handle = timer.register(task);

        // Immediately cancel
        // 立即取消
        let cancel_result = handle.cancel();
        assert!(cancel_result);

        // Wait for enough time to ensure timer does not trigger
        // 等待足够时间确保定时器不触发
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(counter.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn test_postpone_timer() {
        use std::sync::Arc;
        let timer = TimerWheel::with_defaults();
        let counter = Arc::new(AtomicU32::new(0));
        let counter_clone = Arc::clone(&counter);

        let task = TimerWheel::create_task(
            Duration::from_millis(50),
            Some(CallbackWrapper::new(move || {
                let counter = Arc::clone(&counter_clone);
                async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                }
            })),
        );
        let task_id = task.get_id();
        let handle = timer.register(task);

        // Postpone task to 150ms
        // 推迟任务到 150ms
        let postponed = timer.postpone(task_id, Duration::from_millis(150), None);
        assert!(postponed);

        // Wait for original time 50ms, task should not trigger
        // 等待原始时间 50ms，任务不应触发
        tokio::time::sleep(Duration::from_millis(70)).await;
        assert_eq!(counter.load(Ordering::SeqCst), 0);

        // Wait for new trigger time (from postponed start, need to wait about 150ms)
        // 等待新的触发时间（从推迟开始算起，大约需要等待 150ms）
        let result = tokio::time::timeout(
            Duration::from_millis(200),
            handle.into_completion_receiver().0
        ).await;
        assert!(result.is_ok());
        
        // Wait for callback to execute
        // 等待回调执行
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_postpone_with_callback() {
        use std::sync::Arc;
        let timer = TimerWheel::with_defaults();
        let counter = Arc::new(AtomicU32::new(0));
        let counter_clone1 = Arc::clone(&counter);
        let counter_clone2 = Arc::clone(&counter);

        // Create task, original callback adds 1
        let task = TimerWheel::create_task(
            Duration::from_millis(50),
            Some(CallbackWrapper::new(move || {
                let counter = Arc::clone(&counter_clone1);
                async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                }
            })),
        );
        let task_id = task.get_id();
        let handle = timer.register(task);

        // Postpone task and replace callback, new callback adds 10
        // 推迟任务并替换回调，新回调增加 10
        let postponed = timer.postpone(
            task_id,
            Duration::from_millis(100),
            Some(CallbackWrapper::new(move || {
                let counter = Arc::clone(&counter_clone2);
                async move {
                    counter.fetch_add(10, Ordering::SeqCst);
                }
            })),
        );
        assert!(postponed);

        // Wait for task to trigger (after postponed, need to wait 100ms, plus margin)
        // 等待任务触发（推迟后，需要等待 100ms，加上余量）
        let result = tokio::time::timeout(
            Duration::from_millis(200),
            handle.into_completion_receiver().0
        ).await;
        assert!(result.is_ok());
        
        // Wait for callback to execute
        // 等待回调执行
        tokio::time::sleep(Duration::from_millis(20)).await;
        
        // Verify new callback is executed (increased 10 instead of 1)
        // 验证新回调已执行（增加 10 而不是 1）
        assert_eq!(counter.load(Ordering::SeqCst), 10);
    }

    #[tokio::test]
    async fn test_postpone_nonexistent_timer() {
        let timer = TimerWheel::with_defaults();
        
        // Try to postpone nonexistent task
        // 尝试推迟一个不存在的任务
        let fake_task = TimerWheel::create_task(Duration::from_millis(50), None);
        let fake_task_id = fake_task.get_id();
        // Do not register this task
        // 不注册这个任务
        let postponed = timer.postpone(fake_task_id, Duration::from_millis(100), None);
        assert!(!postponed);
    }

    #[tokio::test]
    async fn test_postpone_batch() {
        use std::sync::Arc;
        let timer = TimerWheel::with_defaults();
        let counter = Arc::new(AtomicU32::new(0));

        // Create 3 tasks
        // 创建 3 个任务
        let mut task_ids = Vec::new();
        for _ in 0..3 {
            let counter_clone = Arc::clone(&counter);
            let task = TimerWheel::create_task(
                Duration::from_millis(50),
                Some(CallbackWrapper::new(move || {
                    let counter = Arc::clone(&counter_clone);
                    async move {
                        counter.fetch_add(1, Ordering::SeqCst);
                    }
                })),
            );
            task_ids.push((task.get_id(), Duration::from_millis(150)));
            timer.register(task);
        }

        // Batch postpone
        // 批量推迟
        let postponed = timer.postpone_batch(task_ids);
        assert_eq!(postponed, 3);

        // Wait for original time 50ms, task should not trigger
        // 等待原始时间 50ms，任务不应触发
        tokio::time::sleep(Duration::from_millis(70)).await;
        assert_eq!(counter.load(Ordering::SeqCst), 0);

        // Wait for new trigger time (from postponed start, need to wait about 150ms)
        // 等待新的触发时间（从推迟开始算起，大约需要等待 150ms）
        tokio::time::sleep(Duration::from_millis(200)).await;
        
        // Wait for callback to execute
        // 等待回调执行
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert_eq!(counter.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn test_postpone_batch_with_callbacks() {
        use std::sync::Arc;
        let timer = TimerWheel::with_defaults();
        let counter = Arc::new(AtomicU32::new(0));

        // Create 3 tasks
        // 创建 3 个任务
        let mut task_ids = Vec::new();
        for _ in 0..3 {
            let task = TimerWheel::create_task(
                Duration::from_millis(50),
                None
            );
            task_ids.push(task.get_id());
            timer.register(task);
        }

        // Batch postpone and replace callbacks
        // 批量推迟并替换回调
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

        // Batch postpone and replace callbacks
        // 批量推迟并替换回调
        let postponed = timer.postpone_batch_with_callbacks(updates);
        assert_eq!(postponed, 3);

        // Wait for original time 50ms, task should not trigger
        // 等待原始时间 50ms，任务不应触发
        tokio::time::sleep(Duration::from_millis(70)).await;
        assert_eq!(counter.load(Ordering::SeqCst), 0);

        // Wait for new trigger time (from postponed start, need to wait about 150ms)
        // 等待新的触发时间（从推迟开始算起，大约需要等待 150ms）
        tokio::time::sleep(Duration::from_millis(200)).await;
        
        // Wait for callback to execute
        // 等待回调执行
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert_eq!(counter.load(Ordering::SeqCst), 3);
    }

    #[tokio::test]
    async fn test_postpone_keeps_completion_receiver_valid() {
        use std::sync::Arc;
        let timer = TimerWheel::with_defaults();
        let counter = Arc::new(AtomicU32::new(0));
        let counter_clone = Arc::clone(&counter);

        let task = TimerWheel::create_task(
            Duration::from_millis(50),
            Some(CallbackWrapper::new(move || {
                let counter = Arc::clone(&counter_clone);
                async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                }
            })),
        );
        let task_id = task.get_id();
        let handle = timer.register(task);

        // Postpone task
        // 推迟任务
        timer.postpone(task_id, Duration::from_millis(100), None);

        // Verify original completion_receiver is still valid (after postponed, need to wait 100ms, plus margin)
        // 验证原始完成接收器是否仍然有效（推迟后，需要等待 100ms，加上余量）
        let result = tokio::time::timeout(
            Duration::from_millis(200),
            handle.into_completion_receiver().0
        ).await;
        assert!(result.is_ok(), "Completion receiver should still work after postpone");
        
        // Wait for callback to execute
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }
}

