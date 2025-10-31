use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::oneshot;

/// Global unique task ID generator
/// 
/// 全局唯一任务 ID 生成器
static NEXT_TASK_ID: AtomicU64 = AtomicU64::new(1);

/// Task Completion Reason
///
/// Indicates the reason for task completion, either expired or cancelled.
/// 
/// 任务完成原因，要么是过期，要么是取消。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskCompletionReason {
    /// Task expired normally
    /// 
    /// 任务正常过期
    Expired,
    /// Task was cancelled
    /// 
    /// 任务被取消
    Cancelled,
}

/// Unique identifier for timer tasks
/// 
/// 定时器任务唯一标识符
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TaskId(u64);

impl TaskId {
    /// Generate a new unique task ID (internal use)
    /// 
    /// 生成一个新的唯一任务 ID (内部使用)
    #[inline]
    pub(crate) fn new() -> Self {
        TaskId(NEXT_TASK_ID.fetch_add(1, Ordering::Relaxed))
    }

    /// Get the numeric value of the task ID
    /// 
    /// 获取任务 ID 的数值
    #[inline]
    pub fn as_u64(&self) -> u64 {
        self.0
    }
}

impl Default for TaskId {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

/// Timer Callback Trait
/// 
/// Types implementing this trait can be used as timer callbacks.
/// 
/// 可实现此特性的类型可以作为定时器回调函数。
/// 
/// # Examples (示例)
/// 
/// ```
/// use kestrel_timer::TimerCallback;
/// use std::future::Future;
/// use std::pin::Pin;
/// 
/// struct MyCallback;
/// 
/// impl TimerCallback for MyCallback {
///     fn call(&self) -> Pin<Box<dyn Future<Output = ()> + Send>> {
///         Box::pin(async {
///             println!("Timer callback executed!");
///         })
///     }
/// }
/// ```
pub trait TimerCallback: Send + Sync + 'static {
    /// Execute callback, returns a Future
    /// 
    /// 执行回调函数，返回一个 Future
    fn call(&self) -> Pin<Box<dyn Future<Output = ()> + Send>>;
}

/// Implement TimerCallback trait for closures
/// 
/// Supports Fn() -> Future closures, can be called multiple times, suitable for periodic tasks
/// 
/// 实现 TimerCallback 特性的类型，支持 Fn() -> Future 闭包，可以多次调用，适合周期性任务
impl<F, Fut> TimerCallback for F
where
    F: Fn() -> Fut + Send + Sync + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    fn call(&self) -> Pin<Box<dyn Future<Output = ()> + Send>> {
        Box::pin(self())
    }
}

/// Callback wrapper for standardized callback creation and management
/// 
/// Callback 包装器，用于标准化回调创建和管理
/// 
/// # Examples (示例)
/// 
/// ```
/// use kestrel_timer::CallbackWrapper;
/// 
/// let callback = CallbackWrapper::new(|| async {
///     println!("Timer callback executed!");
/// });
/// ```
#[derive(Clone)]
pub struct CallbackWrapper {
    callback: Arc<dyn TimerCallback>,
}

impl CallbackWrapper {
    /// Create a new callback wrapper
    /// 
    /// # Parameters
    /// - `callback`: Callback object implementing TimerCallback trait
    /// 
    /// # 创建一个新的回调包装器
    /// 
    /// # 参数
    /// - `callback`: 实现 TimerCallback 特性的回调对象
    /// 
    /// # Examples (示例)
    /// 
    /// ```
    /// use kestrel_timer::CallbackWrapper;
    /// 
    /// let callback = CallbackWrapper::new(|| async {
    ///     println!("Timer fired!"); // 定时器触发
    /// });
    /// ```
    #[inline]
    pub fn new(callback: impl TimerCallback) -> Self {
        Self {
            callback: Arc::new(callback),
        }
    }

    /// Call the callback function
    /// 
    /// 调用回调函数
    #[inline]
    pub(crate) fn call(&self) -> Pin<Box<dyn Future<Output = ()> + Send>> {
        self.callback.call()
    }
}

/// Completion notifier for sending notifications when tasks complete
/// 
/// 完成通知器，用于发送任务完成通知
pub struct CompletionNotifier(pub oneshot::Sender<TaskCompletionReason>);

/// Timer Task
/// 
/// Users interact via a two-step API
/// 1. Create task using `TimerTask::new()`
/// 2. Register task using `TimerWheel::register()` or `TimerService::register()`
/// 
/// 定时器任务
/// 
/// 用户通过两步 API 与定时器交互
/// 1. 使用 `TimerTask::new()` 创建任务
/// 2. 使用 `TimerWheel::register()` 或 `TimerService::register()` 注册任务
pub struct TimerTask {
    /// Unique task identifier
    /// 
    /// 唯一任务标识符
    pub(crate) id: TaskId,
    
