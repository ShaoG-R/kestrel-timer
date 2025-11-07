//! # High-Performance Async Timer System
//!
//! High-performance async timer based on Timing Wheel algorithm, supports tokio runtime
//!
//! ## Features
//!
//! - **High Performance**: Uses timing wheel algorithm, insert and delete operations are O(1)
//! - **Large-Scale Support**: Efficiently manages 10000+ concurrent timers
//! - **Async Support**: Based on tokio async runtime
//! - **Thread-Safe**: Uses parking_lot for high-performance locking mechanism
//!
//! 
//! # 高性能异步定时器库
//! 
//! 基于分层时间轮算法的高性能异步定时器库，支持 tokio 运行时
//! 
//! ## 特性
//! 
//! - **高性能**: 使用时间轮算法，插入和删除操作均为 O(1)
//! - **大规模支持**: 高效管理 10000+ 并发定时器
//! - **异步支持**: 基于 tokio 异步运行时
//! - **线程安全**: 使用 parking_lot 提供高性能的锁机制
//! 
//! ## Quick Start (快速开始)
//!
//! ```no_run
//! use kestrel_timer::{TimerWheel, CallbackWrapper, TimerTask};
//! use std::time::Duration;
//! use std::sync::Arc;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     // Create timer manager (创建定时器管理器)
//!     let timer = TimerWheel::with_defaults();
//!     
//!     // Step 1: Allocate handle to get task_id (分配 handle 获取 task_id)
//!     let handle = timer.allocate_handle();
//!     let task_id = handle.task_id();
//!     
//!     // Step 2: Create timer task (创建定时器任务)
//!     let callback = Some(CallbackWrapper::new(|| async {
//!         println!("Timer fired after 1 second!");
//!     }));
//!     let task = TimerTask::new_oneshot(Duration::from_secs(1), callback);
//!     
//!     // Step 3: Register timer task and get completion notification (注册定时器任务并获取完成通知)
//!     let timer_handle = timer.register(handle, task);
//!     
//!     // Wait for timer completion (等待定时器完成)
//!     use kestrel_timer::CompletionReceiver;
//!     let (rx, _handle) = timer_handle.into_parts();
//!     match rx {
//!         CompletionReceiver::OneShot(receiver) => {
//!             receiver.wait().await;
//!         },
//!         _ => {}
//!     }
//!     Ok(())
//! }
//! ```
//!
//! ## English Architecture Description
//!
//! ### Timing Wheel Algorithm
//!
//! Uses hierarchical timing wheel algorithm with L0 and L1 layers:
//!
//! - **L0 Layer (Bottom)**: Handles short delay tasks
//!   - Slot count: Default 512, configurable, must be power of 2
//!   - Time precision: Default 10ms, configurable
//!   - Maximum time span: 5.12 seconds
//!
//! - **L1 Layer (Upper)**: Handles long delay tasks
//!   - Slot count: Default 64, configurable, must be power of 2
//!   - Time precision: Default 1 second, configurable
//!   - Maximum time span: 64 seconds
//!
//! - **Round Mechanism**: Tasks beyond L1 range use round counting
//!
//! ### Task Indexing with DeferredMap
//!
//! Uses `DeferredMap` (a generational arena) for efficient task management:
//!
//! - **Two-Step Registration**: 
//!   1. Allocate handle to get task ID (cheap, no value needed)
//!   2. Insert task using the handle (with completion notifiers)
//!
//! - **Generational Safety**: Each task ID includes:
//!   - Lower 32 bits: Slot index
//!   - Upper 32 bits: Generation counter
//!   - Prevents use-after-free and ABA problems
//!
//! - **Memory Efficiency**: Slots use union-based storage
//!   - Occupied slots: Store task data
//!   - Vacant slots: Store free-list pointer
//!
//! ### Performance Optimization
//!
//! - Uses `parking_lot::Mutex` instead of standard library Mutex for better performance
//! - Uses `DeferredMap` (generational arena) for task indexing:
//!   - O(1) task lookup, insertion, and removal
//!   - Generational indices prevent use-after-free bugs
//!   - Memory-efficient slot reuse with union-based storage
//!   - Deferred insertion allows getting task ID before inserting task
//! - Slot count is power of 2, uses bitwise operations to optimize modulo
//! - Task execution in separate tokio tasks to avoid blocking timing wheel advancement
//! 
//! 
//! 
//! ## 中文架构说明
//!
//! ### 时间轮算法
//!
//! 采用分层时间轮（Hierarchical Timing Wheel）算法，包含 L0 和 L1 两层：
//!
//! - **L0 层（底层）**: 处理短延迟任务
//!   - 槽位数量: 默认 512 个（可配置，必须是 2 的幂次方）
//!   - 时间精度: 默认 10ms（可配置）
//!   - 最大时间跨度: 5.12 秒
//!
//! - **L1 层（高层）**: 处理长延迟任务
//!   - 槽位数量: 默认 64 个（可配置，必须是 2 的幂次方）
//!   - 时间精度: 默认 1 秒（可配置）
//!   - 最大时间跨度: 64 秒
//!
//! - **轮次机制**: 超出 L1 层范围的任务使用轮次计数处理
//!
//! ### 基于 DeferredMap 的任务索引
//!
//! 使用 `DeferredMap`（代数竞技场）实现高效任务管理：
//!
//! - **两步注册流程**：
//!   1. 分配 handle 获取任务 ID（轻量操作，无需准备任务值）
//!   2. 使用 handle 插入任务（携带完成通知器）
//!
//! - **代数安全**: 每个任务 ID 包含：
//!   - 低 32 位：槽位索引
//!   - 高 32 位：代数计数器
//!   - 防止释放后使用和 ABA 问题
//!
//! - **内存高效**: 槽位使用联合体存储
//!   - 已占用槽位：存储任务数据
//!   - 空闲槽位：存储空闲链表指针
//!
//! ### 性能优化
//!
//! - 使用 `parking_lot::Mutex` 替代标准库的 Mutex，提供更好的性能
//! - 使用 `DeferredMap`（代数竞技场）进行任务索引：
//!   - O(1) 任务查找、插入和删除
//!   - 代数索引防止释放后使用（use-after-free）错误
//!   - 基于联合体的槽位存储，内存高效复用
//!   - 延迟插入允许在插入任务前获取任务 ID
//! - 槽位数量为 2 的幂次方，使用位运算优化取模操作
//! - 任务执行在独立的 tokio 任务中，避免阻塞时间轮推进
//! 

