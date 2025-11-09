use crate::{TimerWheel, TimerTask, TaskNotification};
use crate::config::ServiceConfig;
use crate::task::{CallbackWrapper, TaskId};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

#[tokio::test]
async fn test_cancel_task() {
    let timer = TimerWheel::with_defaults();
    let service = timer.create_service(ServiceConfig::default());

    // Add a long-term timer (添加一个长期定时器)
    let handle = service.allocate_handle();
    let task_id = handle.task_id();
    let task = TimerTask::new_oneshot(Duration::from_secs(10), None);
    
    service.register(handle, task).unwrap();

    // Cancel task (取消任务)
    let cancelled = service.cancel_task(task_id);
    assert!(cancelled, "Task should be cancelled successfully");

    // Try to cancel the same task again, should return false (再次尝试取消同一任务，应返回 false)
    let cancelled_again = service.cancel_task(task_id);
    assert!(!cancelled_again, "Task should not exist anymore");
}

#[tokio::test]
async fn test_cancel_nonexistent_task() {
    let timer = TimerWheel::with_defaults();
    let service = timer.create_service(ServiceConfig::default());

    // Add a timer to initialize service (添加定时器以初始化服务)
    let handle = service.allocate_handle();
    let task = TimerTask::new_oneshot(Duration::from_millis(50), None);
    service.register(handle, task).unwrap();

    // Try to cancel a nonexistent task (create a task ID that will not actually be registered)
    // 尝试取消不存在的任务（创建一个实际不会注册的任务 ID）
    let fake_handle = service.allocate_handle();
    let fake_task_id = fake_handle.task_id();
    // Do not register fake_task (不注册 fake_task)
    let cancelled = service.cancel_task(fake_task_id);
    assert!(!cancelled, "Nonexistent task should not be cancelled");
}

#[tokio::test]
async fn test_cancel_task_spawns_background_task() {
    let timer = TimerWheel::with_defaults();
    let service = timer.create_service(ServiceConfig::default());
    let counter = Arc::new(AtomicU32::new(0));

    // Create a timer (创建定时器)
    let counter_clone = Arc::clone(&counter);
    let handle = service.allocate_handle();
    let task_id = handle.task_id();
    let task = TimerTask::new_oneshot(
        Duration::from_secs(10),
        Some(CallbackWrapper::new(move || {
            let counter = Arc::clone(&counter_clone);
            async move {
                counter.fetch_add(1, Ordering::SeqCst);
            }
        })),
    );
    
    service.register(handle, task).unwrap();

    // Use cancel_task (will wait for result, but processed in background coroutine)
    // 使用 cancel_task（将等待结果，但在后台协程中处理）
    let cancelled = service.cancel_task(task_id);
    assert!(cancelled, "Task should be cancelled successfully");

    // Wait long enough to ensure callback is not executed (等待足够长时间以确保回调未执行)
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(counter.load(Ordering::SeqCst), 0, "Callback should not have been executed");

    // Verify task has been removed from active_tasks (验证任务已从 active_tasks 中移除)
    let cancelled_again = service.cancel_task(task_id);
    assert!(!cancelled_again, "Task should have been removed from active_tasks");
}

#[tokio::test]
async fn test_schedule_and_cancel_direct() {
    let timer = TimerWheel::with_defaults();
    let service = timer.create_service(ServiceConfig::default());
    let counter = Arc::new(AtomicU32::new(0));

    // Schedule timer directly
    // 直接调度定时器
    let counter_clone = Arc::clone(&counter);
    let handle = service.allocate_handle();
    let task_id = handle.task_id();
    let task = TimerTask::new_oneshot(
        Duration::from_secs(10),
        Some(CallbackWrapper::new(move || {
            let counter = Arc::clone(&counter_clone);
            async move {
                counter.fetch_add(1, Ordering::SeqCst);
            }
        })),
    );
    service.register(handle, task).unwrap();

    // Immediately cancel
    // 立即取消
    let cancelled = service.cancel_task(task_id);
    assert!(cancelled, "Task should be cancelled successfully");

    // Wait to ensure callback is not executed
    // 等待确保回调未执行
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(counter.load(Ordering::SeqCst), 0, "Callback should not have been executed");
}

