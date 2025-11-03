/// High-performance async SPSC (Single Producer Single Consumer) channel
/// 
/// Built on top of custom SmallVec-based ring buffer for optimal performance.
/// Optimized for low latency and fast creation, designed to replace tokio mpsc in timer implementation.
/// 
/// 高性能异步 SPSC（单生产者单消费者）通道
/// 
/// 基于自定义 SmallVec 环形缓冲区构建以获得最佳性能。
/// 针对低延迟和快速创建进行优化，用于替代定时器实现中的 tokio mpsc。
///
/// # 安全性说明 (Safety Notes)
///
/// 本实现使用 `UnsafeCell` 来提供零成本内部可变性，而不是 `Mutex`。
/// 这是安全的，基于以下保证：
///
/// 1. **单一所有权**：`Sender` 和 `Receiver` 都不实现 `Clone`，确保每个通道只有一个发送者和一个接收者
/// 2. **访问隔离**：`Producer` 只被唯一的 `Sender` 访问，`Consumer` 只被唯一的 `Receiver` 访问
/// 3. **无数据竞争**：由于单一所有权，不会有多个线程同时访问同一个 `Producer` 或 `Consumer`
/// 4. **原子通信**：`Producer` 和 `Consumer` 内部使用原子操作进行跨线程通信
/// 5. **类型系统保证**：通过类型系统强制 SPSC 语义，防止误用为 MPMC
///
/// 这种设计实现了零同步开销，完全消除了 `Mutex` 的性能损失。
///
/// # Safety Guarantees
///
/// This implementation uses `UnsafeCell` for zero-cost interior mutability instead of `Mutex`.
/// This is safe based on the following guarantees:
///
/// 1. **Single Ownership**: Neither `Sender` nor `Receiver` implements `Clone`, ensuring only one sender and one receiver per channel
/// 2. **Access Isolation**: `Producer` is only accessed by the unique `Sender`, `Consumer` only by the unique `Receiver`
/// 3. **No Data Races**: Due to single ownership, there's no concurrent access to the same `Producer` or `Consumer`
/// 4. **Atomic Communication**: `Producer` and `Consumer` use atomic operations internally for cross-thread communication
/// 5. **Type System Enforcement**: SPSC semantics are enforced by the type system, preventing misuse as MPMC
///
/// This design achieves zero synchronization overhead, completely eliminating `Mutex` performance costs.

use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use super::ringbuf;
use super::notify::SingleWaiterNotify;

/// SPSC channel creation function
/// 
/// Creates a bounded SPSC channel with the specified capacity.
/// 
/// # Parameters
/// - `capacity`: Channel capacity
/// 
/// # Returns
/// A tuple of (Sender, Receiver)
/// 
/// # Examples
/// 
/// ```
/// use kestrel_timer::utils::spsc::channel;
/// 
/// #[tokio::main]
/// async fn main() {
///     let (tx, rx) = channel(32);
///     
///     tokio::spawn(async move {
///         tx.send(42).await.unwrap();
///     });
///     
///     let value = rx.recv().await.unwrap();
///     assert_eq!(value, 42);
/// }
/// ```
/// 
/// SPSC 通道创建函数
/// 
/// 创建指定容量的有界 SPSC 通道。
/// 
/// # 参数
/// - `capacity`: 通道容量
/// 
/// # 返回值
/// 返回 (Sender, Receiver) 元组
pub fn channel<T>(capacity: usize) -> (Sender<T>, Receiver<T>) {
    assert!(capacity > 0, "Channel capacity must be greater than 0");
    
    let (producer, consumer) = ringbuf::new(capacity);
    
    let inner = Arc::new(Inner {
        producer: UnsafeCell::new(producer),
        consumer: UnsafeCell::new(consumer),
        closed: AtomicBool::new(false),
        recv_notify: SingleWaiterNotify::new(),
        send_notify: SingleWaiterNotify::new(),
    });
    
    let sender = Sender {
        inner: inner.clone(),
    };
    
    let receiver = Receiver {
        inner,
    };
    
    (sender, receiver)
}

