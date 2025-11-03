use std::sync::Arc;
use std::sync::atomic::{AtomicU8, AtomicPtr, Ordering};
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll, Waker};
use std::ptr;

use crate::task::{ONESHOT_PENDING, ONESHOT_CALLED, ONESHOT_CANCELLED, TaskCompletion};

/// Trait for types that can be used as oneshot state
/// 
/// Types implementing this trait can be converted to/from u8 for atomic storage.
/// This allows for zero-allocation, lock-free state transitions.
/// 
/// 可用作 oneshot 状态的类型的 trait
/// 
/// 实现此 trait 的类型可以与 u8 互相转换以进行原子存储。
/// 这允许零分配、无锁的状态转换。
/// 
/// # Built-in Implementations
/// 
/// - `TaskCompletion`: Called or Cancelled states
/// - `()`: Simple completion notification without state
/// 
/// # Example: Custom State
/// 
/// ```rust,ignore
/// #[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// enum CustomState {
///     Success,
///     Failure,
///     Timeout,
/// }
/// 
/// impl State for CustomState {
///     fn to_u8(&self) -> u8 {
///         match self {
///             CustomState::Success => 1,
///             CustomState::Failure => 2,
///             CustomState::Timeout => 3,
///         }
///     }
///     
///     fn from_u8(value: u8) -> Option<Self> {
///         match value {
///             1 => Some(CustomState::Success),
///             2 => Some(CustomState::Failure),
///             3 => Some(CustomState::Timeout),
///             _ => None,
///         }
///     }
///     
///     fn pending_value() -> u8 {
///         0
///     }
/// }
/// 
/// // Usage:
/// let (notifier, receiver) = Sender::<CustomState>::new();
/// tokio::spawn(async move {
///     notifier.notify(CustomState::Success);
/// });
/// let result = receiver.await; // Direct await
/// ```
pub trait State: Sized + Send + Sync + 'static {
    /// Convert the state to u8 for atomic storage
    /// 
    /// 将状态转换为 u8 以进行原子存储
    fn to_u8(&self) -> u8;
    
    /// Convert u8 back to the state type
    /// 
    /// Returns None if the value doesn't represent a valid state
    /// 
    /// 将 u8 转换回状态类型
    /// 
    /// 如果值不代表有效状态则返回 None
    fn from_u8(value: u8) -> Option<Self>;
    
    /// The pending state value (before completion)
    /// 
    /// 待处理状态值（完成前）
    fn pending_value() -> u8;
}

impl State for TaskCompletion {
    #[inline]
    fn to_u8(&self) -> u8 {
        match self {
            TaskCompletion::Called => ONESHOT_CALLED,
            TaskCompletion::Cancelled => ONESHOT_CANCELLED,
        }
    }
    
    #[inline]
    fn from_u8(value: u8) -> Option<Self> {
        match value {
            ONESHOT_CALLED => Some(TaskCompletion::Called),
            ONESHOT_CANCELLED => Some(TaskCompletion::Cancelled),
            _ => None,
        }
    }
    
    #[inline]
    fn pending_value() -> u8 {
        ONESHOT_PENDING
    }
}

/// Implementation for unit type () - simple completion notification without state
/// 
/// 为单元类型 () 实现 - 简单的完成通知，无需状态信息
impl State for () {
    #[inline]
    fn to_u8(&self) -> u8 {
        1 // Completed
    }
    
    #[inline]
    fn from_u8(value: u8) -> Option<Self> {
        match value {
            1 => Some(()),
            _ => None,
        }
    }
    
    #[inline]
    fn pending_value() -> u8 {
        0 // Pending
    }
}

#[inline]
pub fn channel<T: State>() -> (Sender<T>, Receiver<T>) {
    let (notifier, receiver) = Sender::<T>::new();
    (notifier, receiver)
}

/// Inner state for one-shot completion notification
/// 
/// Uses AtomicPtr<Waker> for minimal overhead:
/// - Creation: Just initialize null pointer (no allocation)
/// - Send: Atomic swap and wake (minimal operations)
/// - Recv: Atomic store waker pointer (no intermediate futures)
/// 
/// Much lighter than both Notify (has waitlist) and AtomicWaker (has state machine)
/// 
/// 一次性完成通知的内部状态
/// 
/// 使用 AtomicPtr<Waker> 实现最小开销：
/// - 创建：仅初始化空指针（无分配）
/// - 发送：原子交换并唤醒（最少操作）
/// - 接收：原子存储 waker 指针（无中间 future）
/// 
/// 比 Notify（有等待列表）和 AtomicWaker（有状态机）都更轻量
pub(crate) struct Inner<T: State> {
    waker: AtomicPtr<Waker>,
    state: AtomicU8,
    _marker: std::marker::PhantomData<T>,
}