#[tokio::test]
async fn test_cancel_batch_direct() {
    let timer = TimerWheel::with_defaults();
    let service = timer.create_service(ServiceConfig::default());
    let counter = Arc::new(AtomicU32::new(0));

    // Batch schedule timers
    // 批量调度定时器
    let handles = service.allocate_handles(10);
    let task_ids: Vec<_> = handles.iter().map(|h| h.task_id()).collect();
    let tasks: Vec<_> = (0..10)
        .map(|_| {
            let counter = Arc::clone(&counter);
            TimerTask::new_oneshot(Duration::from_secs(10), Some(CallbackWrapper::new(move || {
                let counter = Arc::clone(&counter);
                async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                }
            })))
        })
        .collect();

    assert_eq!(task_ids.len(), 10);
    service.register_batch(handles, tasks).unwrap();

    // Batch cancel all tasks
    // 批量取消所有任务
    let cancelled = service.cancel_batch(&task_ids);
    assert_eq!(cancelled, 10, "All 10 tasks should be cancelled");

    // Wait to ensure callback is not executed
    // 等待确保回调未执行
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(counter.load(Ordering::SeqCst), 0, "No callbacks should have been executed");
}

#[tokio::test]
async fn test_cancel_batch_partial() {
    let timer = TimerWheel::with_defaults();
    let service = timer.create_service(ServiceConfig::default());
    let counter = Arc::new(AtomicU32::new(0));

    // Batch schedule timers
    // 批量调度定时器
    let handles = service.allocate_handles(10);
    let task_ids: Vec<_> = handles.iter().map(|h| h.task_id()).collect();
    let tasks: Vec<_> = (0..10)
        .map(|_| {
            let counter = Arc::clone(&counter);
            TimerTask::new_oneshot(Duration::from_secs(10), Some(CallbackWrapper::new(move || {
                let counter = Arc::clone(&counter);
                async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                }
            })))
        })
        .collect();

    service.register_batch(handles, tasks).unwrap();

    // Only cancel first 5 tasks
    // 只取消前 5 个任务
    let to_cancel: Vec<_> = task_ids.iter().take(5).copied().collect();
    let cancelled = service.cancel_batch(&to_cancel);
    assert_eq!(cancelled, 5, "5 tasks should be cancelled");

    // Wait to ensure first 5 callbacks are not executed
    // 等待确保前 5 个回调未执行
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(counter.load(Ordering::SeqCst), 0, "Cancelled tasks should not execute");
}

#[tokio::test]
async fn test_cancel_batch_empty() {
    let timer = TimerWheel::with_defaults();
    let service = timer.create_service(ServiceConfig::default());

    // Cancel empty list
    // 取消空列表
    let empty: Vec<TaskId> = vec![];
    let cancelled = service.cancel_batch(&empty);
    assert_eq!(cancelled, 0, "No tasks should be cancelled");
}

#[tokio::test]
async fn test_cancelled_task_not_forwarded_to_timeout_rx() {
    let timer = TimerWheel::with_defaults();
    let mut service = timer.create_service(ServiceConfig::default());

    // Register two tasks: one will be cancelled, one will expire normally
    // 注册两个任务：一个将被取消，一个将正常过期
    let handle1 = service.allocate_handle();
    let task1_id = handle1.task_id();
    let task1 = TimerTask::new_oneshot(Duration::from_secs(10), None);
    service.register(handle1, task1).unwrap();

    let handle2 = service.allocate_handle();
    let task2_id = handle2.task_id();
    let task2 = TimerTask::new_oneshot(Duration::from_millis(50), None);
    service.register(handle2, task2).unwrap();

    // Cancel first task
    // 取消第一个任务
    let cancelled = service.cancel_task(task1_id);
    assert!(cancelled, "Task should be cancelled");

    // Wait for second task to expire
    // 等待第二个任务过期
    let rx = service.take_receiver().unwrap();
    let received_notification = tokio::time::timeout(Duration::from_millis(200), rx.recv())
        .await
        .expect("Should receive timeout notification")
        .expect("Should receive Some value");

    // Should only receive notification for second task (expired), not for first task (cancelled)
    assert_eq!(received_notification, TaskNotification::OneShot(task2_id), "Should only receive expired task notification");

    // Verify no other notifications (especially cancelled tasks should not have notifications)
    let no_more = tokio::time::timeout(Duration::from_millis(50), rx.recv()).await;
    assert!(no_more.is_err(), "Should not receive any more notifications");
}


