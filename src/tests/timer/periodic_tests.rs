use crate::timer::TimerWheel;
use crate::task::{CallbackWrapper, TimerTask, CompletionReceiver, TaskCompletion};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

// ==================== Periodic Task Tests ====================
// ==================== 周期任务测试 ====================

#[tokio::test]
async fn test_periodic_basic() {
    let timer = TimerWheel::with_defaults();
    let counter = Arc::new(AtomicU32::new(0));
    let counter_clone = Arc::clone(&counter);

    // Create periodic task that triggers every 50ms
    // 创建每 50ms 触发一次的周期任务
    let task = TimerTask::new_periodic(
        Duration::from_millis(50),  // initial delay
        Duration::from_millis(50),  // interval
        Some(CallbackWrapper::new(move || {
            let counter = Arc::clone(&counter_clone);
            async move {
                counter.fetch_add(1, Ordering::SeqCst);
            }
        })),
        None,  // use default buffer size
    );
    let allocate_handle = timer.allocate_handle();
    let (mut rx, _handle) = timer.register(allocate_handle, task).into_parts();

    // Wait for 3 periodic executions (50ms initial + 50ms * 2)
    // 等待 3 次周期执行（50ms 初始 + 50ms * 2）
    tokio::time::sleep(Duration::from_millis(250)).await;
    
    // Verify callback was triggered multiple times
    // 验证回调被多次触发
    let count = counter.load(Ordering::SeqCst);
    assert!(count >= 3, "Expected at least 3 executions, got {}", count);
    
    // Verify completion notifications were sent
    // 验证完成通知已发送
    match rx {
        CompletionReceiver::Periodic(ref mut receiver) => {
            let mut notification_count = 0;
            while let Ok(completion) = receiver.try_recv() {
                assert_eq!(completion, TaskCompletion::Called);
                notification_count += 1;
            }
            assert!(notification_count >= 3, "Expected at least 3 notifications, got {}", notification_count);
        },
        _ => panic!("Expected Periodic completion receiver"),
    }
}

#[tokio::test]
async fn test_periodic_cancel() {
    let timer = TimerWheel::with_defaults();
    let counter = Arc::new(AtomicU32::new(0));
    let counter_clone = Arc::clone(&counter);

    // Create periodic task
    // 创建周期任务
    let task = TimerTask::new_periodic(
        Duration::from_millis(50),
        Duration::from_millis(50),
        Some(CallbackWrapper::new(move || {
            let counter = Arc::clone(&counter_clone);
            async move {
                counter.fetch_add(1, Ordering::SeqCst);
            }
        })),
        None,
    );
    let allocate_handle = timer.allocate_handle();
    let handle = timer.register(allocate_handle, task);

    // Wait for first execution
    // 等待第一次执行
    tokio::time::sleep(Duration::from_millis(80)).await;
    let count_before_cancel = counter.load(Ordering::SeqCst);
    assert!(count_before_cancel >= 1, "Expected at least 1 execution before cancel");

    // Cancel periodic task
    // 取消周期任务
    let cancelled = handle.cancel();
    assert!(cancelled);

    // Wait some more time and verify task stopped
    // 等待更多时间并验证任务已停止
    tokio::time::sleep(Duration::from_millis(150)).await;
    let count_after_cancel = counter.load(Ordering::SeqCst);
    
    // Count should not increase significantly after cancellation
    // 取消后计数不应该显著增加
    assert!(
        count_after_cancel - count_before_cancel <= 1,
        "Task should stop after cancel, before: {}, after: {}",
        count_before_cancel,
        count_after_cancel
    );
}

#[tokio::test]
async fn test_periodic_cancel_notification() {
    let timer = TimerWheel::with_defaults();
    let counter = Arc::new(AtomicU32::new(0));
    let counter_clone = Arc::clone(&counter);

    // Create periodic task
    // 创建周期任务
    let task = TimerTask::new_periodic(
        Duration::from_millis(50),
        Duration::from_millis(50),
        Some(CallbackWrapper::new(move || {
            let counter = Arc::clone(&counter_clone);
            async move {
                counter.fetch_add(1, Ordering::SeqCst);
            }
        })),
        None,
    );
    let allocate_handle = timer.allocate_handle();
    let (mut rx, handle) = timer.register(allocate_handle, task).into_parts();

    // Wait for first execution
    // 等待第一次执行
    tokio::time::sleep(Duration::from_millis(80)).await;

    // Cancel the task
    // 取消任务
    let cancelled = handle.cancel();
    assert!(cancelled);

    // Wait a bit for the cancel notification
    // 等待取消通知
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Verify we received cancellation notification
    // 验证收到取消通知
    match rx {
        CompletionReceiver::Periodic(ref mut receiver) => {
            let mut found_cancelled = false;
            while let Ok(completion) = receiver.try_recv() {
                if completion == TaskCompletion::Cancelled {
                    found_cancelled = true;
                    break;
                }
            }
            assert!(found_cancelled, "Expected to receive Cancelled notification");
        },
        _ => panic!("Expected Periodic completion receiver"),
    }
}

