use crate::task::{CallbackWrapper, TimerTask};
use crate::timer::TimerWheel;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

#[tokio::test]
async fn test_batch_postpone_length_mismatch_preserves_handle() {
    let timer = TimerWheel::with_defaults();
    let handles = timer.allocate_handles(2);
    let tasks = vec![
        TimerTask::new_oneshot(Duration::from_secs(10), None),
        TimerTask::new_oneshot(Duration::from_secs(10), None),
    ];
    let (_receivers, mut batch) = timer.register_batch(handles, tasks).unwrap().into_parts();

    let error = batch.postpone_each(vec![Duration::from_secs(1)]);
    assert!(matches!(
        error,
        Err(crate::error::TimerError::BatchLengthMismatch {
            handles_len: 2,
            tasks_len: 1,
        })
    ));
    assert_eq!(batch.len(), 2);

    let postponed = batch
        .postpone_each(vec![Duration::from_secs(1), Duration::from_secs(1)])
        .unwrap();
    assert_eq!(postponed, 2);

    let error = batch.postpone_each_with_callbacks(vec![]);
    assert!(matches!(
        error,
        Err(crate::error::TimerError::BatchLengthMismatch {
            handles_len: 2,
            tasks_len: 0,
        })
    ));
    assert_eq!(batch.len(), 2);
}

