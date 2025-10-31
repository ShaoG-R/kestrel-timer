use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::oneshot;

/// 全局唯一的任务 ID 生成器 (Global unique task ID generator)
static NEXT_TASK_ID: AtomicU64 = AtomicU64::new(1);

/// 任务完成原因 (Task Completion Reason)
///
/// 表示定时器任务完成的原因，可以是正常到期或被取消。
/// (Indicates the reason for task completion, either expired or cancelled)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskCompletionReason {
    /// 任务正常到期 (Task expired normally)
    Expired,
    /// 任务被取消 (Task was cancelled)
    Cancelled,
}

/// 定时器任务的唯一标识符 (Unique identifier for timer tasks)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct TaskId(u64);

impl TaskId {
    /// 生成一个新的唯一任务 ID（内部使用）
    /// Generate a new unique task ID (internal use)
    #[inline]
    pub(crate) fn new() -> Self {
        TaskId(NEXT_TASK_ID.fetch_add(1, Ordering::Relaxed))
    }

    /// 获取任务 ID 的数值 (Get the numeric value of the task ID)
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

/// 定时器回调 trait (Timer Callback Trait)
/// 
/// 实现此 trait 的类型可以作为定时器的回调函数使用。
/// (Types implementing this trait can be used as timer callbacks)
/// 
/// # 示例
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
    /// 执行回调，返回一个 Future (Execute callback, returns a Future)
    fn call(&self) -> Pin<Box<dyn Future<Output = ()> + Send>>;
}

/// 为闭包实现 TimerCallback trait (Implement TimerCallback trait for closures)
/// 支持 Fn() -> Future 类型的闭包（可以多次调用，适合周期性任务）
/// (Supports Fn() -> Future closures, can be called multiple times, suitable for periodic tasks)
impl<F, Fut> TimerCallback for F
where
    F: Fn() -> Fut + Send + Sync + 'static,
    Fut: Future<Output = ()> + Send + 'static,
{
    fn call(&self) -> Pin<Box<dyn Future<Output = ()> + Send>> {
        Box::pin(self())
    }
}

/// 回调包装器，用于规范化创建和管理回调 (Callback wrapper for standardized callback creation and management)
/// 
/// # 示例 (Examples)
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
    /// 创建新的回调包装器 (Create a new callback wrapper)
    /// 
    /// # 参数 (Parameters)
    /// - `callback`: 实现了 TimerCallback trait 的回调对象 
    /// (Callback object implementing TimerCallback trait)
    /// 
    /// # 示例 (Examples)
    /// 
    /// ```
    /// use kestrel_timer::CallbackWrapper;
    /// 
    /// let callback = CallbackWrapper::new(|| async {
    ///     println!("Timer fired!");
    /// });
    /// ```
    #[inline]
    pub fn new(callback: impl TimerCallback) -> Self {
        Self {
            callback: Arc::new(callback),
        }
    }

    /// 调用回调函数 (Call the callback function)
    #[inline]
    pub(crate) fn call(&self) -> Pin<Box<dyn Future<Output = ()> + Send>> {
        self.callback.call()
    }
}

/// 完成通知器，用于在任务完成时发送通知 (Completion notifier for sending notifications when tasks complete)
pub struct CompletionNotifier(pub oneshot::Sender<TaskCompletionReason>);

/// 定时器任务 (Timer Task)
/// 
/// 用户通过两步式 API 使用 :
/// 1. 使用 `TimerTask::new()` 创建任务
/// 2. 使用 `TimerWheel::register()` 或 `TimerService::register()` 注册任务
/// 
/// Users interact via a two-step API
/// 1. Create task using `TimerTask::new()`
/// 2. Register task using `TimerWheel::register()` or `TimerService::register()`
pub struct TimerTask {
    /// 任务唯一标识符 (Unique task identifier)
    pub(crate) id: TaskId,
    
    /// 用户指定的延迟时间 (User-specified delay duration)
    pub(crate) delay: std::time::Duration,
    
    /// 到期时间（相对于时间轮的 tick 数）(Expiration time in ticks relative to the timing wheel)
    pub(crate) deadline_tick: u64,
    
    /// 轮次计数（用于超出时间轮范围的任务）(Round counter for tasks beyond the wheel's range)
    pub(crate) rounds: u32,
    
    /// 异步回调函数（可选）(Async callback function, optional)
    pub(crate) callback: Option<CallbackWrapper>,
    
    /// 完成通知器（用于在任务完成时发送通知，注册时创建）
    /// (Completion notifier for sending notifications when task completes, created during registration)
    pub(crate) completion_notifier: Option<CompletionNotifier>,
}

impl TimerTask {
    /// 创建新的定时器任务（内部使用）(Create a new timer task, internal use)
    /// 
    /// # 参数 (Parameters)
    /// - `delay`: 延迟时间 (Delay duration)
    /// - `callback`: 回调函数（可选）(Callback function, optional)
    /// 
    /// # 注意 (Note)
    /// 这是内部方法，用户应该使用 `TimerWheel::create_task()` 或 `TimerService::create_task()` 创建任务。
    /// (This is an internal method, users should use `TimerWheel::create_task()` or `TimerService::create_task()` to create tasks)
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

    /// 获取任务 ID (Get task ID)
    /// 
    /// # 示例 (Examples)
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

    /// 内部方法：准备注册（在注册时由时间轮调用）
    /// Internal method: Prepare for registration (called by timing wheel during registration)
    /// 
    /// 注意：此方法已内联到 insert/insert_batch 中以提升性能，但保留此方法以供未来可能的其他用途
    /// Note: This method has been inlined into insert/insert_batch for performance, but kept for potential future use
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

    /// 获取回调函数的克隆（如果存在）
    /// (Get a clone of the callback function if present)
    #[inline]
    pub(crate) fn get_callback(&self) -> Option<CallbackWrapper> {
        self.callback.clone()
    }
}

/// 任务位置信息（包含层级），用于分层时间轮
/// Task location information (including level) for hierarchical timing wheel
/// 
/// 优化内存布局：将 level 字段放在最前，利用结构体对齐减少填充
/// Memory layout optimization: level field placed first to reduce padding via struct alignment
#[derive(Debug, Clone, Copy)]
pub(crate) struct TaskLocation {
    /// 槽位索引 (Slot index)
    pub slot_index: usize,
    /// 任务在槽位 Vec 中的索引位置（用于 O(1) 取消）
    /// (Index position of task in slot Vec for O(1) cancellation)
    pub vec_index: usize,
    /// 层级：0 = L0（底层），1 = L1（高层）
    /// Level: 0 = L0 (bottom layer), 1 = L1 (upper layer)
    /// 使用 u8 而非 bool，为未来可能的多层扩展预留空间
    /// (Using u8 instead of bool to reserve space for potential multi-layer expansion)
    pub level: u8,
}

impl TaskLocation {
    #[inline(always)]
    pub fn new(level: u8, slot_index: usize, vec_index: usize) -> Self {
        Self {
            slot_index,
            vec_index,
            level,
        }
    }
}

