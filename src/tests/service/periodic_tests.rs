use crate::{TimerWheel, TimerTask, TaskNotification};
use crate::config::ServiceConfig;
use crate::task::CallbackWrapper;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

#[tokio::test]
async fn test_periodic_task_basic() {
    let timer = TimerWheel::with_defaults();
    let mut service = timer.create_service(ServiceConfig::default());
    let counter = Arc::new(AtomicU32::new(0));

    // Register a periodic task with 50ms interval (注册一个 50ms 间隔的周期性任务)
    let counter_clone = Arc::clone(&counter);
    let handle = service.allocate_handle();
    let task_id = handle.task_id();
    let task = TimerTask::new_periodic(
        Duration::from_millis(30),  // initial delay (初始延迟)
        Duration::from_millis(50),  // interval (间隔)
        Some(CallbackWrapper::new(move || {
            let counter = Arc::clone(&counter_clone);
            async move {
                counter.fetch_add(1, Ordering::SeqCst);
            }
        })),
        None,
    );
    service.register(handle, task).unwrap();

    // Receive periodic notifications (接收周期性通知)
    let rx = service.take_receiver().unwrap();
    let mut notification_count = 0;

    // Receive first 3 notifications (接收前 3 个通知)
    while notification_count < 3 {
        match tokio::time::timeout(Duration::from_millis(200), rx.recv()).await {
            Ok(Some(notification)) => {
                match notification {
                    TaskNotification::Periodic(id) => {
                        assert_eq!(id, task_id, "Should receive notification for correct task");
                        notification_count += 1;
                    }
                    _ => panic!("Expected periodic notification"),
                }
            }
            Ok(None) => break,
            Err(_) => panic!("Timeout waiting for periodic notification"),
        }
    }

    assert_eq!(notification_count, 3, "Should receive 3 periodic notifications");
    
    // Wait for callback to execute
    // 等待回调执行
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert_eq!(counter.load(Ordering::SeqCst), 3, "Callback should execute 3 times");

    // Cancel the periodic task (取消周期性任务)
    let cancelled = service.cancel_task(task_id);
    assert!(cancelled, "Should be able to cancel periodic task");
}

#[tokio::test]
async fn test_periodic_task_cancel_no_notification() {
    let timer = TimerWheel::with_defaults();
    let mut service = timer.create_service(ServiceConfig::default());

    // Register a periodic task (注册周期性任务)
    let handle = service.allocate_handle();
    let task_id = handle.task_id();
    let task = TimerTask::new_periodic(
        Duration::from_millis(30),
        Duration::from_millis(50),
        None,
        None,
    );
    service.register(handle, task).unwrap();

    // Wait for first notification (等待第一个通知)
    let rx = service.take_receiver().unwrap();
    let notification = tokio::time::timeout(Duration::from_millis(100), rx.recv())
        .await
        .expect("Should receive first notification")
        .expect("Should receive Some value");
    
    assert_eq!(notification, TaskNotification::Periodic(task_id));

    // Cancel the task (取消任务)
    let cancelled = service.cancel_task(task_id);
    assert!(cancelled, "Should be able to cancel task");

    // Should not receive cancelled notification (不应该接收到取消通知)
    match tokio::time::timeout(Duration::from_millis(100), rx.recv()).await {
        Ok(Some(_)) => panic!("Should not receive cancelled notification"),
        Ok(None) | Err(_) => {} // Expected: timeout or channel closed
    }
}