pub mod config;
pub mod error;
pub mod task;
pub mod wheel;
pub mod timer;
mod service;

#[cfg(test)]
mod tests;

// Re-export public API
pub use task::{CallbackWrapper, TaskId, TimerTask, TaskCompletion};
pub use timer::handle::{TimerHandle, TimerHandleWithCompletion, BatchHandle, BatchHandleWithCompletion};
pub use task::CompletionReceiver;
pub use timer::TimerWheel;
pub use service::{TimerService, TaskNotification};

#[cfg(test)]
mod integration_tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    #[tokio::test]
    async fn test_basic_timer() {
        let timer = TimerWheel::with_defaults();
        let counter = Arc::new(AtomicU32::new(0));
        let counter_clone = Arc::clone(&counter);

        let handle = timer.allocate_handle();
        let task = TimerTask::new_oneshot(
            Duration::from_millis(50),
            Some(CallbackWrapper::new(move || {
                let counter =  Arc::clone(&counter_clone);
                async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                }
            })),
        );
        timer.register(handle, task);

        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_multiple_timers() {
        let timer = TimerWheel::with_defaults();
        let counter = Arc::new(AtomicU32::new(0));

        // Create 10 timers
        for i in 0..10 {
            let counter_clone = Arc::clone(&counter);
            let handle = timer.allocate_handle();
            let task = TimerTask::new_oneshot(
                Duration::from_millis(10 * (i + 1)),
                Some(CallbackWrapper::new(move || {
                    let counter = Arc::clone(&counter_clone);
                    async move {
                        counter.fetch_add(1, Ordering::SeqCst);
                    }
                })),
            );
            timer.register(handle, task);
        }

        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(counter.load(Ordering::SeqCst), 10);
    }

    #[tokio::test]
    async fn test_timer_cancellation() {
        let timer = TimerWheel::with_defaults();
        let counter = Arc::new(AtomicU32::new(0));

        // Create 5 timers
        let mut handles = Vec::new();
        for _ in 0..5 {
            let counter_clone = Arc::clone(&counter);
            let alloc_handle = timer.allocate_handle();
            let task = TimerTask::new_oneshot(
                Duration::from_millis(100),
                Some(CallbackWrapper::new(move || {
                    let counter = Arc::clone(&counter_clone);
                    async move {
                        counter.fetch_add(1, Ordering::SeqCst);
                    }
                })),
            );
            let handle = timer.register(alloc_handle, task);
            handles.push(handle);
        }

        // Cancel first 3 timers
        for i in 0..3 {
            let cancel_result = handles[i].cancel();
            assert!(cancel_result);
        }

        tokio::time::sleep(Duration::from_millis(200)).await;
        // Only 2 timers should be triggered
        assert_eq!(counter.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn test_completion_notification_once() {
        let timer = TimerWheel::with_defaults();
        let counter = Arc::new(AtomicU32::new(0));
        let counter_clone = Arc::clone(&counter);

        let alloc_handle = timer.allocate_handle();
        let task = TimerTask::new_oneshot(
            Duration::from_millis(50),
            Some(CallbackWrapper::new(move || {
                let counter = Arc::clone(&counter_clone);
                async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                }
            })),
        );
        let handle = timer.register(alloc_handle, task);

        // Wait for completion notification
        let (rx, _handle) = handle.into_parts();
        match rx {
            task::CompletionReceiver::OneShot(receiver) => {
                receiver.wait().await;
            },
            _ => panic!("Expected OneShot completion receiver"),
        }

        // Verify callback has been executed (wait a moment to ensure callback execution is complete)
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert_eq!(counter.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn test_notify_only_timer_once() {
        let timer = TimerWheel::with_defaults();
        
        let alloc_handle = timer.allocate_handle();
        let task = TimerTask::new_oneshot(Duration::from_millis(50), None);
        let handle = timer.register(alloc_handle, task);

        // Wait for completion notification (no callback, only notification)
        let (rx, _handle) = handle.into_parts();
        match rx {
            task::CompletionReceiver::OneShot(receiver) => {
                receiver.wait().await;
            },
            _ => panic!("Expected OneShot completion receiver"),
        }
    }

    #[tokio::test]
    async fn test_batch_completion_notifications() {
        let timer = TimerWheel::with_defaults();
        let counter = Arc::new(AtomicU32::new(0));

        // Step 1: Allocate handles
        let handles = timer.allocate_handles(5);

        // Step 2: Create batch callbacks
        let tasks: Vec<_> = (0..5)
            .map(|i| {
                let counter = Arc::clone(&counter);
                let delay = Duration::from_millis(50 + i as u64 * 10);
                let callback = CallbackWrapper::new(move || {
                    let counter = Arc::clone(&counter);
                    async move {
                        counter.fetch_add(1, Ordering::SeqCst);
                    }
                });
                TimerTask::new_oneshot(delay, Some(callback))
            })
            .collect();

        // Step 3: Batch register
        let batch = timer.register_batch(handles, tasks).expect("register_batch should succeed");
        let (receivers, _batch_handle) = batch.into_parts();

        // Wait for all completion notifications
        for rx in receivers {
            match rx {
                task::CompletionReceiver::OneShot(receiver) => {
                    receiver.wait().await;
                },
                _ => panic!("Expected OneShot completion receiver"),
            }
        }

        // Wait a moment to ensure callback execution is complete
        tokio::time::sleep(Duration::from_millis(50)).await;

        // Verify all callbacks have been executed
        assert_eq!(counter.load(Ordering::SeqCst), 5);
    }

    #[tokio::test]
    async fn test_completion_reason_expired() {
        let timer = TimerWheel::with_defaults();
        
        let alloc_handle = timer.allocate_handle();
        let task = TimerTask::new_oneshot(Duration::from_millis(50), None);
        let handle = timer.register(alloc_handle, task);

        // Wait for completion notification and verify reason is Expired
        let (rx, _handle) = handle.into_parts();
        let result = match rx {
            task::CompletionReceiver::OneShot(receiver) => {
                receiver.wait().await
            },
            _ => panic!("Expected OneShot completion receiver"),
        };
        assert_eq!(result, TaskCompletion::Called);
    }

    #[tokio::test]
    async fn test_completion_reason_cancelled() {
        let timer = TimerWheel::with_defaults();
        
        let alloc_handle = timer.allocate_handle();
        let task = TimerTask::new_oneshot(Duration::from_secs(10), None);
        let handle = timer.register(alloc_handle, task);

        // Cancel task
        let cancelled = handle.cancel();
        assert!(cancelled);

        // Wait for completion notification and verify reason is Cancelled
        let (rx, _handle) = handle.into_parts();
        let result = match rx {
            task::CompletionReceiver::OneShot(receiver) => {
                receiver.wait().await
            },
            _ => panic!("Expected OneShot completion receiver"),
        };
        assert_eq!(result, TaskCompletion::Cancelled);
    }

    #[tokio::test]
    async fn test_batch_completion_reasons() {
        let timer = TimerWheel::with_defaults();
        
        // Step 1: Allocate handles
        let handles = timer.allocate_handles(5);
        
        // Step 2: Create 5 tasks with 10 seconds delay
        let tasks: Vec<_> = (0..5)
            .map(|_| TimerTask::new_oneshot(Duration::from_secs(10), None))
            .collect();
        
        // Step 3: Batch register
        let batch = timer.register_batch(handles, tasks).expect("register_batch should succeed");
        let task_ids: Vec<_> = batch.task_ids().to_vec();
        let (mut receivers, _batch_handle) = batch.into_parts();

        // Cancel first 3 tasks
        timer.cancel_batch(&task_ids[0..3]);

        // Verify first 3 tasks received Cancelled notification
        for rx in receivers.drain(0..3) {
            let result = match rx {
                task::CompletionReceiver::OneShot(receiver) => {
                    receiver.wait().await
                },
                _ => panic!("Expected OneShot completion receiver"),
            };
            assert_eq!(result, TaskCompletion::Cancelled);
        }

        // Cancel remaining tasks and verify
        timer.cancel_batch(&task_ids[3..5]);
        for rx in receivers {
            let result = match rx {
                task::CompletionReceiver::OneShot(receiver) => {
                    receiver.wait().await
                },
                _ => panic!("Expected OneShot completion receiver"),
            };
            assert_eq!(result, TaskCompletion::Cancelled);
        }
    }
}
