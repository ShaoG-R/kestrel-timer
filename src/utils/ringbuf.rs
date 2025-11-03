/// High-performance SPSC Ring Buffer with SmallVec optimization
/// 
/// 基于 SmallVec 的高性能 SPSC 环形缓冲区
/// 
/// This implementation uses SmallVec to store data on the stack for small capacities (≤32),
/// avoiding heap allocation overhead and improving `new()` performance.
/// 
/// 此实现使用 SmallVec 在栈上存储小容量数据（≤32），避免堆分配开销，提升 `new()` 性能。

use std::mem::MaybeUninit;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use smallvec::SmallVec;

/// Ring buffer error for push operations
/// 
/// push 操作的环形缓冲区错误
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PushError<T> {
    /// Buffer is full
    /// 
    /// 缓冲区已满
    Full(T),
}

/// Ring buffer error for pop operations
/// 
/// pop 操作的环形缓冲区错误
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PopError {
    /// Buffer is empty
    /// 
    /// 缓冲区为空
    Empty,
}

/// Shared data between producer and consumer
/// 
/// 生产者和消费者之间的共享数据
pub struct SharedData<T> {
    /// Buffer storage using SmallVec for stack allocation optimization
    /// 
    /// 使用 SmallVec 的缓冲区存储，优化栈分配
    buffer: SmallVec<[MaybeUninit<T>; 32]>,
    
    /// Actual capacity (power of 2)
    /// 
    /// 实际容量（2 的幂次）
    capacity: usize,
    
    /// Mask for fast modulo operation (capacity - 1)
    /// 
    /// 快速取模运算的掩码（capacity - 1）
    mask: usize,
    
    /// Write index (accessed by producer, read by consumer)
    /// 
    /// 写入索引（由生产者访问，由消费者读取）
    write_idx: AtomicUsize,
    
    /// Read index (accessed by consumer, read by producer)
    /// 
    /// 读取索引（由消费者访问，由生产者读取）
    read_idx: AtomicUsize,
}

/// Producer half of the ring buffer
/// 
/// 环形缓冲区的生产者端
pub struct Producer<T> {
    /// Shared data
    /// 
    /// 共享数据
    shared: Arc<SharedData<T>>,
    
    /// Cached read index for performance (avoid reading atomic repeatedly)
    /// 
    /// 缓存的读索引以提升性能（避免重复读取原子变量）
    cached_read: usize,
}

/// Consumer half of the ring buffer
/// 
/// 环形缓冲区的消费者端
pub struct Consumer<T> {
    /// Shared data
    /// 
    /// 共享数据
    shared: Arc<SharedData<T>>,
    
    /// Cached write index for performance (avoid reading atomic repeatedly)
    /// 
    /// 缓存的写索引以提升性能（避免重复读取原子变量）
    cached_write: usize,
}

impl<T> SharedData<T> {
    /// Get the capacity of the buffer
    /// 
    /// 获取缓冲区容量
    #[inline]
    pub fn capacity(&self) -> usize {
        self.capacity
    }
}

/// Create a new ring buffer with the specified capacity
/// 
/// 创建指定容量的新环形缓冲区
/// 
/// # Parameters
/// - `capacity`: Desired capacity (will be rounded up to next power of 2)
/// 
/// # Returns
/// A tuple of (Producer, Consumer)
/// 
/// # Panics
/// Panics if capacity is 0
/// 
/// # 参数
/// - `capacity`: 期望容量（将向上取整到下一个 2 的幂次）
/// 
/// # 返回值
/// 返回 (Producer, Consumer) 元组
/// 
/// # Panics
/// 如果容量为 0 则 panic
pub fn new<T>(capacity: usize) -> (Producer<T>, Consumer<T>) {
        assert!(capacity > 0, "Capacity must be greater than 0");
        
        // Round up to next power of 2 for efficient masking
        // 向上取整到下一个 2 的幂次以实现高效的掩码操作
        let actual_capacity = capacity.next_power_of_two();
        let mask = actual_capacity - 1;
        
        // Initialize buffer with uninitialized memory - optimized for performance
        // 使用未初始化内存初始化缓冲区 - 性能优化版本
        // 
        // SAFETY: MaybeUninit<T> does not require initialization, so it's safe to 
        // set the length directly. This is much faster than resize_with or a loop.
        // 
        // 安全性：MaybeUninit<T> 不需要初始化，因此直接设置长度是安全的。
        // 这比 resize_with 或循环快得多。
        let buffer = {
            let mut buf = SmallVec::with_capacity(actual_capacity);
            unsafe {
                buf.set_len(actual_capacity);
            }
            buf
        };
        
        let shared = Arc::new(SharedData {
            buffer,
            capacity: actual_capacity,
            mask,
            write_idx: AtomicUsize::new(0),
            read_idx: AtomicUsize::new(0),
        });
        
        let producer = Producer {
            shared: shared.clone(),
            cached_read: 0,
        };
        
        let consumer = Consumer {
            shared,
            cached_write: 0,
        };
        
        (producer, consumer)
}