impl<T: State> Inner<T> {
    /// Create a new oneshot inner state
    /// 
    /// Extremely fast: just initializes null pointer and pending state
    /// 
    /// 创建一个新的 oneshot 内部状态
    /// 
    /// 极快：仅初始化空指针和待处理状态
    #[inline]
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            waker: AtomicPtr::new(ptr::null_mut()),
            state: AtomicU8::new(T::pending_value()),
            _marker: std::marker::PhantomData,
        })
    }
    
    /// Send a completion notification (set state and wake)
    /// 
    /// Optimized: atomically swap waker and wake if exists
    /// 
    /// 发送完成通知（设置状态并唤醒）
    /// 
    /// 优化：原子交换 waker 并在存在时唤醒
    #[inline]
    pub(crate) fn send(&self, state: T) {
        // Store state first with Release ordering
        self.state.store(state.to_u8(), Ordering::Release);
        
        // Atomically take the waker (if any) and wake it
        let waker_ptr = self.waker.swap(ptr::null_mut(), Ordering::AcqRel);
        if !waker_ptr.is_null() {
            // SAFETY: We know this pointer was created by Box::into_raw in register_waker
            // and we're the only one who can swap it out
            unsafe {
                let waker = Box::from_raw(waker_ptr);
                waker.wake();
            }
        }
    }
    
    /// Register a waker to be notified on completion
    /// 
    /// 注册一个 waker 以在完成时收到通知
    #[inline]
    fn register_waker(&self, waker: &Waker) {
        // Create a boxed waker and convert to raw pointer
        let new_waker = Box::into_raw(Box::new(waker.clone()));
        
        // Atomically swap the old waker with the new one
        let old_waker = self.waker.swap(new_waker, Ordering::AcqRel);
        
        // Drop the old waker if it exists
        if !old_waker.is_null() {
            // SAFETY: This pointer was created by Box::into_raw
            unsafe {
                let _ = Box::from_raw(old_waker);
            }
        }
    }
}

impl<T: State> Drop for Inner<T> {
    fn drop(&mut self) {
        // Clean up any remaining waker
        let waker_ptr = self.waker.load(Ordering::Acquire);
        if !waker_ptr.is_null() {
            // SAFETY: This pointer was created by Box::into_raw and we're dropping
            unsafe {
                let _ = Box::from_raw(waker_ptr);
            }
        }
    }
}

/// Completion notifier for one-shot tasks
/// 
/// Optimized implementation using AtomicPtr<Waker> + AtomicU8:
/// - Faster creation than Notify (no waitlist allocation) or AtomicWaker (no state machine)
/// - Lower memory footprint (just two atomic fields)
/// - Direct waker management without intermediate futures
/// - Single Arc allocation for both sender and receiver
/// 
/// 一次性任务完成通知器
/// 
/// 使用 AtomicPtr<Waker> + AtomicU8 的优化实现：
/// - 比 Notify（无等待列表分配）或 AtomicWaker（无状态机）创建更快
/// - 更小的内存占用（仅两个原子字段）
/// - 直接管理 waker，无中间 future
/// - 发送器和接收器共享单个 Arc 分配
pub struct Sender<T: State = TaskCompletion> {
    inner: Arc<Inner<T>>,
}

impl<T: State> Sender<T> {
    /// Create a new oneshot completion notifier with receiver
    /// 
    /// 创建一个新的 oneshot 完成通知器和接收器
    /// 
    /// # Returns
    /// Returns a tuple of (notifier, receiver)
    /// 
    /// 返回 (通知器, 接收器) 元组
    #[inline]
    pub fn new() -> (Self, Receiver<T>) {
        let inner = Inner::new();
        
        let notifier = Sender {
            inner: inner.clone(),
        };
        let receiver = Receiver {
            inner,
        };
        
        (notifier, receiver)
    }
    
    /// Notify completion with the given state
    /// 
    /// 使用给定状态通知完成
    #[inline]
    pub fn notify(&self, state: T) {
        self.inner.send(state);
    }
}