/// Shared internal state for SPSC channel
/// 
/// Contains both shared state and the ring buffer halves.
/// Uses UnsafeCell for zero-cost interior mutability of Producer/Consumer.
/// 
/// SPSC 通道的共享内部状态
/// 
/// 包含共享状态和环形缓冲区的两端。
/// 使用 UnsafeCell 实现 Producer/Consumer 的零成本内部可变性。
struct Inner<T> {
    /// Producer (wrapped in UnsafeCell for zero-cost interior mutability)
    /// 
    /// 生产者（用 UnsafeCell 包装以实现零成本内部可变性）
    producer: UnsafeCell<ringbuf::Producer<T>>,
    
    /// Consumer (wrapped in UnsafeCell for zero-cost interior mutability)
    /// 
    /// 消费者（用 UnsafeCell 包装以实现零成本内部可变性）
    consumer: UnsafeCell<ringbuf::Consumer<T>>,
    
    /// Channel closed flag
    /// 
    /// 通道关闭标志
    closed: AtomicBool,
    
    /// Notifier for receiver waiting (lightweight single-waiter)
    /// 
    /// 接收者等待通知器（轻量级单等待者）
    recv_notify: SingleWaiterNotify,
    
    /// Notifier for sender waiting when buffer is full (lightweight single-waiter)
    /// 
    /// 发送者等待通知器，当缓冲区满时使用（轻量级单等待者）
    send_notify: SingleWaiterNotify,
}

// SAFETY: Inner<T> 可以在线程间安全共享的原因：
// 1. Sender 和 Receiver 都不实现 Clone，确保单一所有权
// 2. producer 只被唯一的 Sender 访问，不会有多线程竞争
// 3. consumer 只被唯一的 Receiver 访问，不会有多线程竞争
// 4. closed、recv_notify、send_notify 都已经是线程安全的
// 5. Producer 和 Consumer 内部使用原子操作进行跨线程通信
unsafe impl<T: Send> Sync for Inner<T> {}

/// SPSC channel sender
/// 
/// SPSC 通道发送器
pub struct Sender<T> {
    inner: Arc<Inner<T>>,
}

/// SPSC channel receiver
/// 
/// SPSC 通道接收器
pub struct Receiver<T> {
    inner: Arc<Inner<T>>,
}

/// Send error type
/// 
/// 发送错误类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendError<T> {
    /// Channel is closed
    /// 
    /// 通道已关闭
    Closed(T),
}

/// Try-receive error type
/// 
/// 尝试接收错误类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TryRecvError {
    /// Channel is empty
    /// 
    /// 通道为空
    Empty,
    
    /// Channel is closed
    /// 
    /// 通道已关闭
    Closed,
}

/// Try-send error type
/// 
/// 尝试发送错误类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrySendError<T> {
    /// Buffer is full
    /// 
    /// 缓冲区已满
    Full(T),
    
    /// Channel is closed
    /// 
    /// 通道已关闭
    Closed(T),
}

impl<T> Sender<T> {
    /// Send a message to the channel (async, waits if buffer is full)
    /// 
    /// # Errors
    /// Returns `SendError::Closed` if the receiver has been dropped
    /// 
    /// 向通道发送消息（异步，如果缓冲区满则等待）
    /// 
    /// # 错误
    /// 如果接收器已被丢弃，返回 `SendError::Closed`
    pub async fn send(&self, mut value: T) -> Result<(), SendError<T>> {
        loop {
            match self.try_send(value) {
                Ok(()) => return Ok(()),
                Err(TrySendError::Closed(v)) => return Err(SendError::Closed(v)),
                Err(TrySendError::Full(v)) => {
                    // Store the value to retry
                    // 存储值以便重试
                    value = v;
                    
                    // Wait for space to become available
                    // 等待空间可用
                    self.inner.send_notify.notified().await;
                    
                    // Check if channel was closed while waiting
                    // 检查等待时通道是否已关闭
                    if self.inner.closed.load(Ordering::Acquire) {
                        return Err(SendError::Closed(value));
                    }
                    
                    // Retry with the value in next loop iteration
                    // 在下一次循环迭代中使用该值重试
                }
            }
        }
    }
    