impl<T> Producer<T> {
    /// Push a value into the buffer
    /// 
    /// 向缓冲区推送一个值
    /// 
    /// # Errors
    /// Returns `PushError::Full` if the buffer is full
    /// 
    /// # 错误
    /// 如果缓冲区满则返回 `PushError::Full`
    #[inline]
    pub fn push(&mut self, value: T) -> Result<(), PushError<T>> {
        let write = self.shared.write_idx.load(Ordering::Relaxed);
        let mut read = self.cached_read;
        
        // Check if buffer is full
        // 检查缓冲区是否已满
        if write.wrapping_sub(read) >= self.shared.capacity {
            // Update cached read index from consumer
            // 从消费者更新缓存的读索引
            read = self.shared.read_idx.load(Ordering::Acquire);
            self.cached_read = read;
            
            if write.wrapping_sub(read) >= self.shared.capacity {
                return Err(PushError::Full(value));
            }
        }
        
        // Write value to buffer
        // 将值写入缓冲区
        let index = write & self.shared.mask;
        unsafe {
            let ptr = self.shared.buffer[index].as_ptr() as *mut T;
            ptr.write(value);
        }
        
        // Update write index with Release ordering to ensure visibility
        // 使用 Release 顺序更新写索引以确保可见性
        self.shared.write_idx.store(write.wrapping_add(1), Ordering::Release);
        
        Ok(())
    }
}

impl<T> Consumer<T> {
    /// Pop a value from the buffer
    /// 
    /// 从缓冲区弹出一个值
    /// 
    /// # Errors
    /// Returns `PopError::Empty` if the buffer is empty
    /// 
    /// # 错误
    /// 如果缓冲区空则返回 `PopError::Empty`
    #[inline]
    pub fn pop(&mut self) -> Result<T, PopError> {
        let read = self.shared.read_idx.load(Ordering::Relaxed);
        let mut write = self.cached_write;
        
        // Check if buffer is empty
        // 检查缓冲区是否为空
        if read == write {
            // Update cached write index from producer
            // 从生产者更新缓存的写索引
            write = self.shared.write_idx.load(Ordering::Acquire);
            self.cached_write = write;
            
            if read == write {
                return Err(PopError::Empty);
            }
        }
        
        // Read value from buffer
        // 从缓冲区读取值
        let index = read & self.shared.mask;
        let value = unsafe {
            let ptr = self.shared.buffer[index].as_ptr();
            ptr.read()
        };
        
        // Update read index with Release ordering to ensure visibility
        // 使用 Release 顺序更新读索引以确保可见性
        self.shared.read_idx.store(read.wrapping_add(1), Ordering::Release);
        
        Ok(value)
    }
    
    /// Check if the buffer is empty
    /// 
    /// 检查缓冲区是否为空
    #[inline]
    pub fn is_empty(&self) -> bool {
        let read = self.shared.read_idx.load(Ordering::Relaxed);
        let write = self.shared.write_idx.load(Ordering::Acquire);
        read == write
    }
    
    /// Get the number of elements currently in the buffer
    /// 
    /// 获取缓冲区中当前的元素数量
    #[inline]
    pub fn slots(&self) -> usize {
        let read = self.shared.read_idx.load(Ordering::Relaxed);
        let write = self.shared.write_idx.load(Ordering::Acquire);
        write.wrapping_sub(read)
    }
    
    /// Get a reference to the shared buffer data
    /// 
    /// 获取共享缓冲区数据的引用
    #[inline]
    pub fn buffer(&self) -> &SharedData<T> {
        &self.shared
    }
}