/// Completion receiver for one-shot tasks
/// 
/// Implements `Future` directly, allowing direct `.await` usage on both owned values and mutable references
/// 
/// 一次性任务完成通知接收器
/// 
/// 直接实现了 `Future`，允许对拥有的值和可变引用都直接使用 `.await`
/// 
/// # Examples
/// 
/// ## Using default TaskCompletion type
/// 
/// ```rust,ignore
/// let (notifier, receiver) = Sender::new();
/// 
/// tokio::spawn(async move {
///     notifier.notify(TaskCompletion::Called);
/// });
/// 
/// // Two equivalent ways to await:
/// let result = receiver.await;               // Direct await via Future impl
/// // or
/// let result = receiver.wait().await;        // Explicit wait method
/// ```
/// 
/// ## Awaiting on mutable reference
/// 
/// ```rust,ignore
/// let (notifier, mut receiver) = Sender::new();
/// 
/// tokio::spawn(async move {
///     notifier.notify(TaskCompletion::Called);
/// });
/// 
/// // Can also await on &mut receiver
/// let result = (&mut receiver).await;
/// ```
/// 
/// ## Using unit type for simple completion
/// 
/// ```rust,ignore
/// let (notifier, receiver) = Sender::<()>::new();
/// 
/// tokio::spawn(async move {
///     // ... do work ...
///     notifier.notify(());  // Signal completion
/// });
/// 
/// receiver.await;  // Wait for completion
/// ```
/// 
/// ## Using custom state
/// 
/// ```rust,ignore
/// let (notifier, receiver) = Sender::<CustomState>::new();
/// 
/// tokio::spawn(async move {
///     notifier.notify(CustomState::Success);
/// });
/// 
/// match receiver.await {
///     CustomState::Success => println!("Success!"),
///     CustomState::Failure => println!("Failed"),
///     CustomState::Timeout => println!("Timed out"),
/// }
/// ```
pub struct Receiver<T: State = TaskCompletion> {
    inner: Arc<Inner<T>>,
}

// Receiver is Unpin because all its fields are Unpin
impl<T: State> Unpin for Receiver<T> {}

impl<T: State> Receiver<T> {
    /// Wait for task completion asynchronously
    /// 
    /// This is equivalent to using `.await` directly on the receiver
    /// 
    /// 异步等待任务完成
    /// 
    /// 这等同于直接在 receiver 上使用 `.await`
    /// 
    /// # Returns
    /// Returns the completion state
    /// 
    /// # 返回值
    /// 返回完成状态
    #[inline]
    pub async fn wait(self) -> T {
        self.await
    }
}

/// Direct Future implementation for Receiver
/// 
/// This allows both `receiver.await` and `(&mut receiver).await` to work
/// 
/// Optimized implementation:
/// - Fast path: Immediate return if already completed (no allocation)
/// - Slow path: Direct waker registration (simpler than Notify's waitlist)
/// - No intermediate future state needed
/// 
/// 为 Receiver 直接实现 Future
/// 
/// 这允许 `receiver.await` 和 `(&mut receiver).await` 都能工作
/// 
/// 优化实现：
/// - 快速路径：如已完成则立即返回（无分配）
/// - 慢速路径：直接注册 waker（比 Notify 的等待列表更简单）
/// - 无需中间 future 状态
impl<T: State> Future for Receiver<T> {
    type Output = T;
    
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // SAFETY: Receiver is Unpin, so we can safely get a mutable reference
        let this = self.get_mut();
        
        // Fast path: check if already completed
        let current = this.inner.state.load(Ordering::Acquire);
        if let Some(state) = T::from_u8(current) {
            if current != T::pending_value() {
                return Poll::Ready(state);
            }
        }
        
        // Slow path: register waker for notification
        // This is much lighter than Notify's waitlist management
        this.inner.register_waker(cx.waker());
        
        // Check again after registering waker to avoid race condition
        // The sender might have completed between our first check and waker registration
        let current = this.inner.state.load(Ordering::Acquire);
        if let Some(state) = T::from_u8(current) {
            if current != T::pending_value() {
                return Poll::Ready(state);
            }
        }
        
