use crate::task::{CallbackWrapper, CompletionReceiver, TimerTask};
use crate::timer::TimerWheel;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

#[tokio::test]
async fn test_postpone_timer() {
    let timer = TimerWheel::with_defaults();
    let counter = Arc::new(AtomicU32::new(0));
    let counter_clone = Arc::clone(&counter);

    let task = TimerTask::new_oneshot(
        Duration::from_millis(50),
        Some(CallbackWrapper::new(move || {
            let counter = Arc::clone(&counter_clone);
            async move {
                counter.fetch_add(1, Ordering::SeqCst);
            }
        })),
    );
    let allocate_handle = timer.allocate_handle();
    let task_id = allocate_handle.task_id();
    let handle = timer.register(allocate_handle, task);

    // Postpone task to 150ms
    // 推迟任务到 150ms
    let postponed = timer.postpone(task_id, Duration::from_millis(150), None);
    assert!(postponed);

    // Wait for original time 50ms, task should not trigger
    // 等待原始时间 50ms，任务不应触发
    tokio::time::sleep(Duration::from_millis(70)).await;
    assert_eq!(counter.load(Ordering::SeqCst), 0);

    // Wait for new trigger time (from postponed start, need to wait about 150ms)
    // 等待新的触发时间（从推迟开始算起，大约需要等待 150ms）
    let (rx, _handle) = handle.into_parts();
    let result = match rx {
        CompletionReceiver::OneShot(receiver) => {
            tokio::time::timeout(Duration::from_millis(200), receiver.wait()).await
        }
        _ => panic!("Expected OneShot completion receiver"),
    };
    assert!(result.is_ok());

    // Wait for callback to execute
    // 等待回调执行
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert_eq!(counter.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn test_postpone_with_callback() {
    let timer = TimerWheel::with_defaults();
    let counter = Arc::new(AtomicU32::new(0));
    let counter_clone1 = Arc::clone(&counter);
    let counter_clone2 = Arc::clone(&counter);

    // Create task, original callback adds 1
    let task = TimerTask::new_oneshot(
        Duration::from_millis(50),
        Some(CallbackWrapper::new(move || {
            let counter = Arc::clone(&counter_clone1);
            async move {
                counter.fetch_add(1, Ordering::SeqCst);
            }
        })),
    );
    let allocate_handle = timer.allocate_handle();
    let task_id = allocate_handle.task_id();
    let handle = timer.register(allocate_handle, task);

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

    // Wait for task to trigger (after postponed, need to wait 100ms, plus margin)
    // 等待任务触发（推迟后，需要等待 100ms，加上余量）
    let (rx, _handle) = handle.into_parts();
    let result = match rx {
        CompletionReceiver::OneShot(receiver) => {
            tokio::time::timeout(Duration::from_millis(200), receiver.wait()).await
        }
        _ => panic!("Expected OneShot completion receiver"),
    };
    assert!(result.is_ok());

    // Wait for callback to execute
    // 等待回调执行
    tokio::time::sleep(Duration::from_millis(20)).await;

    // Verify new callback is executed (increased 10 instead of 1)
    // 验证新回调已执行（增加 10 而不是 1）
    assert_eq!(counter.load(Ordering::SeqCst), 10);
}

#[tokio::test]
async fn test_postpone_keeps_completion_receiver_valid() {
    let timer = TimerWheel::with_defaults();
    let counter = Arc::new(AtomicU32::new(0));
    let counter_clone = Arc::clone(&counter);

    let task = TimerTask::new_oneshot(
        Duration::from_millis(50),
        Some(CallbackWrapper::new(move || {
            let counter = Arc::clone(&counter_clone);
            async move {
                counter.fetch_add(1, Ordering::SeqCst);
            }
        })),
    );
    let allocate_handle = timer.allocate_handle();
    let task_id = allocate_handle.task_id();
    let handle = timer.register(allocate_handle, task);

    // Postpone task
    // 推迟任务
    timer.postpone(task_id, Duration::from_millis(100), None);

    // Verify original completion_receiver is still valid (after postponed, need to wait 100ms, plus margin)
    // 验证原始完成接收器是否仍然有效（推迟后，需要等待 100ms，加上余量）
    let (rx, _handle) = handle.into_parts();
    let result = match rx {
        CompletionReceiver::OneShot(receiver) => {
            tokio::time::timeout(Duration::from_millis(200), receiver.wait()).await
        }
        _ => panic!("Expected OneShot completion receiver"),
    };
    assert!(
        result.is_ok(),
        "Completion receiver should still work after postpone"
    );

    // Wait for callback to execute
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert_eq!(counter.load(Ordering::SeqCst), 1);
}