    /// User-specified delay duration
    /// 
    /// 用户指定的延迟时间
    pub(crate) delay: std::time::Duration,
    
    /// Expiration time in ticks relative to the timing wheel
    /// 
    /// 相对于时间轮的过期时间（以 tick 为单位）
    pub(crate) deadline_tick: u64,
    
    /// Round counter for tasks beyond the wheel's range
    /// 
    /// 超出时间轮范围的任务轮数计数器
    pub(crate) rounds: u32,
    
    /// Async callback function, optional
    /// 
    /// 异步回调函数，可选
    pub(crate) callback: Option<CallbackWrapper>,
    
    /// Completion notifier for sending notifications when task completes, created during registration
    /// 
    /// 完成通知器，用于发送任务完成通知，在注册期间创建
    pub(crate) completion_notifier: Option<CompletionNotifier>,
}

impl TimerTask {
    /// Create a new timer task, internal use
    /// 
    /// # Parameters
    /// - `delay`: Delay duration
    /// - `callback`: Callback function, optional
    /// 
    /// # Note
    /// This is an internal method, users should use `TimerWheel::create_task()` or `TimerService::create_task()` to create tasks.
    /// 
    /// 
    /// 创建一个新的定时器任务，内部使用
    /// 
    /// # 参数
    /// - `delay`: 延迟时间
    /// - `callback`: 回调函数，可选
    /// 
    /// # 注意
    /// 这是一个内部方法，用户应该使用 `TimerWheel::create_task()` 或 `TimerService::create_task()` 来创建任务。
    #[inline]
    pub(crate) fn new(delay: std::time::Duration, callback: Option<CallbackWrapper>) -> Self {
        Self {
            id: TaskId::new(),
            delay,
            deadline_tick: 0,
            rounds: 0,
            callback,
            completion_notifier: None,
        }
    }

    /// Get task ID
    /// 
    /// 获取任务 ID
    /// 
    /// # Examples (示例)
    /// 
    /// ```no_run
    /// use kestrel_timer::TimerWheel;
    /// use std::time::Duration;
    /// 
    /// let task = TimerWheel::create_task(Duration::from_secs(1), None);
    /// let task_id = task.get_id();
    /// println!("Task ID: {:?}", task_id);
    /// ```
    pub fn get_id(&self) -> TaskId {
        self.id
    }

    /// Prepare for registration (called by timing wheel during registration)
    /// 
    /// 准备注册（在注册期间由时间轮调用）
    #[allow(dead_code)]
    pub(crate) fn prepare_for_registration(
        &mut self,
        completion_notifier: CompletionNotifier,
        deadline_tick: u64,
        rounds: u32,
    ) {
        self.completion_notifier = Some(completion_notifier);
        self.deadline_tick = deadline_tick;
        self.rounds = rounds;
    }

    /// Get a clone of the callback function if present
    /// 
    /// 获取回调函数（如果存在）
    #[inline]
    pub(crate) fn get_callback(&self) -> Option<CallbackWrapper> {
        self.callback.clone()
    }
}

/// Task location information (including level) for hierarchical timing wheel
/// 
/// Memory layout optimization: level field placed first to reduce padding via struct alignment
/// 
/// 任务位置信息（包括层级）用于分层时间轮
/// 
/// 内存布局优化：将 level 字段放在第一位，通过结构体对齐来减少填充
#[derive(Debug, Clone, Copy)]
pub(crate) struct TaskLocation {
    /// Slot index
    /// 
    /// 槽索引
    pub slot_index: usize,
    /// Index position of task in slot Vec for O(1) cancellation
    /// 
    /// 槽向量中任务的索引位置，用于 O(1) 取消
    pub vec_index: usize,
    /// Level: 0 = L0 (bottom layer), 1 = L1 (upper layer)
    /// Using u8 instead of bool to reserve space for potential multi-layer expansion
    /// 
    /// 层级：0 = L0（底层），1 = L1（上层）
    /// 使用 u8 而不是 bool 来保留空间，用于潜在的多层扩展
    pub level: u8,
}

impl TaskLocation {
    /// Create a new task location information
    /// 
    /// 创建一个新的任务位置信息
    #[inline(always)]
    pub fn new(level: u8, slot_index: usize, vec_index: usize) -> Self {
        Self {
            slot_index,
            vec_index,
            level,
        }
    }
}

