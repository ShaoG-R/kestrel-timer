use crate::{TimerWheel, TimerTask, TaskNotification};
use crate::config::ServiceConfig;
use crate::task::{CallbackWrapper, TaskId};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

#[tokio::test]
async fn test_postpone() {
    let timer = TimerWheel::with_defaults();
    let mut service = timer.create_service(ServiceConfig::default());
    let counter = Arc::new(AtomicU32::new(0));

    // Register a task, original callback increases 1
    // 注册一个任务，原始回调增加 1
    let counter_clone1 = Arc::clone(&counter);
    let handle = service.allocate_handle();
    let task_id = handle.task_id();
    let task = TimerTask::new_oneshot(
        Duration::from_millis(50),
        Some(CallbackWrapper::new(move || {
            let counter = Arc::clone(&counter_clone1);
            async move {
                counter.fetch_add(1, Ordering::SeqCst);
            }
            })),
    );
    service.register(handle, task).unwrap();

    // Postpone task and replace callback, new callback increases 10
    // 延期任务并替换回调，新回调增加 10
    let counter_clone2 = Arc::clone(&counter);
    let postponed = service.postpone(
        task_id,
        Duration::from_millis(100),
        Some(CallbackWrapper::new(move || {
            let counter = Arc::clone(&counter_clone2);
            async move {
                counter.fetch_add(10, Ordering::SeqCst);
            }
        }))
    );
    assert!(postponed, "Task should be postponed successfully");

    // Receive timeout notification (after postponing, need to wait 100ms, plus margin)
    // 接收超时通知（延期后，需要等待 100ms，加上余量）
    let rx = service.take_receiver().unwrap();
    let received_task_id = tokio::time::timeout(Duration::from_millis(200), rx.recv())
        .await
        .expect("Should receive timeout notification")
        .expect("Should receive Some value");

    assert_eq!(received_task_id, TaskNotification::OneShot(task_id));
    
    // Wait for callback to execute
    // 等待回调执行
    tokio::time::sleep(Duration::from_millis(20)).await;
    
    // Verify new callback is executed (increased 10 instead of 1)
    // 验证新回调已执行（增加 10 而不是 1）
    assert_eq!(counter.load(Ordering::SeqCst), 10);
}

#[tokio::test]
async fn test_postpone_nonexistent_task() {
    let timer = TimerWheel::with_defaults();
    let service = timer.create_service(ServiceConfig::default());

    // Try to postpone a nonexistent task
    // 尝试延期一个不存在的任务
    let fake_handle = service.allocate_handle();
    let fake_task_id = fake_handle.task_id();
    // Do not register this task
    // 不注册这个任务
    let postponed = service.postpone(fake_task_id, Duration::from_millis(100), None);
    assert!(!postponed, "Nonexistent task should not be postponed");
}

