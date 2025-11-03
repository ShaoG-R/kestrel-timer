/// High-performance async SPSC (Single Producer Single Consumer) channel
/// 
/// Built on top of custom SmallVec-based ring buffer for optimal performance.
/// Optimized for low latency and fast creation, designed to replace tokio mpsc in timer implementation.
/// 
/// 高性能异步 SPSC（单生产者单消费者）通道
/// 
/// 基于自定义 SmallVec 环形缓冲区构建以获得最佳性能。
/// 针对低延迟和快速创建进行优化，用于替代定时器实现中的 tokio mpsc。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use tokio::sync::Notify;
use parking_lot::Mutex;
use super::ringbuf;

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
        producer: Mutex::new(producer),
        consumer: Mutex::new(consumer),
        closed: AtomicBool::new(false),
        recv_notify: Notify::new(),
        send_notify: Notify::new(),
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
/// SPSC 通道的共享内部状态
struct Inner<T> {
    /// Producer (wrapped in Mutex for interior mutability)
    /// 
    /// 生产者（用 Mutex 包装以实现内部可变性）
    producer: Mutex<ringbuf::Producer<T>>,
    
    /// Consumer (wrapped in Mutex for interior mutability)
    /// 
    /// 消费者（用 Mutex 包装以实现内部可变性）
    consumer: Mutex<ringbuf::Consumer<T>>,
    
    /// Channel closed flag
    /// 
    /// 通道关闭标志
    closed: AtomicBool,
    
    /// Notifier for receiver waiting
    /// 
    /// 接收者等待通知器
    recv_notify: Notify,
    
    /// Notifier for sender waiting (when buffer is full)
    /// 
    /// 发送者等待通知器（当缓冲区满时）
    send_notify: Notify,
}

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
        
        // Try to push directly to producer
        // 直接尝试推送到生产者
        let mut producer = self.inner.producer.lock();
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
        // Try to pop directly from consumer
        // 直接尝试从消费者弹出
        let mut consumer = self.inner.consumer.lock();
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
        self.inner.consumer.lock().is_empty()
    }
    
    /// Get the number of messages currently in the channel
    /// 
    /// 获取通道中当前的消息数量
    #[inline]
    pub fn len(&self) -> usize {
        self.inner.consumer.lock().slots()
    }
    
    /// Get the capacity of the channel
    /// 
    /// 获取通道的容量
    #[inline]
    pub fn capacity(&self) -> usize {
        self.inner.consumer.lock().buffer().capacity()
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
