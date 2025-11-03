/// Lightweight single-waiter notification primitive
/// 
/// Optimized for SPSC (Single Producer Single Consumer) pattern where
/// only one task waits at a time. Much lighter than tokio::sync::Notify.
/// 
/// 轻量级单等待者通知原语
/// 
/// 为 SPSC（单生产者单消费者）模式优化，其中每次只有一个任务等待。
/// 比 tokio::sync::Notify 更轻量。

use std::sync::atomic::{AtomicU8, AtomicPtr, Ordering};
use std::ptr;
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll, Waker};

// States for the notification
const EMPTY: u8 = 0;      // No waiter, no notification
const WAITING: u8 = 1;    // Waiter registered
const NOTIFIED: u8 = 2;   // Notification sent

/// Lightweight single-waiter notifier optimized for SPSC pattern
/// 
/// Much lighter than tokio::sync::Notify:
/// - No waitlist allocation (just one atomic pointer + one atomic state)
/// - Direct waker management (no intermediate state machine)
/// - Faster creation and notification
/// - Handles notification-before-wait correctly
/// 
/// 为 SPSC 模式优化的轻量级单等待者通知器
/// 
/// 比 tokio::sync::Notify 更轻量：
/// - 无需等待列表分配（仅一个原子指针 + 一个原子状态）
/// - 直接管理 waker（无复杂状态机）
/// - 更快的创建和通知速度
/// - 正确处理通知先于等待的情况
pub struct SingleWaiterNotify {
    state: AtomicU8,
    waker: AtomicPtr<Waker>,
}

impl SingleWaiterNotify {
    /// Create a new single-waiter notifier
    /// 
    /// 创建一个新的单等待者通知器
    #[inline]
    pub fn new() -> Self {
        Self {
            state: AtomicU8::new(EMPTY),
            waker: AtomicPtr::new(ptr::null_mut()),
        }
    }
    
    /// Returns a future that completes when notified
    /// 
    /// 返回一个在收到通知时完成的 future
    #[inline]
    pub fn notified(&self) -> Notified<'_> {
        Notified { 
            notify: self,
            registered: false,
        }
    }
    
    /// Wake the waiting task (if any)
    /// 
    /// If called before wait, the next wait will complete immediately.
    /// 
    /// 唤醒等待的任务（如果有）
    /// 
    /// 如果在等待之前调用，下一次等待将立即完成。
    #[inline]
    pub fn notify_one(&self) {
        // Mark as notified
        let prev_state = self.state.swap(NOTIFIED, Ordering::AcqRel);
        
        // If there was a waiter, wake it
        if prev_state == WAITING {
            let waker_ptr = self.waker.swap(ptr::null_mut(), Ordering::AcqRel);
            if !waker_ptr.is_null() {
                // SAFETY: This pointer was created by Box::into_raw in register_waker
                unsafe {
                    let waker = Box::from_raw(waker_ptr);
                    waker.wake();
                }
            }
        }
    }
    
    /// Register a waker to be notified
    /// 
    /// Returns true if already notified (fast path)
    /// 
    /// 注册一个 waker 以接收通知
    /// 
    /// 如果已经被通知则返回 true（快速路径）
    #[inline]
    fn register_waker(&self, waker: &Waker) -> bool {
        // Try to transition from EMPTY to WAITING
        match self.state.compare_exchange(
            EMPTY,
            WAITING,
            Ordering::AcqRel,
            Ordering::Acquire,
        ) {
            Ok(_) => {
                // Successfully transitioned, store the waker
                let new_waker = Box::into_raw(Box::new(waker.clone()));
                let old_waker = self.waker.swap(new_waker, Ordering::AcqRel);
                
                if !old_waker.is_null() {
                    // SAFETY: This pointer was created by Box::into_raw
                    unsafe {
                        let _ = Box::from_raw(old_waker);
                    }
                }
                false // Not notified yet
            }
            Err(state) => {
                // Already notified or waiting
                if state == NOTIFIED {
                    // Reset to EMPTY for next wait
                    self.state.store(EMPTY, Ordering::Release);
                    true // Already notified
                } else {
                    // State is WAITING, update the waker
                    let new_waker = Box::into_raw(Box::new(waker.clone()));
                    let old_waker = self.waker.swap(new_waker, Ordering::AcqRel);
                    
                    if !old_waker.is_null() {
                        // SAFETY: This pointer was created by Box::into_raw
                        unsafe {
                            let _ = Box::from_raw(old_waker);
                        }
                    }
                    
                    // Check if notified while we were updating waker
                    if self.state.load(Ordering::Acquire) == NOTIFIED {
                        self.state.store(EMPTY, Ordering::Release);
                        true
                    } else {
                        false
                    }
                }
            }
        }
    }
}