    /// Try to send a message without blocking
    /// 
    /// # Errors
    /// - Returns `TrySendError::Full` if the buffer is full
    /// - Returns `TrySendError::Closed` if the receiver has been dropped
    /// 
    /// 尝试非阻塞地发送消息
    /// 
    /// # 错误
    /// - 如果缓冲区满，返回 `TrySendError::Full`
    /// - 如果接收器已被丢弃，返回 `TrySendError::Closed`
    pub fn try_send(&self, value: T) -> Result<(), TrySendError<T>> {
        // Check if channel is closed first
        // 首先检查通道是否已关闭
        if self.inner.closed.load(Ordering::Acquire) {
            return Err(TrySendError::Closed(value));
        }
        
        // SAFETY: Sender 不实现 Clone，因此只有一个 Sender 实例
        // 不会有多个线程同时访问 producer
        let producer = unsafe { &mut *self.inner.producer.get() };
        
        match producer.push(value) {
            Ok(()) => {
                // Successfully sent, notify receiver
                // 成功发送，通知接收者
                self.inner.recv_notify.notify_one();
                Ok(())
            }
            Err(ringbuf::PushError::Full(v)) => {
                Err(TrySendError::Full(v))
            }
        }
    }
    
    /// Check if the channel is closed
    /// 
    /// 检查通道是否已关闭
    #[inline]
    pub fn is_closed(&self) -> bool {
        self.inner.closed.load(Ordering::Acquire)
    }
}

impl<T> Receiver<T> {
    /// Receive a message from the channel (async, waits if buffer is empty)
    /// 
    /// Returns `None` if the channel is closed and empty
    /// 
    /// 从通道接收消息（异步，如果缓冲区空则等待）
    /// 
    /// 如果通道已关闭且为空，返回 `None`
    pub async fn recv(&self) -> Option<T> {
        loop {
            match self.try_recv() {
                Ok(value) => return Some(value),
                Err(TryRecvError::Closed) => return None,
                Err(TryRecvError::Empty) => {
                    // Check if channel is closed before waiting
                    // 等待前检查通道是否已关闭
                    if self.inner.closed.load(Ordering::Acquire) {
                        // Double check if there are any remaining items
                        // 再次检查是否有剩余项
                        if let Ok(value) = self.try_recv() {
                            return Some(value);
                        }
                        return None;
                    }
                    
                    // Wait for data to become available
                    // 等待数据可用
                    self.inner.recv_notify.notified().await;
                }
            }
        }
    }
    
    /// Try to receive a message without blocking
    /// 
    /// # Errors
    /// - Returns `TryRecvError::Empty` if the buffer is empty
    /// - Returns `TryRecvError::Closed` if the sender has been dropped and buffer is empty
    /// 
    /// 尝试非阻塞地接收消息
    /// 
    /// # 错误
    /// - 如果缓冲区空，返回 `TryRecvError::Empty`
    /// - 如果发送器已被丢弃且缓冲区空，返回 `TryRecvError::Closed`
    pub fn try_recv(&self) -> Result<T, TryRecvError> {
        // SAFETY: Receiver 不实现 Clone，因此只有一个 Receiver 实例
        // 不会有多个线程同时访问 consumer
        let consumer = unsafe { &mut *self.inner.consumer.get() };
        
        match consumer.pop() {
            Ok(value) => {
                // Successfully received, notify sender
                // 成功接收，通知发送者
                self.inner.send_notify.notify_one();
                Ok(value)
            }
            Err(ringbuf::PopError::Empty) => {
                if self.inner.closed.load(Ordering::Acquire) {
                    Err(TryRecvError::Closed)
                } else {
                    Err(TryRecvError::Empty)
                }
            }
        }
    }
    
    /// Check if the channel is empty
    /// 
    /// 检查通道是否为空
    #[inline]
    pub fn is_empty(&self) -> bool {
        // SAFETY: Receiver 不实现 Clone，因此只有一个 Receiver 实例
        // is_empty 只读取数据，不需要可变访问，但我们通过 UnsafeCell 访问以保持一致性
        let consumer = unsafe { &*self.inner.consumer.get() };
        consumer.is_empty()
    }
    
    /// Get the number of messages currently in the channel
    /// 
    /// 获取通道中当前的消息数量
    #[inline]
    pub fn len(&self) -> usize {
        // SAFETY: Receiver 不实现 Clone，因此只有一个 Receiver 实例
        // slots 只读取数据，不需要可变访问，但我们通过 UnsafeCell 访问以保持一致性
        let consumer = unsafe { &*self.inner.consumer.get() };
        consumer.slots()
    }
    
    /// Get the capacity of the channel
    /// 
    /// 获取通道的容量
    #[inline]
    pub fn capacity(&self) -> usize {
        // SAFETY: Receiver 不实现 Clone，因此只有一个 Receiver 实例
        // capacity 只读取数据，不需要可变访问，但我们通过 UnsafeCell 访问以保持一致性
        let consumer = unsafe { &*self.inner.consumer.get() };
        consumer.buffer().capacity()
    }
}

