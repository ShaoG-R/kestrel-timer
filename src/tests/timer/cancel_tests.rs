use crate::task::{CallbackWrapper, TimerTask};
use crate::timer::TimerWheel;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

#[tokio::test]
async fn test_cancel_timer() {
    let timer = TimerWheel::with_defaults();
    let counter = Arc::new(AtomicU32::new(0));
    let counter_clone = Arc::clone(&counter);

    let task = TimerTask::new_oneshot(
        Duration::from_millis(100),
        Some(CallbackWrapper::new(move || {
            let counter = Arc::clone(&counter_clone);
            async move {
                counter.fetch_add(1, Ordering::SeqCst);
            }
        })),
    );
    let allocate_handle = timer.allocate_handle();
    let handle = timer.register(allocate_handle, task);

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
    let timer = TimerWheel::with_defaults();
    let counter = Arc::new(AtomicU32::new(0));
    let counter_clone = Arc::clone(&counter);

    let task = TimerTask::new_oneshot(
        Duration::from_millis(100),
        Some(CallbackWrapper::new(move || {
            let counter = Arc::clone(&counter_clone);
            async move {
                counter.fetch_add(1, Ordering::SeqCst);
            }
        })),
    );
    let allocate_handle = timer.allocate_handle();
    let handle = timer.register(allocate_handle, task);

    // Immediately cancel
    // 立即取消
    let cancel_result = handle.cancel();
    assert!(cancel_result);

    // Wait for enough time to ensure timer does not trigger
    // 等待足够时间确保定时器不触发
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(counter.load(Ordering::SeqCst), 0);
}