        Poll::Pending
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_oneshot_called() {
        let (notifier, receiver) = Sender::new();
        
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            notifier.notify(TaskCompletion::Called);
        });
        
        let result = receiver.wait().await;
        assert_eq!(result, TaskCompletion::Called);
    }
    
    #[tokio::test]
    async fn test_oneshot_cancelled() {
        let (notifier, receiver) = Sender::new();
        
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            notifier.notify(TaskCompletion::Cancelled);
        });
        
        let result = receiver.wait().await;
        assert_eq!(result, TaskCompletion::Cancelled);
    }
    
    #[tokio::test]
    async fn test_oneshot_immediate_called() {
        let (notifier, receiver) = Sender::new();
        
        // Notify before waiting (fast path)
        notifier.notify(TaskCompletion::Called);
        
        let result = receiver.wait().await;
        assert_eq!(result, TaskCompletion::Called);
    }
    
    #[tokio::test]
    async fn test_oneshot_immediate_cancelled() {
        let (notifier, receiver) = Sender::new();
        
        // Notify before waiting (fast path)
        notifier.notify(TaskCompletion::Cancelled);
        
        let result = receiver.wait().await;
        assert_eq!(result, TaskCompletion::Cancelled);
    }
    
    // Test with custom state type
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum CustomState {
        Success,
        Failure,
        Timeout,
    }
    
    impl State for CustomState {
        fn to_u8(&self) -> u8 {
            match self {
                CustomState::Success => 1,
                CustomState::Failure => 2,
                CustomState::Timeout => 3,
            }
        }
        
        fn from_u8(value: u8) -> Option<Self> {
            match value {
                1 => Some(CustomState::Success),
                2 => Some(CustomState::Failure),
                3 => Some(CustomState::Timeout),
                _ => None,
            }
        }
        
        fn pending_value() -> u8 {
            0
        }
    }
    
    #[tokio::test]
    async fn test_oneshot_custom_state() {
        let (notifier, receiver) = Sender::<CustomState>::new();
        
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            notifier.notify(CustomState::Success);
        });
        
        let result = receiver.wait().await;
        assert_eq!(result, CustomState::Success);
    }
    
    #[tokio::test]
    async fn test_oneshot_custom_state_timeout() {
        let (notifier, receiver) = Sender::<CustomState>::new();
        
        // Immediate notification
        notifier.notify(CustomState::Timeout);
        
        let result = receiver.wait().await;
        assert_eq!(result, CustomState::Timeout);
    }
    
    #[tokio::test]
    async fn test_oneshot_unit_type() {
        let (notifier, receiver) = Sender::<()>::new();
        
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            notifier.notify(());
        });
        
        let result = receiver.wait().await;
        assert_eq!(result, ());
    }
    
    #[tokio::test]
    async fn test_oneshot_unit_type_immediate() {
        let (notifier, receiver) = Sender::<()>::new();
        
        // Immediate notification (fast path)
        notifier.notify(());
        
        let result = receiver.wait().await;
        assert_eq!(result, ());
    }
    
    // Tests for IntoFuture implementation
    #[tokio::test]
    async fn test_oneshot_into_future_called() {
        let (notifier, receiver) = Sender::new();
        
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            notifier.notify(TaskCompletion::Called);
        });
        
        // Direct await without .wait()
        let result = receiver.await;
        assert_eq!(result, TaskCompletion::Called);
    }
    
    #[tokio::test]
    async fn test_oneshot_into_future_immediate() {
        let (notifier, receiver) = Sender::new();
        
        // Notify before awaiting (fast path)
        notifier.notify(TaskCompletion::Cancelled);
        
        // Direct await
        let result = receiver.await;
        assert_eq!(result, TaskCompletion::Cancelled);
    }
    
    #[tokio::test]
    async fn test_oneshot_into_future_unit_type() {
        let (notifier, receiver) = Sender::<()>::new();
        
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            notifier.notify(());
        });
        
        // Direct await with unit type
        let result = receiver.await;
        assert_eq!(result, ());
    }
    
    #[tokio::test]
    async fn test_oneshot_into_future_custom_state() {
        let (notifier, receiver) = Sender::<CustomState>::new();
        
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            notifier.notify(CustomState::Failure);
        });
        
        // Direct await with custom state
        let result = receiver.await;
        assert_eq!(result, CustomState::Failure);
    }
    
    // Test awaiting on &mut receiver
    #[tokio::test]
    async fn test_oneshot_await_mut_reference() {
        let (notifier, mut receiver) = Sender::new();
        
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            notifier.notify(TaskCompletion::Called);
        });
        
        // Await on mutable reference
        let result = (&mut receiver).await;
        assert_eq!(result, TaskCompletion::Called);
    }
    
    #[tokio::test]
    async fn test_oneshot_await_mut_reference_unit_type() {
        let (notifier, mut receiver) = Sender::<()>::new();
        
        // Immediate notification
        notifier.notify(());
        
        // Await on mutable reference (fast path)
        let result = (&mut receiver).await;
        assert_eq!(result, ());
    }
}