impl<T> Drop for Receiver<T> {
    fn drop(&mut self) {
        // Mark channel as closed when receiver is dropped
        // 当接收器被丢弃时标记通道为已关闭
        self.inner.closed.store(true, Ordering::Release);
        
        // Notify sender in case it's waiting
        // 通知发送者以防它正在等待
        self.inner.send_notify.notify_one();
    }
}

impl<T> Drop for Sender<T> {
    fn drop(&mut self) {
        // Mark channel as closed when sender is dropped
        // 当发送器被丢弃时标记通道为已关闭
        self.inner.closed.store(true, Ordering::Release);
        
        // Notify receiver in case it's waiting
        // 通知接收器以防它正在等待
        self.inner.recv_notify.notify_one();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_basic_send_recv() {
        let (tx, rx) = channel(4);
        
        tx.send(1).await.unwrap();
        tx.send(2).await.unwrap();
        tx.send(3).await.unwrap();
        
        assert_eq!(rx.recv().await, Some(1));
        assert_eq!(rx.recv().await, Some(2));
        assert_eq!(rx.recv().await, Some(3));
    }
    
    #[tokio::test]
    async fn test_try_send_recv() {
        let (tx, rx) = channel(4);
        
        tx.try_send(1).unwrap();
        tx.try_send(2).unwrap();
        
        assert_eq!(rx.try_recv().unwrap(), 1);
        assert_eq!(rx.try_recv().unwrap(), 2);
        assert!(matches!(rx.try_recv(), Err(TryRecvError::Empty)));
    }
    
    #[tokio::test]
    async fn test_channel_closed_on_sender_drop() {
        let (tx, rx) = channel(4);
        
        tx.send(1).await.unwrap();
        drop(tx);
        
        assert_eq!(rx.recv().await, Some(1));
        assert_eq!(rx.recv().await, None);
    }
    
    #[tokio::test]
    async fn test_channel_closed_on_receiver_drop() {
        let (tx, rx) = channel::<i32>(4);
        
        drop(rx);
        
        assert!(matches!(tx.send(1).await, Err(SendError::Closed(1))));
    }
    
    #[tokio::test]
    async fn test_cross_task_communication() {
        let (tx, rx) = channel(4);
        
        let sender_handle = tokio::spawn(async move {
            for i in 0..10 {
                tx.send(i).await.unwrap();
            }
        });
        
        let receiver_handle = tokio::spawn(async move {
            let mut sum = 0;
            while let Some(value) = rx.recv().await {
                sum += value;
            }
            sum
        });
        
        sender_handle.await.unwrap();
        let sum = receiver_handle.await.unwrap();
        assert_eq!(sum, 45); // 0+1+2+...+9 = 45
    }
    
    #[tokio::test]
    async fn test_backpressure() {
        let (tx, rx) = channel(4);
        
        // Fill the buffer
        tx.try_send(1).unwrap();
        tx.try_send(2).unwrap();
        tx.try_send(3).unwrap();
        tx.try_send(4).unwrap();
        
        // Buffer should be full now
        assert!(matches!(tx.try_send(5), Err(TrySendError::Full(5))));
        
        // This should block and then succeed when we consume
        let send_handle = tokio::spawn(async move {
            tx.send(5).await.unwrap();
            tx.send(6).await.unwrap();
        });
        
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
        
        assert_eq!(rx.recv().await, Some(1));
        assert_eq!(rx.recv().await, Some(2));
        assert_eq!(rx.recv().await, Some(3));
        assert_eq!(rx.recv().await, Some(4));
        assert_eq!(rx.recv().await, Some(5));
        assert_eq!(rx.recv().await, Some(6));
        
        send_handle.await.unwrap();
    }
    
    #[tokio::test]
    async fn test_capacity_and_len() {
        let (tx, rx) = channel::<i32>(8);
        
        assert_eq!(rx.capacity(), 8);
        assert_eq!(rx.len(), 0);
        assert!(rx.is_empty());
        
        tx.try_send(1).unwrap();
        tx.try_send(2).unwrap();
        
        tokio::time::sleep(tokio::time::Duration::from_millis(1)).await;
        assert_eq!(rx.len(), 2);
        assert!(!rx.is_empty());
    }
}
