use crate::task::{TaskId, TaskCompletionReason};
use crate::wheel::Wheel;
use parking_lot::Mutex;
use std::sync::Arc;
use tokio::sync::oneshot;

/// Completion receiver for receiving timer completion notifications
/// 
/// 完成通知接收器，用于接收定时器完成通知
pub struct CompletionReceiver(pub oneshot::Receiver<TaskCompletionReason>);

/// Timer handle for managing timer lifecycle (without completion receiver)
/// 
/// Note: This type does not implement Clone to prevent duplicate cancellation of the same timer. Each timer should have only one owner.
/// 
/// 定时器句柄，用于管理定时器生命周期（不含完成通知接收器）
/// 
/// 注意：此类型未实现 Clone 以防止重复取消同一定时器。每个定时器应该只有一个所有者。
pub struct TimerHandle {
    pub(crate) task_id: TaskId,
    pub(crate) wheel: Arc<Mutex<Wheel>>,
}

impl TimerHandle {
    #[inline]
    pub(crate) fn new(task_id: TaskId, wheel: Arc<Mutex<Wheel>>) -> Self {
        Self { task_id, wheel }
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
    /// let handle_with_completion = timer.register(task);
    /// let (rx, handle) = handle_with_completion.into_parts();
    /// 
    /// // Cancel the timer
    /// let success = handle.cancel();
    /// println!("Canceled successfully: {}", success);
    /// # }
    /// ```
    #[inline]
    pub fn cancel(&self) -> bool {
        let mut wheel = self.wheel.lock();
        wheel.cancel(self.task_id)
    }
}

/// Timer handle with completion receiver for managing timer lifecycle
/// 
/// Note: This type does not implement Clone to prevent duplicate cancellation of the same timer. Each timer should have only one owner.
/// 
/// 包含完成通知接收器的定时器句柄，用于管理定时器生命周期
/// 
/// 注意：此类型未实现 Clone 以防止重复取消同一定时器。每个定时器应该只有一个所有者。
pub struct TimerHandleWithCompletion {
    handle: TimerHandle,
    pub(crate) completion_rx: CompletionReceiver,
}

impl TimerHandleWithCompletion {
    pub(crate) fn new(handle: TimerHandle, completion_rx: oneshot::Receiver<TaskCompletionReason>) -> Self {
        Self { handle, completion_rx: CompletionReceiver(completion_rx) }
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
        self.handle.cancel()
    }

    /// Split handle into completion receiver and timer handle
    /// 
    /// 将句柄拆分为完成通知接收器和定时器句柄
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
    /// // Split into receiver and handle
    /// // 拆分为接收器和句柄
    /// let (rx, handle) = handle.into_parts();
    /// 
    /// // Wait for timer completion
    /// // 等待定时器完成
    /// rx.0.await.ok();
    /// println!("Timer completed!");
    /// # }
    /// ```
    pub fn into_parts(self) -> (CompletionReceiver, TimerHandle) {
        (self.completion_rx, self.handle)
    }
}

/// Batch timer handle for managing batch-scheduled timers (without completion receivers)
/// 
/// Note: This type does not implement Clone to prevent duplicate cancellation of the same batch of timers. Use `into_iter()` or `into_handles()` to access individual timer handles.
/// 
/// 批量定时器句柄，用于管理批量调度的定时器（不含完成通知接收器）
/// 
/// 注意：此类型未实现 Clone 以防止重复取消同一批定时器。使用 `into_iter()` 或 `into_handles()` 访问单个定时器句柄。
pub struct BatchHandle {
    pub(crate) task_ids: Vec<TaskId>,
    pub(crate) wheel: Arc<Mutex<Wheel>>,
}

impl BatchHandle {
    #[inline]
    pub(crate) fn new(task_ids: Vec<TaskId>, wheel: Arc<Mutex<Wheel>>) -> Self {
        Self { task_ids, wheel }
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
    /// let batch_with_completion = timer.register_batch(tasks);
    /// let (rxs, batch) = batch_with_completion.into_parts();
    /// 
    /// let cancelled = batch.cancel_all();
    /// println!("Canceled {} timers", cancelled);
    /// # }
    /// ```
    #[inline]
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
    /// let batch_with_completion = timer.register_batch(tasks);
    /// let (rxs, batch) = batch_with_completion.into_parts();
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
    #[inline]
    pub fn into_handles(self) -> Vec<TimerHandle> {
        self.task_ids
            .into_iter()
            .map(|task_id| {
                TimerHandle::new(task_id, self.wheel.clone())
            })
            .collect()
    }

    /// Get the number of batch tasks
    /// 
    /// 获取批量任务数量
    #[inline]
    pub fn len(&self) -> usize {
        self.task_ids.len()
    }

    /// Check if batch tasks are empty
    /// 
    /// 检查批量任务是否为空
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.task_ids.is_empty()
    }