impl<T> Drop for Consumer<T> {
    fn drop(&mut self) {
        // Clean up any remaining elements in the buffer
        // 清理缓冲区中的剩余元素
        while self.pop().is_ok() {
            // Elements are dropped automatically
            // 元素自动被 drop
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_basic_push_pop() {
        let (mut producer, mut consumer) = new::<i32>(4);
        
        assert!(producer.push(1).is_ok());
        assert!(producer.push(2).is_ok());
        assert!(producer.push(3).is_ok());
        
        assert_eq!(consumer.pop().unwrap(), 1);
        assert_eq!(consumer.pop().unwrap(), 2);
        assert_eq!(consumer.pop().unwrap(), 3);
        assert!(consumer.pop().is_err());
    }
    
    #[test]
    fn test_capacity_rounding() {
        let (_, consumer) = new::<i32>(5);
        // 5 should round up to 8 (next power of 2)
        assert_eq!(consumer.buffer().capacity(), 8);
        
        let (_, consumer) = new::<i32>(32);
        assert_eq!(consumer.buffer().capacity(), 32);
        
        let (_, consumer) = new::<i32>(33);
        // 33 should round up to 64
        assert_eq!(consumer.buffer().capacity(), 64);
    }
    
    #[test]
    fn test_buffer_full() {
        let (mut producer, mut consumer) = new::<i32>(4);
        // Actual capacity is 4, but we can only store 3 items (one slot reserved)
        
        assert!(producer.push(1).is_ok());
        assert!(producer.push(2).is_ok());
        assert!(producer.push(3).is_ok());
        assert!(producer.push(4).is_ok());
        
        // Buffer should be full now
        assert!(matches!(producer.push(5), Err(PushError::Full(5))));
        
        // Pop one item to make space
        assert_eq!(consumer.pop().unwrap(), 1);
        
        // Now we should be able to push again
        assert!(producer.push(5).is_ok());
    }
    
    #[test]
    fn test_buffer_empty() {
        let (mut producer, mut consumer) = new::<i32>(4);
        
        assert!(consumer.pop().is_err());
        assert!(consumer.is_empty());
        
        producer.push(42).unwrap();
        assert!(!consumer.is_empty());
        
        consumer.pop().unwrap();
        assert!(consumer.is_empty());
    }
    
    #[test]
    fn test_slots() {
        let (mut producer, consumer) = new::<i32>(8);
        
        assert_eq!(consumer.slots(), 0);
        
        producer.push(1).unwrap();
        producer.push(2).unwrap();
        producer.push(3).unwrap();
        
        assert_eq!(consumer.slots(), 3);
    }
    
    #[test]
    fn test_wrap_around() {
        let (mut producer, mut consumer) = new::<i32>(4);
        
        // Fill and empty the buffer multiple times to test wrap-around
        for round in 0..10 {
            for i in 0..4 {
                producer.push(round * 10 + i).unwrap();
            }
            
            for i in 0..4 {
                assert_eq!(consumer.pop().unwrap(), round * 10 + i);
            }
        }
    }
    
    #[test]
    fn test_drop_cleanup() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::sync::Arc;
        
        #[derive(Debug)]
        struct DropCounter {
            counter: Arc<AtomicUsize>,
        }
        
        impl Drop for DropCounter {
            fn drop(&mut self) {
                self.counter.fetch_add(1, Ordering::SeqCst);
            }
        }
        
        let counter = Arc::new(AtomicUsize::new(0));
        
        {
            let (mut producer, consumer) = new(8);
            
            for _ in 0..5 {
                producer.push(DropCounter { counter: counter.clone() }).unwrap();
            }
            
            // Drop consumer, which should drop all remaining items
            drop(consumer);
        }
        
        // All 5 items should have been dropped
        assert_eq!(counter.load(Ordering::SeqCst), 5);
    }
    
    #[test]
    fn test_concurrent_access() {
        use std::thread;
        
        let (mut producer, mut consumer) = new::<u64>(128);
        
        let producer_handle = thread::spawn(move || {
            for i in 0..1000 {
                loop {
                    if producer.push(i).is_ok() {
                        break;
                    }
                    thread::yield_now();
                }
            }
        });
        
        let consumer_handle = thread::spawn(move || {
            let mut received = Vec::new();
            for _ in 0..1000 {
                loop {
                    match consumer.pop() {
                        Ok(val) => {
                            received.push(val);
                            break;
                        }
                        Err(_) => thread::yield_now(),
                    }
                }
            }
            received
        });
        
        producer_handle.join().unwrap();
        let received = consumer_handle.join().unwrap();
        
        // Verify all numbers were received in order
        assert_eq!(received.len(), 1000);
        for (i, &val) in received.iter().enumerate() {
            assert_eq!(val, i as u64);
        }
    }
    
    #[test]
    fn test_small_capacity_stack_allocation() {
        // Test that small capacities (≤32) use stack allocation
        // This test mainly ensures the code compiles and works with SmallVec
        let (mut producer, mut consumer) = new::<u8>(16);
        
        for i in 0..10 {
            producer.push(i).unwrap();
        }
        
        for i in 0..10 {
            assert_eq!(consumer.pop().unwrap(), i);
        }
    }
    
    #[test]
    fn test_large_capacity_heap_allocation() {
        // Test that large capacities (>32) work correctly with heap allocation
        let (mut producer, mut consumer) = new::<u8>(64);
        
        for i in 0..50 {
            producer.push(i).unwrap();
        }
        
        for i in 0..50 {
            assert_eq!(consumer.pop().unwrap(), i);
        }
    }
    
    #[test]
    #[should_panic(expected = "Capacity must be greater than 0")]
    fn test_zero_capacity_panics() {
        let _ = new::<i32>(0);
    }
}