#[tokio::test]
async fn test_mixed_oneshot_and_periodic_tasks() {
    let timer = TimerWheel::with_defaults();
    let mut service = timer.create_service(ServiceConfig::default());

    // Register one-shot tasks (注册一次性任务)
    let handle1 = service.allocate_handle();
    let oneshot_id = handle1.task_id();
    let oneshot_task = TimerTask::new_oneshot(Duration::from_millis(50), None);
    service.register(handle1, oneshot_task).unwrap();

    // Register periodic task (注册周期性任务)
    let handle2 = service.allocate_handle();
    let periodic_id = handle2.task_id();
    let periodic_task = TimerTask::new_periodic(
        Duration::from_millis(30),
        Duration::from_millis(40),
        None,
        None,
    );
    service.register(handle2, periodic_task).unwrap();

    // Receive notifications (接收通知)
    let rx = service.take_receiver().unwrap();
    let mut oneshot_received = false;
    let mut periodic_count = 0;

    // Receive notifications for a while (接收一段时间的通知)
    let start = tokio::time::Instant::now();
    while start.elapsed() < Duration::from_millis(200) {
        match tokio::time::timeout(Duration::from_millis(100), rx.recv()).await {
            Ok(Some(notification)) => {
                match notification {
                    TaskNotification::OneShot(id) => {
                        assert_eq!(id, oneshot_id, "Should be one-shot task");
                        oneshot_received = true;
                    }
                    TaskNotification::Periodic(id) => {
                        assert_eq!(id, periodic_id, "Should be periodic task");
                        periodic_count += 1;
                    }
                }
            }
            Ok(None) => break,
            Err(_) => break,
        }
    }

    assert!(oneshot_received, "Should receive one-shot notification");
    assert!(periodic_count >= 2, "Should receive at least 2 periodic notifications");

    // Cancel periodic task (取消周期性任务)
    service.cancel_task(periodic_id);
}

#[tokio::test]
async fn test_periodic_task_batch_register() {
    let timer = TimerWheel::with_defaults();
    let mut service = timer.create_service(ServiceConfig::default());
    let counter = Arc::new(AtomicU32::new(0));

    // Register multiple periodic tasks in batch (批量注册多个周期性任务)
    let handles = service.allocate_handles(3);
    let task_ids: Vec<_> = handles.iter().map(|h| h.task_id()).collect();
    let tasks: Vec<_> = (0..3)
        .map(|_| {
            let counter = Arc::clone(&counter);
            TimerTask::new_periodic(
                Duration::from_millis(30),
                Duration::from_millis(50),
                Some(CallbackWrapper::new(move || {
                    let counter = Arc::clone(&counter);
                    async move {
                        counter.fetch_add(1, Ordering::SeqCst);
                    }
                })),
                None,
            )
        })
        .collect();
    
    service.register_batch(handles, tasks).unwrap();

    // Receive notifications (接收通知)
    let rx = service.take_receiver().unwrap();
    let mut notification_counts = std::collections::HashMap::new();

    // Receive notifications for a while (接收一段时间的通知)
    let start = tokio::time::Instant::now();
    while start.elapsed() < Duration::from_millis(180) {
        match tokio::time::timeout(Duration::from_millis(100), rx.recv()).await {
            Ok(Some(TaskNotification::Periodic(id))) => {
                *notification_counts.entry(id).or_insert(0) += 1;
            }
            Ok(Some(_)) => panic!("Expected periodic notification"),
            Ok(None) => break,
            Err(_) => break,
        }
    }

    // Each task should receive at least 2 notifications (每个任务应该至少收到 2 个通知)
    for task_id in &task_ids {
        let count = notification_counts.get(task_id).copied().unwrap_or(0);
        assert!(count >= 2, "Task {:?} should receive at least 2 notifications, got {}", task_id, count);
    }

    // Wait for callbacks to execute
    // 等待回调执行
    tokio::time::sleep(Duration::from_millis(20)).await;
    let total_callbacks = counter.load(Ordering::SeqCst);
    assert!(total_callbacks >= 6, "Should have at least 6 callback executions (3 tasks * 2), got {}", total_callbacks);

    // Cancel all periodic tasks (取消所有周期性任务)
    let cancelled = service.cancel_batch(&task_ids);
    assert_eq!(cancelled, 3, "Should cancel all 3 tasks");
}