    /// Get reference to all task IDs
    /// 
    /// 获取所有任务 ID 的引用
    #[inline]
    pub fn task_ids(&self) -> &[TaskId] {
        &self.task_ids
    }
}

/// Batch timer handle with completion receivers for managing batch-scheduled timers
/// 
/// Note: This type does not implement Clone to prevent duplicate cancellation of the same batch of timers. Use `into_iter()` or `into_handles()` to access individual timer handles.
/// 
/// 包含完成通知接收器的批量定时器句柄，用于管理批量调度的定时器
/// 
/// 注意：此类型未实现 Clone 以防止重复取消同一批定时器。使用 `into_iter()` 或 `into_handles()` 访问单个定时器句柄。
pub struct BatchHandleWithCompletion {
    handles: BatchHandle,
    completion_rxs: Vec<oneshot::Receiver<TaskCompletionReason>>,
}

impl BatchHandleWithCompletion {
    #[inline]
    pub(crate) fn new(handles: BatchHandle, completion_rxs: Vec<oneshot::Receiver<TaskCompletionReason>>) -> Self {
        Self { handles, completion_rxs }
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
    #[inline]
    pub fn cancel_all(self) -> usize {
        self.handles.cancel_all()
    }

    /// Convert batch handle to Vec of individual timer handles with completion receivers
    ///
    /// Consumes BatchHandleWithCompletion and creates independent TimerHandleWithCompletion for each task
    /// 
    /// 将批量句柄转换为单个定时器句柄的 Vec
    ///
    /// 消费 BatchHandleWithCompletion 并为每个任务创建独立的 TimerHandleWithCompletion
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
    #[inline]
    pub fn into_handles(self) -> Vec<TimerHandleWithCompletion> {
        self.handles.into_handles()
            .into_iter()
            .zip(self.completion_rxs.into_iter())
            .map(|(handle, rx)| {
                TimerHandleWithCompletion::new(handle, rx)
            })
            .collect()
    }

    /// Get the number of batch tasks
    /// 
    /// 获取批量任务数量
    #[inline]
    pub fn len(&self) -> usize {
        self.handles.len()
    }

    /// Check if batch tasks are empty
    /// 
    /// 检查批量任务是否为空
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.handles.is_empty()
    }

    /// Get reference to all task IDs
    /// 
    /// 获取所有任务 ID 的引用
    #[inline]
    pub fn task_ids(&self) -> &[TaskId] {
        self.handles.task_ids()
    }

    /// Split batch handle into completion receivers and batch handle
    /// 
    /// 将批量句柄拆分为完成通知接收器列表和批量句柄
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
    /// // Split into receivers and handle
    /// // 拆分为接收器和句柄
    /// let (receivers, batch_handle) = batch.into_parts();
    /// for rx in receivers {
    ///     tokio::spawn(async move {
    ///         if rx.await.is_ok() {
    ///             println!("A timer completed!");
    ///         }
    ///     });
    /// }
    /// # }
    /// ```
    #[inline]
    pub fn into_parts(self) -> (Vec<oneshot::Receiver<TaskCompletionReason>>, BatchHandle) {
        let handle = BatchHandle::new(self.handles.task_ids.clone(), self.handles.wheel);
        (self.completion_rxs, handle)
    }
}

/// Implement IntoIterator to allow direct iteration over BatchHandleWithCompletion
/// 
/// 实现 IntoIterator 以允许直接迭代 BatchHandleWithCompletion
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
/// // Iterate directly, each element is an independent TimerHandleWithCompletion
/// // 直接迭代，每个元素是一个独立的 TimerHandleWithCompletion
/// for handle in batch {
///     // Can operate each handle individually
///     // 可以单独操作每个句柄
/// }
/// # }
/// ```
impl IntoIterator for BatchHandleWithCompletion {
    type Item = TimerHandleWithCompletion;
    type IntoIter = BatchHandleWithCompletionIter;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        BatchHandleWithCompletionIter {
            task_ids: self.handles.task_ids.into_iter(),
            completion_rxs: self.completion_rxs.into_iter(),
            wheel: self.handles.wheel,
        }
    }
}

/// Iterator for BatchHandleWithCompletion
/// 
/// BatchHandleWithCompletion 的迭代器
pub struct BatchHandleWithCompletionIter {
    task_ids: std::vec::IntoIter<TaskId>,
    completion_rxs: std::vec::IntoIter<oneshot::Receiver<TaskCompletionReason>>,
    wheel: Arc<Mutex<Wheel>>,
}

impl Iterator for BatchHandleWithCompletionIter {
    type Item = TimerHandleWithCompletion;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        match (self.task_ids.next(), self.completion_rxs.next()) {
            (Some(task_id), Some(rx)) => {
                Some(TimerHandleWithCompletion::new(TimerHandle::new(task_id, self.wheel.clone()), rx))
            }
            _ => None,
        }
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.task_ids.size_hint()
    }
}

impl ExactSizeIterator for BatchHandleWithCompletionIter {
    #[inline]
    fn len(&self) -> usize {
        self.task_ids.len()
    }
}