#[tokio::test]
async fn test_postpone_batch() {
    let timer = TimerWheel::with_defaults();
    let counter = Arc::new(AtomicU32::new(0));

    // Create 3 tasks
    // 创建 3 个任务
    let mut task_ids = Vec::new();
    for _ in 0..3 {
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
        let _handle = timer.register(allocate_handle, task).unwrap();
        task_ids.push((task_id, Duration::from_millis(150)));
    }

    // Batch postpone
    // 批量推迟
    let postponed = timer.postpone_batch(task_ids).unwrap();
    assert_eq!(postponed, 3);

    // Wait for original time 50ms, task should not trigger
    // 等待原始时间 50ms，任务不应触发
    tokio::time::sleep(Duration::from_millis(70)).await;
    assert_eq!(counter.load(Ordering::SeqCst), 0);

    // Wait for new trigger time (from postponed start, need to wait about 150ms)
    // 等待新的触发时间（从推迟开始算起，大约需要等待 150ms）
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Wait for callback to execute
    // 等待回调执行
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert_eq!(counter.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn test_postpone_batch_with_callbacks() {
    let timer = TimerWheel::with_defaults();
    let counter = Arc::new(AtomicU32::new(0));

    // Create 3 tasks
    // 创建 3 个任务
    let mut task_ids = Vec::new();
    for _ in 0..3 {
        let task = TimerTask::new_oneshot(Duration::from_millis(50), None);
        let allocate_handle = timer.allocate_handle();
        let task_id = allocate_handle.task_id();
        let _handle = timer.register(allocate_handle, task).unwrap();
        task_ids.push(task_id);
    }

    // Batch postpone and replace callbacks
    // 批量推迟并替换回调
    let updates: Vec<_> = task_ids
        .into_iter()
        .map(|id| {
            let counter_clone = Arc::clone(&counter);
            (
                id,
                Duration::from_millis(150),
                Some(CallbackWrapper::new(move || {
                    let counter = Arc::clone(&counter_clone);
                    async move {
                        counter.fetch_add(1, Ordering::SeqCst);
                    }
                })),
            )
        })
        .collect();

    // Batch postpone and replace callbacks
    // 批量推迟并替换回调
    let postponed = timer.postpone_batch_with_callbacks(updates).unwrap();
    assert_eq!(postponed, 3);

    // Wait for original time 50ms, task should not trigger
    // 等待原始时间 50ms，任务不应触发
    tokio::time::sleep(Duration::from_millis(70)).await;
    assert_eq!(counter.load(Ordering::SeqCst), 0);

    // Wait for new trigger time (from postponed start, need to wait about 150ms)
    // 等待新的触发时间（从推迟开始算起，大约需要等待 150ms）
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Wait for callback to execute
    // 等待回调执行
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert_eq!(counter.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn test_periodic_batch_cancel() {
    let timer = TimerWheel::with_defaults();
    let counter = Arc::new(AtomicU32::new(0));

    // Create multiple periodic tasks
    // 创建多个周期任务
    let mut task_ids = Vec::new();
    for _ in 0..3 {
        let counter_clone = Arc::clone(&counter);
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
        )
        .unwrap();
        let allocate_handle = timer.allocate_handle();
        let task_id = allocate_handle.task_id();
        task_ids.push(task_id);
        let _handle = timer.register(allocate_handle, task).unwrap();
    }

    // Wait for first execution
    // 等待第一次执行
    tokio::time::sleep(Duration::from_millis(80)).await;
    let count_before_cancel = counter.load(Ordering::SeqCst);
    assert!(
        count_before_cancel >= 3,
        "Expected at least 3 executions before cancel"
    );

    // Batch cancel
    // 批量取消
    let cancelled = timer.cancel_batch(&task_ids).unwrap();
    assert_eq!(cancelled, 3);

    // Wait and verify tasks stopped
    // 等待并验证任务已停止
    tokio::time::sleep(Duration::from_millis(150)).await;
    let count_after_cancel = counter.load(Ordering::SeqCst);

    // Count should not increase significantly after cancellation
    // 取消后计数不应该显著增加
    assert!(
        count_after_cancel - count_before_cancel <= 3,
        "Tasks should stop after cancel, before: {}, after: {}",
        count_before_cancel,
        count_after_cancel
    );
}

#[tokio::test]
async fn test_periodic_batch_postpone() {
    let timer = TimerWheel::with_defaults();
    let counter = Arc::new(AtomicU32::new(0));

    // Create 3 periodic tasks
    // 创建 3 个周期任务
    let mut postpone_updates = Vec::new();
    for _ in 0..3 {
        let counter_clone = Arc::clone(&counter);
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
        )
        .unwrap();
        let allocate_handle = timer.allocate_handle();
        let task_id = allocate_handle.task_id();
        let _handle = timer.register(allocate_handle, task).unwrap();
        postpone_updates.push((task_id, Duration::from_millis(150)));
    }

    // Batch postpone
    // 批量推迟
    let postponed = timer.postpone_batch(postpone_updates).unwrap();
    assert_eq!(postponed, 3);

    // Wait original time, should not trigger
    // 等待原始时间，不应触发
    tokio::time::sleep(Duration::from_millis(80)).await;
    assert_eq!(
        counter.load(Ordering::SeqCst),
        0,
        "Tasks should not trigger at original time"
    );

    // Wait for postponed time
    // 等待推迟后的时间
    tokio::time::sleep(Duration::from_millis(150)).await;

    let count = counter.load(Ordering::SeqCst);
    assert!(
        count >= 3,
        "All tasks should trigger after postpone, got count: {}",
        count
    );
}

#[tokio::test]
async fn test_periodic_batch_register() {
    let timer = TimerWheel::with_defaults();
    let counter = Arc::new(AtomicU32::new(0));

    // Step 1: Allocate handles
    // 分配 handles
    let handles = timer.allocate_handles(3);

    // Step 2: Create batch of periodic tasks
    // 创建批量周期任务
    let tasks: Vec<_> = (0..3)
        .map(|_| {
            let counter_clone = Arc::clone(&counter);
            TimerTask::new_periodic(
                Duration::from_millis(50),
                Duration::from_millis(50),
                Some(CallbackWrapper::new(move || {
                    let counter = Arc::clone(&counter_clone);
                    async move {
                        counter.fetch_add(1, Ordering::SeqCst);
                    }
                })),
                None,
            )
            .unwrap()
        })
        .collect();

    // Step 3: Batch register
    // 批量注册
    let batch_handle = timer
        .register_batch(handles, tasks)
        .expect("register_batch should succeed");
    assert_eq!(batch_handle.len(), 3);

    // Wait for executions
    // 等待执行
    tokio::time::sleep(Duration::from_millis(250)).await;

    let count = counter.load(Ordering::SeqCst);
    // Each task should execute at least 3 times, total at least 9
    // 每个任务应至少执行 3 次，总共至少 9 次
    assert!(
        count >= 9,
        "Expected at least 9 total executions, got {}",
        count
    );
}

#[tokio::test]
async fn test_timer_wheel_batch_64_delay_order() {
    tokio::time::pause();

    let timer = TimerWheel::with_defaults();
    let fired_order: Arc<parking_lot::Mutex<Vec<usize>>> =
        Arc::new(parking_lot::Mutex::new(Vec::new()));

    // Step 1: Batch allocate handles
    let handles = timer.allocate_handles(100);
    let _task_ids: Vec<_> = handles.iter().map(|h| h.task_id()).collect();

    // Step 2: Create tasks
    let fired_order_clone = Arc::clone(&fired_order);
    let tasks: Vec<_> = (0..100)
        .map(|i: usize| {
            let delay = Duration::from_secs(i as u64);
            let fired_order = Arc::clone(&fired_order_clone);
            let callback = Some(CallbackWrapper::new(move || {
                let fired_order = Arc::clone(&fired_order);
                async move {
                    fired_order.lock().push(i);
                }
            }));
            TimerTask::new_oneshot(delay, callback)
        })
        .collect();

    // Step 3: Batch register
    timer.register_batch(handles, tasks).unwrap();

    // Sleep 105s in virtual time to allow all 100 timers (0..100s) to fire
    tokio::time::sleep(Duration::from_secs(105)).await;

    // Yield multiple times to give spawned tokio callback tasks time to run
    for _ in 0..50 {
        tokio::task::yield_now().await;
    }

    let fired = fired_order.lock().clone();
    assert_eq!(fired.len(), 100);

    let expected: Vec<usize> = (0..100).collect();
    assert_eq!(
        fired, expected,
        "Timers 0..100 should fire in exact sequential order (including 64s)"
    );
}