#[tokio::test]
async fn test_periodic_postpone() {
    let timer = TimerWheel::with_defaults();
    let counter = Arc::new(AtomicU32::new(0));
    let counter_clone = Arc::clone(&counter);

    // Create periodic task with 50ms initial delay and 50ms interval
    // 创建初始延迟 50ms 和间隔 50ms 的周期任务
    let task = TimerTask::new_periodic(
        Duration::from_millis(50),
        Duration::from_millis(50),
        Some(CallbackWrapper::new(move || {
            let counter = Arc::clone(&counter_clone);
            async move {
                counter.fetch_add(1, Ordering::SeqCst);
            }
        })),
        None,
    );
    let allocate_handle = timer.allocate_handle();
    let task_id = allocate_handle.task_id();
    let _handle = timer.register(allocate_handle, task);

    // Immediately postpone the task to 150ms
    // 立即推迟任务到 150ms
    let postponed = timer.postpone(task_id, Duration::from_millis(150), None);
    assert!(postponed);

    // Wait original time, should not trigger
    // 等待原始时间，不应触发
    tokio::time::sleep(Duration::from_millis(80)).await;
    assert_eq!(counter.load(Ordering::SeqCst), 0, "Task should not trigger at original time");

    // Wait for postponed time
    // 等待推迟后的时间
    tokio::time::sleep(Duration::from_millis(150)).await;
    
    // Verify task started executing
    // 验证任务已开始执行
    let count = counter.load(Ordering::SeqCst);
    assert!(count >= 1, "Task should trigger after postpone, got count: {}", count);
}

#[tokio::test]
async fn test_periodic_postpone_with_callback() {
    let timer = TimerWheel::with_defaults();
    let counter = Arc::new(AtomicU32::new(0));
    let counter_clone1 = Arc::clone(&counter);
    let counter_clone2 = Arc::clone(&counter);

    // Create periodic task, original callback adds 1
    // 创建周期任务，原始回调增加 1
    let task = TimerTask::new_periodic(
        Duration::from_millis(50),
        Duration::from_millis(50),
        Some(CallbackWrapper::new(move || {
            let counter = Arc::clone(&counter_clone1);
            async move {
                counter.fetch_add(1, Ordering::SeqCst);
            }
        })),
        None,
    );
    let allocate_handle = timer.allocate_handle();
    let task_id = allocate_handle.task_id();
    let _handle = timer.register(allocate_handle, task);

    // Postpone task and replace callback, new callback adds 10
    // 推迟任务并替换回调，新回调增加 10
    let postponed = timer.postpone(
        task_id,
        Duration::from_millis(100),
        Some(CallbackWrapper::new(move || {
            let counter = Arc::clone(&counter_clone2);
            async move {
                counter.fetch_add(10, Ordering::SeqCst);
            }
        })),
    );
    assert!(postponed);

    // Wait for task to trigger
    // 等待任务触发
    tokio::time::sleep(Duration::from_millis(200)).await;
    
    let count = counter.load(Ordering::SeqCst);
    // Should be at least 10 (new callback), not 1 (old callback)
    // 应该至少是 10（新回调），而不是 1（旧回调）
    assert!(count >= 10, "New callback should be used, got count: {}", count);
    
    // Verify it's a multiple of 10 (new callback)
    // 验证是 10 的倍数（新回调）
    assert_eq!(count % 10, 0, "Count should be multiple of 10, got: {}", count);
}

#[tokio::test]
async fn test_periodic_completion_receiver() {
    let timer = TimerWheel::with_defaults();
    let counter = Arc::new(AtomicU32::new(0));
    let counter_clone = Arc::clone(&counter);

    // Create periodic task
    // 创建周期任务
    let task = TimerTask::new_periodic(
        Duration::from_millis(50),
        Duration::from_millis(50),
        Some(CallbackWrapper::new(move || {
            let counter = Arc::clone(&counter_clone);
            async move {
                counter.fetch_add(1, Ordering::SeqCst);
            }
        })),
        None,
    );
    let allocate_handle = timer.allocate_handle();
    let (mut rx, _handle) = timer.register(allocate_handle, task).into_parts();

    // Continuously receive completion notifications
    // 持续接收完成通知
    let mut notification_count = 0;
    match rx {
        CompletionReceiver::Periodic(ref mut receiver) => {
            // Receive notifications for ~250ms
            // 接收约 250ms 的通知
            let timeout = tokio::time::sleep(Duration::from_millis(250));
            tokio::pin!(timeout);
            
            loop {
                tokio::select! {
                    _ = &mut timeout => break,
                    result = receiver.recv() => {
                        match result {
                            Some(completion) => {
                                assert_eq!(completion, TaskCompletion::Called);
                                notification_count += 1;
                            }
                            None => break,
                        }
                    }
                }
            }
        },
        _ => panic!("Expected Periodic completion receiver"),
    }

    // Should receive at least 3 notifications
    // 应该至少收到 3 次通知
    assert!(notification_count >= 3, "Expected at least 3 notifications, got {}", notification_count);
}