#[tokio::test]
async fn test_postpone_batch() {
    let timer = TimerWheel::with_defaults();
    let mut service = timer.create_service(ServiceConfig::default());
    let counter = Arc::new(AtomicU32::new(0));

    // Register 3 tasks
    // 注册 3 个任务
    let mut task_ids = Vec::new();
    for _ in 0..3 {
        let counter_clone = Arc::clone(&counter);
        let handle = service.allocate_handle();
        let task_id = handle.task_id();
        let task = TimerTask::new_oneshot(
            Duration::from_millis(50),
        Some(CallbackWrapper::new(move || {
            let counter = Arc::clone(&counter_clone);
            async move {
                counter.fetch_add(1, Ordering::SeqCst);
            }
        })),
    );
        task_ids.push((task_id, Duration::from_millis(150), None));
        service.register(handle, task).unwrap();
    }

    // Batch postpone
    // 批量延期
    let postponed = service.postpone_batch_with_callbacks(task_ids);
    assert_eq!(postponed, 3, "All 3 tasks should be postponed");

    // Wait for original time 50ms, task should not trigger
    // 等待原始时间 50ms，任务应该不触发
    tokio::time::sleep(Duration::from_millis(70)).await;
    assert_eq!(counter.load(Ordering::SeqCst), 0);

    // Receive all timeout notifications
    // 接收所有超时通知
    let mut received_count = 0;
    let rx = service.take_receiver().unwrap();
    
    while received_count < 3 {
        match tokio::time::timeout(Duration::from_millis(200), rx.recv()).await {
            Ok(Some(_task_id)) => {
                received_count += 1;
            }
            Ok(None) => break,
            Err(_) => break,
        }
    }

    assert_eq!(received_count, 3);
    
    // Wait for callback to execute
    // 等待回调执行
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert_eq!(counter.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn test_postpone_batch_with_callbacks() {
    let timer = TimerWheel::with_defaults();
    let mut service = timer.create_service(ServiceConfig::default());
    let counter = Arc::new(AtomicU32::new(0));

    // Register 3 tasks
    // 注册 3 个任务
    let mut task_ids = Vec::new();
    for _ in 0..3 {
        let handle = service.allocate_handle();
        let task_id = handle.task_id();
        let task = TimerTask::new_oneshot(
            Duration::from_millis(50),
            None,
        );
        task_ids.push(task_id);
        service.register(handle, task).unwrap();
    }

    // Batch postpone and replace callback
    // 批量延期并替换回调
    let updates: Vec<_> = task_ids
        .into_iter()
        .map(|id| {
            let counter_clone = Arc::clone(&counter);
            (id, Duration::from_millis(150), Some(CallbackWrapper::new(move || {
                let counter = Arc::clone(&counter_clone);
                async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                }
                })))
        })
        .collect();

    let postponed = service.postpone_batch_with_callbacks(updates);
    assert_eq!(postponed, 3, "All 3 tasks should be postponed");

    // Wait for original time 50ms, task should not trigger
    // 等待原始时间 50ms，任务应该不触发
    tokio::time::sleep(Duration::from_millis(70)).await;
    assert_eq!(counter.load(Ordering::SeqCst), 0);

    // Receive all timeout notifications
    // 接收所有超时通知
    let mut received_count = 0;
    let rx = service.take_receiver().unwrap();
    
    while received_count < 3 {
        match tokio::time::timeout(Duration::from_millis(200), rx.recv()).await {
            Ok(Some(_task_id)) => {
                received_count += 1;
            }
            Ok(None) => break,
            Err(_) => break,
        }
    }

    assert_eq!(received_count, 3);
    
    // Wait for callback to execute
    // 等待回调执行
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert_eq!(counter.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn test_postpone_batch_empty() {
    let timer = TimerWheel::with_defaults();
    let service = timer.create_service(ServiceConfig::default());

    // Postpone empty list
    let empty: Vec<(TaskId, Duration, Option<CallbackWrapper>)> = vec![];
    let postponed = service.postpone_batch_with_callbacks(empty);
    assert_eq!(postponed, 0, "No tasks should be postponed");
}

#[tokio::test]
async fn test_postpone_keeps_timeout_notification_valid() {
    let timer = TimerWheel::with_defaults();
    let mut service = timer.create_service(ServiceConfig::default());
    let counter = Arc::new(AtomicU32::new(0));

    // Register a task
    // 注册一个任务
    let counter_clone = Arc::clone(&counter);
    let handle = service.allocate_handle();
    let task_id = handle.task_id();
    let task = TimerTask::new_oneshot(
        Duration::from_millis(50),
        Some(CallbackWrapper::new(move || {
            let counter = Arc::clone(&counter_clone);
            async move {
                counter.fetch_add(1, Ordering::SeqCst);
            }
        })),
    );
    service.register(handle, task).unwrap();

    // Postpone task
    // 延期任务
    service.postpone(task_id, Duration::from_millis(100), None);

    // Verify timeout notification is still valid (after postponing, need to wait 100ms, plus margin)
    // 验证超时通知是否仍然有效（延期后，需要等待 100ms，加上余量）
    let rx = service.take_receiver().unwrap();
    let received_task_id = tokio::time::timeout(Duration::from_millis(200), rx.recv())
        .await
        .expect("Should receive timeout notification")
        .expect("Should receive Some value");

    assert_eq!(received_task_id, TaskNotification::OneShot(task_id), "Timeout notification should still work after postpone");
    
    // Wait for callback to execute
    // 等待回调执行
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert_eq!(counter.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn test_postpone_batch_without_callbacks() {
    let timer = TimerWheel::with_defaults();
    let mut service = timer.create_service(ServiceConfig::default());
    let counter = Arc::new(AtomicU32::new(0));

    // Register 3 tasks, with original callback
    // 注册 3 个任务，带有原始回调
    let mut task_ids = Vec::new();
    for _ in 0..3 {
        let counter_clone = Arc::clone(&counter);
        let handle = service.allocate_handle();
        let task_id = handle.task_id();
        let task = TimerTask::new_oneshot(
            Duration::from_millis(50),
            Some(CallbackWrapper::new(move || {
                let counter = Arc::clone(&counter_clone);
                async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                }
            })),
        );
        task_ids.push(task_id);
        service.register(handle, task).unwrap();
    }

    // Batch postpone, without replacing callback
    // 批量延期，不替换回调
    let updates: Vec<_> = task_ids
        .iter()
        .map(|&id| (id, Duration::from_millis(150)))
        .collect();
    let postponed = service.postpone_batch(updates);
    assert_eq!(postponed, 3, "All 3 tasks should be postponed");

    // Wait for original time 50ms, task should not trigger
    // 等待原始时间 50ms，任务应该不触发
    tokio::time::sleep(Duration::from_millis(70)).await;
    assert_eq!(counter.load(Ordering::SeqCst), 0, "Callbacks should not fire yet");

    // Receive all timeout notifications
    // 接收所有超时通知
    let mut received_count = 0;
    let rx = service.take_receiver().unwrap();
    
    while received_count < 3 {
        match tokio::time::timeout(Duration::from_millis(200), rx.recv()).await {
            Ok(Some(_task_id)) => {
                received_count += 1;
            }
            Ok(None) => break,
            Err(_) => break,
        }
    }

    assert_eq!(received_count, 3, "Should receive 3 timeout notifications");
    
    // Wait for callback to execute
    // 等待回调执行
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert_eq!(counter.load(Ordering::SeqCst), 3, "All callbacks should execute");
}