impl Drop for SingleWaiterNotify {
    fn drop(&mut self) {
        // Clean up any remaining waker
        let waker_ptr = self.waker.load(Ordering::Acquire);
        if !waker_ptr.is_null() {
            // SAFETY: This pointer was created by Box::into_raw
            unsafe {
                let _ = Box::from_raw(waker_ptr);
            }
        }
    }
}

/// Future returned by `SingleWaiterNotify::notified()`
/// 
/// `SingleWaiterNotify::notified()` 返回的 Future
pub struct Notified<'a> {
    notify: &'a SingleWaiterNotify,
    registered: bool,
}

impl Future for Notified<'_> {
    type Output = ();
    
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        // On first poll, register the waker
        if !self.registered {
            self.registered = true;
            if self.notify.register_waker(cx.waker()) {
                // Already notified (fast path)
                return Poll::Ready(());
            }
        } else {
            // On subsequent polls, check if notified
            if self.notify.state.load(Ordering::Acquire) == NOTIFIED {
                self.notify.state.store(EMPTY, Ordering::Release);
                return Poll::Ready(());
            }
            // Update waker in case it changed
            self.notify.register_waker(cx.waker());
        }
        
        Poll::Pending
    }
}

impl Drop for Notified<'_> {
    fn drop(&mut self) {
        if self.registered {
            // If we registered but are being dropped, try to clean up
            if self.notify.state.load(Ordering::Acquire) == WAITING {
                // Try to transition back to EMPTY
                let _ = self.notify.state.compare_exchange(
                    WAITING,
                    EMPTY,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tokio::time::{sleep, Duration};

    #[tokio::test]
    async fn test_notify_before_wait() {
        let notify = Arc::new(SingleWaiterNotify::new());
        
        // Notify before waiting
        notify.notify_one();
        
        // Should complete immediately
        notify.notified().await;
    }

    #[tokio::test]
    async fn test_notify_after_wait() {
        let notify = Arc::new(SingleWaiterNotify::new());
        let notify_clone = notify.clone();
        
        // Spawn a task that notifies after a delay
        tokio::spawn(async move {
            sleep(Duration::from_millis(10)).await;
            notify_clone.notify_one();
        });
        
        // Wait for notification
        notify.notified().await;
    }

    #[tokio::test]
    async fn test_multiple_notify_cycles() {
        let notify = Arc::new(SingleWaiterNotify::new());
        
        for _ in 0..10 {
            let notify_clone = notify.clone();
            tokio::spawn(async move {
                sleep(Duration::from_millis(5)).await;
                notify_clone.notify_one();
            });
            
            notify.notified().await;
        }
    }

    #[tokio::test]
    async fn test_concurrent_notify() {
        let notify = Arc::new(SingleWaiterNotify::new());
        let notify_clone = notify.clone();
        
        // Multiple notifiers (only one should wake the waiter)
        for _ in 0..5 {
            let n = notify_clone.clone();
            tokio::spawn(async move {
                sleep(Duration::from_millis(10)).await;
                n.notify_one();
            });
        }
        
        notify.notified().await;
    }

    #[tokio::test]
    async fn test_notify_no_waiter() {
        let notify = SingleWaiterNotify::new();
        
        // Notify with no waiter should not panic
        notify.notify_one();
        notify.notify_one();
        
        // Next wait should complete immediately
        notify.notified().await;
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn test_stress_test() {
        let notify = Arc::new(SingleWaiterNotify::new());
        
        for i in 0..100 {
            let notify_clone = notify.clone();
            tokio::spawn(async move {
                sleep(Duration::from_micros(i % 10)).await;
                notify_clone.notify_one();
            });
            
            notify.notified().await;
        }
    }

    #[tokio::test]
    async fn test_immediate_notification_race() {
        // Test the race between notification and registration
        for _ in 0..100 {
            let notify = Arc::new(SingleWaiterNotify::new());
            let notify_clone = notify.clone();
            
            let waiter = tokio::spawn(async move {
                notify.notified().await;
            });
            
            // Notify immediately (might happen before or after registration)
            notify_clone.notify_one();
            
            // Should complete without timeout
            tokio::time::timeout(Duration::from_millis(100), waiter)
                .await
                .expect("Should not timeout")
                .expect("Task should complete");
        }
    }
}

