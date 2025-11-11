use futures::future;
use kestrel_timer::config::ServiceConfig;
use kestrel_timer::{CallbackWrapper, CompletionReceiver, TimerTask, TimerWheel};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

#[tokio::test]
async fn test_large_scale_timers() {
    // Test large-scale concurrent timers (10000+)
    // 测试大规模并发定时器（10000+）
    let timer = Arc::new(TimerWheel::with_defaults());
    let counter = Arc::new(AtomicU32::new(0));
    const TIMER_COUNT: u32 = 10_000;

    let start = Instant::now();

    // Concurrent creation of 10000 timers
    // 并发创建 10000 个定时器
    let mut futures = Vec::new();
    for i in 0..TIMER_COUNT {
        let timer_clone = Arc::clone(&timer);
        let counter_clone = Arc::clone(&counter);
        let delay = Duration::from_millis(10 + (i % 100) as u64);

        let future = async move {
            let task = TimerTask::new_oneshot(
                delay,
                Some(CallbackWrapper::new(move || {
                    let counter = Arc::clone(&counter_clone);
                    async move {
                        counter.fetch_add(1, Ordering::SeqCst);
                    }
                })),
            );
            let handle = timer_clone.allocate_handle();
            timer_clone.register(handle, task)
        };
        futures.push(future);
    }

    // Concurrent waiting for all timers to be created
    future::join_all(futures).await;

    println!(
        "Creation of {} timers took: {:?}",
        TIMER_COUNT,
        start.elapsed()
    );

    // Waiting for all timers to trigger
    tokio::time::sleep(Duration::from_millis(200)).await;

    let count = counter.load(Ordering::SeqCst);
    println!("Number of triggered timers: {}", count);
    assert_eq!(count, TIMER_COUNT, "All timers should have been triggered");
}

#[tokio::test]
async fn test_timer_precision() {
    // Test timer precision
    // 测试定时器精度
    let timer = TimerWheel::with_defaults();
    let start_time = Arc::new(parking_lot::Mutex::new(None::<Instant>));
    let end_time = Arc::new(parking_lot::Mutex::new(None::<Instant>));

    *start_time.lock() = Some(Instant::now());

    let end_clone = Arc::clone(&end_time);
    let task = TimerTask::new_oneshot(
        Duration::from_millis(100),
        Some(CallbackWrapper::new(move || {
            let end_time = Arc::clone(&end_clone);
            async move {
                *end_time.lock() = Some(Instant::now());
            }
        })),
    );
    let allocated_handle = timer.allocate_handle();
    let handle = timer.register(allocated_handle, task);

    // Use completion_receiver to wait for timer completion, instead of fixed sleep time
    // This can avoid race conditions
    // 使用完成接收器等待定时器完成，而不是固定睡眠时间
    // 这样可以避免竞争条件
    let (rx, _handle) = handle.into_parts();
    match rx {
        CompletionReceiver::OneShot(receiver) => {
            receiver.recv().await.unwrap();
        }
        _ => {}
    }

    // Additional wait to ensure callback execution is complete
    tokio::time::sleep(Duration::from_millis(20)).await;

    let start = start_time.lock().expect("start_time should be set");
    let end = end_time
        .lock()
        .expect("end_time should be set after timer completion");
    let elapsed = end.duration_since(start);

    println!("Expected delay: 100ms, actual delay: {:?}", elapsed);

    // Allow ±50ms error (considering scheduling delay and system load)
    // 允许 ±50ms 误差（考虑调度延迟和系统负载）
    assert!(
        elapsed >= Duration::from_millis(80) && elapsed <= Duration::from_millis(180),
        "Timer precision is within acceptable range, actual delay: {:?}",
        elapsed
    );
}

#[tokio::test]
async fn test_concurrent_operations() {
    // Test concurrent operations (adding and canceling timers simultaneously)
    // 测试并发操作（同时添加和取消定时器）
    let timer = Arc::new(TimerWheel::with_defaults());
    let counter = Arc::new(AtomicU32::new(0));

    // Concurrent creation of all timers (5 tasks × 1000 timers = 5000)
    // 并发创建所有定时器（5 个任务 × 1000 个定时器 = 5000 个）
    let mut all_futures = Vec::new();

    for _ in 0..5 {
        for _ in 0..1000 {
            let timer_clone = Arc::clone(&timer);
            let counter_clone = Arc::clone(&counter);

            let future = async move {
                let task = TimerTask::new_oneshot(
                    Duration::from_millis(50),
                    Some(CallbackWrapper::new(move || {
                        let counter = Arc::clone(&counter_clone);
                        async move {
                            counter.fetch_add(1, Ordering::SeqCst);
                        }
                    })),
                );
                let handle = timer_clone.allocate_handle();
                timer_clone.register(handle, task)
            };

            all_futures.push(future);
        }
    }

    // Concurrent waiting for all timers to be created
    // 并发等待所有定时器创建完成
    future::join_all(all_futures).await;

    // Waiting for timers to trigger
    // 等待定时器触发
    tokio::time::sleep(Duration::from_millis(150)).await;

    let count = counter.load(Ordering::SeqCst);
    println!("Number of triggered timers: {}", count);
    assert_eq!(count, 5000, "All timers should have been triggered");
}

#[tokio::test]
async fn test_timer_with_different_delays() {
    // Test timers with different delays
    // 测试不同延迟的定时器
    let timer = TimerWheel::with_defaults();
    let results = Arc::new(parking_lot::Mutex::new(Vec::new()));

    let delays = vec![10, 20, 30, 50, 100, 150, 200];
    let mut handles = Vec::new();

    for (idx, &delay_ms) in delays.iter().enumerate() {
        let results_clone = Arc::clone(&results);

        let task = TimerTask::new_oneshot(
            Duration::from_millis(delay_ms),
            Some(CallbackWrapper::new(move || {
                let results = Arc::clone(&results_clone);
                async move {
                    results.lock().push((idx, delay_ms));
                }
            })),
        );
        let allocated_handle = timer.allocate_handle();
        let handle = timer.register(allocated_handle, task);

        handles.push(handle);
    }

    // Use completion_receiver to wait for all timers to complete, instead of fixed sleep time
    // This can ensure all timers have actually triggered
    // 使用完成接收器等待所有定时器完成，而不是固定睡眠时间
    // 这样可以确保所有定时器都实际触发
    for handle in handles {
        let (rx, _handle) = handle.into_parts();
        match rx {
            CompletionReceiver::OneShot(receiver) => {
                receiver.recv().await.unwrap();
            }
            _ => {}
        }
    }

    // Additional wait to ensure all callbacks are executed completely
    // 额外等待确保所有回调完全执行
    tokio::time::sleep(Duration::from_millis(50)).await;

    let final_results = results.lock();
    println!("Trigger order: {:?}", final_results);
    assert_eq!(
        final_results.len(),
        delays.len(),
        "All timers should have been triggered"
    );
}

#[tokio::test]
async fn test_memory_efficiency() {
    // Test memory efficiency - create many timers and then cancel
    // 测试内存效率 - 创建许多定时器并然后取消
    let timer = Arc::new(TimerWheel::with_defaults());

    // Concurrent creation of 5000 timers
    // 并发创建 5000 个定时器
    let mut create_futures = Vec::new();
    for _ in 0..5000 {
        let timer_clone = Arc::clone(&timer);
        let future = async move {
            let task = TimerTask::new_oneshot(Duration::from_secs(10), None);
            let handle = timer_clone.allocate_handle();
            timer_clone.register(handle, task)
        };
        create_futures.push(future);
    }

    let handles = future::join_all(create_futures).await;

    // Cancel all timers (now synchronous operation)
    // 取消所有定时器（现在同步操作）
    let cancelled_count = handles
        .into_iter()
        .map(|handle| handle.cancel())
        .filter(|&success| success)
        .count();

    println!("Number of cancelled timers: {}", cancelled_count);
    assert_eq!(cancelled_count, 5000);
}

#[tokio::test]
async fn test_batch_schedule() {
    // Test batch scheduling timers
    // 测试批量调度定时器
    let timer = TimerWheel::with_defaults();
    let counter = Arc::new(AtomicU32::new(0));

    const BATCH_SIZE: usize = 100;
    let start = Instant::now();

    // Allocate handles first (先分配 handles)
    let handles = timer.allocate_handles(BATCH_SIZE);

    // Create batch tasks (创建批量任务)
    let tasks: Vec<_> = (0..BATCH_SIZE)
        .map(|i| {
            let counter_clone = Arc::clone(&counter);
            let delay = Duration::from_millis(50 + (i % 10) as u64);
            let callback = Some(CallbackWrapper::new(move || {
                let counter = Arc::clone(&counter_clone);
                async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                }
            }));
            TimerTask::new_oneshot(delay, callback)
        })
        .collect();

    // Batch schedule
    // 批量调度
    let batch = timer.register_batch(handles, tasks).unwrap();

    println!(
        "Batch scheduling of {} timers took: {:?}",
        BATCH_SIZE,
        start.elapsed()
    );
    assert_eq!(batch.len(), BATCH_SIZE);

    // Waiting for all timers to trigger
    // 等待所有定时器触发
    tokio::time::sleep(Duration::from_millis(150)).await;

    let count = counter.load(Ordering::SeqCst);
    println!("Number of triggered timers: {}", count);
    assert_eq!(
        count, BATCH_SIZE as u32,
        "All 100 timers should have been triggered"
    );
}

#[tokio::test]
async fn test_batch_cancel() {
    // Test batch canceling timers
    // 测试批量取消定时器
    let timer = TimerWheel::with_defaults();
    const TIMER_COUNT: usize = 500;

    // Allocate handles first (先分配 handles)
    let handles = timer.allocate_handles(TIMER_COUNT);

    // Create batch timers (创建批量定时器)
    let tasks: Vec<_> = (0..TIMER_COUNT)
        .map(|_| TimerTask::new_oneshot(Duration::from_secs(10), None))
        .collect();

    let batch = timer.register_batch(handles, tasks).unwrap();
    assert_eq!(batch.len(), TIMER_COUNT);

    // Batch cancel (using BatchHandle's cancel_all method)
    // 批量取消（使用 BatchHandle 的 cancel_all 方法）
    let start = Instant::now();
    let cancelled = batch.cancel_all();
    let elapsed = start.elapsed();

    println!("Batch canceling {} timers took: {:?}", TIMER_COUNT, elapsed);
    assert_eq!(
        cancelled, TIMER_COUNT,
        "All 500 timers should have been successfully cancelled"
    );
}

#[tokio::test]
async fn test_batch_cancel_partial() {
    // Test partial batch canceling
    // 测试部分批量取消
    let timer = TimerWheel::with_defaults();

    // Allocate handles first (先分配 handles)
    let allocated_handles = timer.allocate_handles(10);

    // Create 10 timers (创建 10 个定时器)
    let tasks: Vec<_> = (0..10)
        .map(|_| TimerTask::new_oneshot(Duration::from_millis(100), None))
        .collect();

    let batch = timer.register_batch(allocated_handles, tasks).unwrap();

    // Convert to individual handles (转换为单个句柄)
    let mut handles = batch.into_handles();

    // Split: take first 5, keep last 5 (分割：取前 5 个，保留后 5 个)
    let remaining_handles = handles.split_off(5);

    // Cancel first 5 (取消前 5 个)
    let mut cancelled_count = 0;
    for handle in handles {
        if handle.cancel() {
            cancelled_count += 1;
        }
    }
    assert_eq!(cancelled_count, 5);

    // Wait for remaining timers to trigger - increase wait time to 200ms to ensure timers actually trigger
    // 等待剩余定时器触发 - 增加等待时间到 200ms 确保定时器实际触发
    // tick_duration is 10ms, 100ms delay needs 10 ticks, plus scheduling delay and callback execution time
    // tick_duration 是 10ms，100ms 延迟需要 10 个 tick，加上调度延迟和回调执行时间
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Try to cancel timers that have already triggered
    // 尝试取消已经触发的定时器
    let mut cancelled_after = 0;
    for handle in remaining_handles {
        if handle.cancel() {
            cancelled_after += 1;
        }
    }
    assert_eq!(
        cancelled_after, 0,
        "Triggered timers should not be cancelled"
    );
}

#[tokio::test]
async fn test_batch_cancel_no_wait() {
    // Test batch canceling without waiting for results
    // 测试批量取消而不等待结果
    let timer = TimerWheel::with_defaults();

    // Allocate handles first (先分配 handles)
    let allocated_handles = timer.allocate_handles(100);

    // Create 100 timers (创建 100 个定时器)
    let tasks: Vec<_> = (0..100)
        .map(|_| TimerTask::new_oneshot(Duration::from_secs(10), None))
        .collect();

    let batch = timer.register_batch(allocated_handles, tasks).unwrap();

    // Batch cancel (using BatchHandle's cancel_all method, now synchronous)
    // 批量取消（使用 BatchHandle 的 cancel_all 方法，现在同步操作）
    let start = Instant::now();
    let _ = batch.cancel_all();
    let elapsed = start.elapsed();

    println!("Batch cancel (without waiting) took: {:?}", elapsed);

    // Operation without waiting should be very fast (a few microseconds)
    // 没有等待的操作应该非常快（几微秒）
    assert!(
        elapsed < Duration::from_millis(10),
        "Operation without waiting should be very fast"
    );
}

#[tokio::test]
async fn test_postpone_single_timer() {
    // Test postponing a single timer
    // 测试推迟单个定时器
    let timer = TimerWheel::with_defaults();
    let counter = Arc::new(AtomicU32::new(0));
    let counter_clone = Arc::clone(&counter);

    // Create task, original callback adds 1
    let allocated_handle = timer.allocate_handle();
    let task_id = allocated_handle.task_id();
    let task = TimerTask::new_oneshot(
        Duration::from_millis(50),
        Some(CallbackWrapper::new(move || {
            let counter = Arc::clone(&counter_clone);
            async move {
                counter.fetch_add(1, Ordering::SeqCst);
            }
        })),
    );
    let handle = timer.register(allocated_handle, task);

    // Postpone task to 150ms
    // 推迟任务到 150ms
    let postponed = timer.postpone(task_id, Duration::from_millis(150), None);
    assert!(postponed, "Task should have been successfully postponed");

    // Wait for original time 50ms, task should not trigger
    // 等待原始时间 50ms，任务应该不触发
    tokio::time::sleep(Duration::from_millis(70)).await;
    assert_eq!(
        counter.load(Ordering::SeqCst),
        0,
        "Task should not trigger at original time"
    );

    // Wait for new trigger time
    // 等待新触发时间
    let (rx, _handle) = handle.into_parts();
    let result = match rx {
        CompletionReceiver::OneShot(receiver) => {
            tokio::time::timeout(Duration::from_millis(200), receiver.recv()).await.unwrap()
        }
        _ => panic!("Expected OneShot receiver"),
    };
    assert!(result.is_ok(), "Task should trigger at postponed time");

    tokio::time::sleep(Duration::from_millis(20)).await;
    assert_eq!(
        counter.load(Ordering::SeqCst),
        1,
        "Task should have been executed once"
    );
}

#[tokio::test]
async fn test_postpone_with_new_callback() {
    // Test postponing and replacing callback function
    // 测试推迟并替换回调函数
    let timer = TimerWheel::with_defaults();
    let counter = Arc::new(AtomicU32::new(0));
    let counter_clone1 = Arc::clone(&counter); // 原始回调增加 1
    let counter_clone2 = Arc::clone(&counter); // 新回调增加 10

    // Create task, original callback adds 1 (创建任务，原始回调增加 1)
    let allocated_handle = timer.allocate_handle();
    let task_id = allocated_handle.task_id();
    let task = TimerTask::new_oneshot(
        Duration::from_millis(50),
        Some(CallbackWrapper::new(move || {
            let counter = Arc::clone(&counter_clone1);
            async move {
                counter.fetch_add(1, Ordering::SeqCst);
            }
        })),
    );
    let handle = timer.register(allocated_handle, task);

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
    assert!(
        postponed,
        "Task should have been successfully postponed and replaced callback"
    );

    // Wait for task to trigger
    // 等待任务触发
    let (rx, _handle) = handle.into_parts();
    let result = match rx {
        CompletionReceiver::OneShot(receiver) => {
            tokio::time::timeout(Duration::from_millis(200), receiver.recv()).await.unwrap()
        }
        _ => panic!("Expected OneShot receiver"),
    };
    assert!(result.is_ok(), "Task should trigger");

    tokio::time::sleep(Duration::from_millis(20)).await;
    assert_eq!(
        counter.load(Ordering::SeqCst),
        10,
        "New callback should have been executed (add 10 instead of 1)"
    );
}

#[tokio::test]
async fn test_batch_postpone() {
    // Test batch postponing timers
    // 测试批量推迟定时器
    let timer = TimerWheel::with_defaults();
    let counter = Arc::new(AtomicU32::new(0));
    const BATCH_SIZE: usize = 100;

    // Create batch tasks
    // 创建批量任务
    let mut task_ids = Vec::new();
    for _ in 0..BATCH_SIZE {
        let counter_clone = Arc::clone(&counter);
        let allocated_handle = timer.allocate_handle();
        let task_id = allocated_handle.task_id();
        let task = TimerTask::new_oneshot(
            Duration::from_millis(50),
            Some(CallbackWrapper::new(move || {
                let counter = Arc::clone(&counter_clone);
                async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                }
            })),
        );
        task_ids.push((task_id, Duration::from_millis(150)));
        timer.register(allocated_handle, task);
    }

    // Batch postpone
    // 批量推迟
    let start = Instant::now();
    let postponed = timer.postpone_batch(task_ids);
    let elapsed = start.elapsed();

    println!("Batch postponing {} timers took: {:?}", BATCH_SIZE, elapsed);
    assert_eq!(
        postponed, BATCH_SIZE,
        "All tasks should have been successfully postponed"
    );

    // Wait for original time 50ms, task should not trigger
    // 等待原始时间 50ms，任务应该不触发
    tokio::time::sleep(Duration::from_millis(70)).await;
    assert_eq!(
        counter.load(Ordering::SeqCst),
        0,
        "Task should not trigger at original time"
    );

    // Wait for new trigger time
    // 等待新触发时间
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(
        counter.load(Ordering::SeqCst),
        BATCH_SIZE as u32,
        "All tasks should have been executed"
    );
}

#[tokio::test]
async fn test_postpone_batch_with_callbacks() {
    // Test batch postponing and replacing callbacks
    // 测试批量推迟并替换回调
    let timer = TimerWheel::with_defaults();
    let counter = Arc::new(AtomicU32::new(0));
    const BATCH_SIZE: usize = 50;

    // Create batch tasks (initial callbacks are empty)
    // 创建批量任务（初始回调为空）
    let mut task_ids = Vec::new();
    for _ in 0..BATCH_SIZE {
        let allocated_handle = timer.allocate_handle();
        let task_id = allocated_handle.task_id();
        let task = TimerTask::new_oneshot(Duration::from_millis(50), None);
        task_ids.push(task_id);
        timer.register(allocated_handle, task);
    }

    // Batch postpone and replace callbacks
    // 批量推迟并替换回调
    let updates: Vec<_> = task_ids
        .into_iter()
        .map(|id| {
            let counter = Arc::clone(&counter);
            (
                id,
                Duration::from_millis(150),
                Some(CallbackWrapper::new(move || {
                    let counter = Arc::clone(&counter);
                    async move {
                        counter.fetch_add(1, Ordering::SeqCst);
                    }
                })),
            )
        })
        .collect();

    let start = Instant::now();
    let postponed = timer.postpone_batch_with_callbacks(updates);
    let elapsed = start.elapsed();

    println!(
        "Batch postponing and replacing callbacks {} timers took: {:?}",
        BATCH_SIZE, elapsed
    );
    assert_eq!(
        postponed, BATCH_SIZE,
        "All tasks should have been successfully postponed"
    );

    // Wait for original time 50ms, task should not trigger
    // 等待原始时间 50ms，任务应该不触发
    tokio::time::sleep(Duration::from_millis(70)).await;
    assert_eq!(
        counter.load(Ordering::SeqCst),
        0,
        "Task should not trigger at original time"
    );

    // Wait for new trigger time
    // 等待新触发时间
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(
        counter.load(Ordering::SeqCst),
        BATCH_SIZE as u32,
        "All new callbacks should have been executed"
    );
}

#[tokio::test]
async fn test_postpone_multiple_times() {
    // Test postponing the same timer multiple times
    // 测试多次推迟同一个定时器
    let timer = TimerWheel::with_defaults();
    let counter = Arc::new(AtomicU32::new(0));
    let counter_clone = Arc::clone(&counter); // 原始回调增加 1

    let allocated_handle = timer.allocate_handle();
    let task_id = allocated_handle.task_id();
    let task = TimerTask::new_oneshot(
        Duration::from_millis(50),
        Some(CallbackWrapper::new(move || {
            let counter = Arc::clone(&counter_clone);
            async move {
                counter.fetch_add(1, Ordering::SeqCst);
            }
        })),
    );
    let handle = timer.register(allocated_handle, task);

    // First postpone to 100ms
    // 第一次推迟到 100ms
    assert!(timer.postpone(task_id, Duration::from_millis(100), None));
    tokio::time::sleep(Duration::from_millis(60)).await;
    assert_eq!(
        counter.load(Ordering::SeqCst),
        0,
        "Task should not trigger after first postpone"
    );

    // Second postpone to 150ms
    // 第二次推迟到 150ms
    assert!(timer.postpone(task_id, Duration::from_millis(150), None));
    tokio::time::sleep(Duration::from_millis(60)).await;
    assert_eq!(
        counter.load(Ordering::SeqCst),
        0,
        "Task should not trigger after second postpone"
    );

    // Third postpone to 100ms
    // 第三次推迟到 100ms
    assert!(timer.postpone(task_id, Duration::from_millis(100), None));

    // Wait for final trigger
    // 等待最终触发
    let (rx, _handle) = handle.into_parts();
    let result = match rx {
        CompletionReceiver::OneShot(receiver) => {
            tokio::time::timeout(Duration::from_millis(200), receiver.recv()).await.unwrap()
        }
        _ => panic!("Expected OneShot receiver"),
    };
    assert!(result.is_ok(), "Task should finally trigger");

    tokio::time::sleep(Duration::from_millis(20)).await;
    assert_eq!(
        counter.load(Ordering::SeqCst),
        1,
        "Task should have been executed only once"
    );
}

#[tokio::test]
async fn test_postpone_with_service() {
    // Test postponing a timer through TimerService
    // 测试通过 TimerService 推迟定时器
    let timer = TimerWheel::with_defaults();
    let mut service = timer.create_service(ServiceConfig::default());
    let counter = Arc::new(AtomicU32::new(0));
    let counter_clone = Arc::clone(&counter); // 原始回调增加 1

    let allocated_handle = service.allocate_handle();
    let task_id = allocated_handle.task_id();
    let task = TimerTask::new_oneshot(
        Duration::from_millis(50),
        Some(CallbackWrapper::new(move || {
            let counter = Arc::clone(&counter_clone);
            async move {
                counter.fetch_add(1, Ordering::SeqCst);
            }
        })),
    );
    service.register(allocated_handle, task).unwrap();

    // Postpone task
    // 推迟任务
    let postponed = service.postpone(task_id, Duration::from_millis(150), None);
    assert!(postponed, "Task should have been successfully postponed");

    // Wait for original time, task should not trigger
    // 等待原始时间，任务应该不触发
    tokio::time::sleep(Duration::from_millis(70)).await;
    assert_eq!(
        counter.load(Ordering::SeqCst),
        0,
        "Task should not trigger at original time"
    );

    // Receive timeout notification
    // 接收超时通知
    let rx = service.take_receiver().unwrap();
    let result = tokio::time::timeout(Duration::from_millis(200), rx.recv()).await;
    assert!(result.is_ok(), "Should receive timeout notification");

    tokio::time::sleep(Duration::from_millis(20)).await;
    assert_eq!(
        counter.load(Ordering::SeqCst),
        1,
        "Task should have been executed"
    );
}

#[tokio::test]
async fn test_single_wheel_multiple_services() {
    // Test race condition when a single TimerWheel is shared by multiple TimerServices
    // 测试当一个 TimerWheel 被多个 TimerServices 共享时发生竞争条件
    let timer = Arc::new(TimerWheel::with_defaults());
    let counter = Arc::new(AtomicU32::new(0));
    const SERVICE_COUNT: usize = 10;
    const TASKS_PER_SERVICE: usize = 100;

    let start = Instant::now();
    // Create multiple TimerServices, sharing the same TimerWheel
    // 创建多个 TimerServices，共享同一个 TimerWheel
    let mut services = Vec::new();
    for _ in 0..SERVICE_COUNT {
        let service = timer.create_service(ServiceConfig::default());
        services.push(service);
    }

    // Register timers from multiple services concurrently
    // 并发注册多个服务的定时器
    let mut handles = Vec::new();
    for mut service in services {
        let counter_clone = Arc::clone(&counter);
        let handle = tokio::spawn(async move {
            for i in 0..TASKS_PER_SERVICE {
                let counter_inner = Arc::clone(&counter_clone);
                let task = TimerTask::new_oneshot(
                    Duration::from_millis(50 + (i % 20) as u64),
                    Some(CallbackWrapper::new(move || {
                        let counter = Arc::clone(&counter_inner);
                        async move {
                            counter.fetch_add(1, Ordering::SeqCst);
                        }
                    })),
                );
                let handle = service.allocate_handle();
                if let Ok(_) = service.register(handle, task) {
                    // Successfully registered (成功注册)
                }
            }

            // Return service receiver for subsequent check
            // 返回服务接收器用于后续检查
            service.take_receiver()
        });
        handles.push(handle);
    }

    // Wait for all services to complete registration
    // 等待所有服务完成注册
    let mut all_receivers = Vec::new();
    for handle in handles {
        if let Ok(receiver) = handle.await {
            if let Some(rx) = receiver {
                all_receivers.push(rx);
            }
        }
    }

    println!(
        "From {} services concurrently creating {} timers took: {:?}",
        SERVICE_COUNT,
        SERVICE_COUNT * TASKS_PER_SERVICE,
        start.elapsed()
    );

    // Wait for all timers to trigger (maximum delay is 50+19=69ms, 200ms is enough)
    // 等待所有定时器触发（最大延迟是 50+19=69ms，200ms 足够）
    tokio::time::sleep(Duration::from_millis(200)).await;

    let count = counter.load(Ordering::SeqCst);
    println!(
        "Triggered timers: {} / {}",
        count,
        SERVICE_COUNT * TASKS_PER_SERVICE
    );
    assert_eq!(
        count,
        (SERVICE_COUNT * TASKS_PER_SERVICE) as u32,
        "All timers from different services should have been correctly triggered"
    );
}

#[tokio::test]
async fn test_multiple_services_concurrent_operations() {
    // Test race condition when multiple Services perform concurrent operations (register, cancel, postpone)
    // 测试当多个服务并发执行操作（注册、取消、推迟）时发生竞争条件
    let timer = Arc::new(TimerWheel::with_defaults());
    let counter = Arc::new(AtomicU32::new(0));
    const SERVICE_COUNT: usize = 5;

    let start = Instant::now();

    // Create multiple services to perform concurrent operations
    // 创建多个服务并发执行操作
    let mut handles = Vec::new();

    for service_idx in 0..SERVICE_COUNT {
        let timer_clone = Arc::clone(&timer);
        let counter_clone = Arc::clone(&counter);

        let handle = tokio::spawn(async move {
            let service = timer_clone.create_service(ServiceConfig::default());
            let mut task_ids = Vec::new();

            // Register 100 timers for each service
            // 注册 100 个定时器每个服务
            for _ in 0..100 {
                let counter_inner = Arc::clone(&counter_clone);
                let task = TimerTask::new_oneshot(
                    Duration::from_millis(100),
                    Some(CallbackWrapper::new(move || {
                        let counter = Arc::clone(&counter_inner);
                        async move {
                            counter.fetch_add(1, Ordering::SeqCst);
                        }
                    })),
                );
                let handle = service.allocate_handle();
                let task_id = handle.task_id();
                task_ids.push(task_id);

                if let Ok(_) = service.register(handle, task) {
                    // Successfully registered
                    // 成功注册
                }
            }

            // Perform different operations based on service index
            // 根据服务索引执行不同的操作
            match service_idx % 3 {
                0 => {
                    // Service 0, 3: Postpone some timers
                    // 服务 0, 3: 推迟一些定时器
                    for task_id in task_ids.iter().step_by(2) {
                        service.postpone(*task_id, Duration::from_millis(150), None);
                    }
                }
                1 => {
                    // Service 1, 4: Cancel some timers
                    // 服务 1, 4: 取消一些定时器
                    for task_id in task_ids.iter().step_by(3) {
                        timer_clone.cancel(*task_id);
                    }
                }
                _ => {
                    // Service 2: Do not perform extra operations, let timers trigger normally
                    // 服务 2: 不执行额外操作，让定时器正常触发
                }
            }

            (service_idx, task_ids.len())
        });

        handles.push(handle);
    }

    // Wait for all operations to complete
    // 等待所有操作完成
    for handle in handles {
        let _ = handle.await;
    }

    println!(
        "Multiple services concurrent operations took: {:?}",
        start.elapsed()
    );

    // Wait for all timers to trigger
    // 等待所有定时器触发
    tokio::time::sleep(Duration::from_millis(250)).await;

    let count = counter.load(Ordering::SeqCst);
    println!("Final triggered timer count: {}", count);

    // Verify:
    // Service 0: 100 tasks, 50 postponed but still trigger = 100 triggers
    // Service 1: 100 tasks, approximately 34 cancelled = approximately 66 triggers
    // Service 2: 100 tasks, all trigger = 100 triggers
    // Service 3: 100 tasks, 50 postponed but still trigger = 100 triggers
    // Service 4: 100 tasks, approximately 34 cancelled = approximately 66 triggers
    // Expected: 100 + 66 + 100 + 100 + 66 = 432 (approximately)

    // 验证：
    // 服务 0: 100 个任务，50 个推迟但仍触发 = 100 个触发
    // 服务 1: 100 个任务，大约 34 个取消 = 大约 66 个触发
    // 服务 2: 100 个任务，全部触发 = 100 个触发
    // 服务 3: 100 个任务，50 个推迟但仍触发 = 100 个触发
    // 服务 4: 100 个任务，大约 34 个取消 = 大约 66 个触发
    // 预期：100 + 66 + 100 + 100 + 66 = 432 (大约)
    assert!(
        count >= 400 && count <= 450,
        "Considering cancellation operations, the number of triggered timers should be in a reasonable range, actual: {}",
        count
    );
}

#[tokio::test]
async fn test_service_isolation() {
    // Test isolation of multiple Services (an operation of one Service should not affect other Services)
    // 测试多个服务的隔离性（一个服务的操作不应影响其他服务）
    let timer = Arc::new(TimerWheel::with_defaults());
    let counter1 = Arc::new(AtomicU32::new(0));
    let counter2 = Arc::new(AtomicU32::new(0));

    // Create two services
    // 创建两个服务
    let service1 = timer.create_service(ServiceConfig::default());
    let service2 = timer.create_service(ServiceConfig::default());

    // Service 1 registers 100 timers
    // 服务 1 注册 100 个定时器
    let mut task_ids_1 = Vec::new();
    for _ in 0..100 {
        let counter = Arc::clone(&counter1);
        let task = TimerTask::new_oneshot(
            Duration::from_millis(80),
            Some(CallbackWrapper::new(move || {
                let counter = Arc::clone(&counter);
                async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                }
            })),
        );
        let handle = service1.allocate_handle();
        task_ids_1.push(handle.task_id());
        service1.register(handle, task).unwrap();
    }

    // Service 2 registers 100 timers
    // 服务 2 注册 100 个定时器
    for _ in 0..100 {
        let counter = Arc::clone(&counter2);
        let task = TimerTask::new_oneshot(
            Duration::from_millis(80),
            Some(CallbackWrapper::new(move || {
                let counter = Arc::clone(&counter);
                async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                }
            })),
        );
        let handle = service2.allocate_handle();
        service2.register(handle, task).unwrap();
    }

    // Cancel all tasks of service1
    // 取消服务 1 的所有任务
    for task_id in task_ids_1 {
        timer.cancel(task_id);
    }

    // Wait for timers to trigger
    // 等待定时器触发
    tokio::time::sleep(Duration::from_millis(150)).await;

    let count1 = counter1.load(Ordering::SeqCst);
    let count2 = counter2.load(Ordering::SeqCst);

    println!("Service 1 triggered count: {}", count1);
    println!("Service 2 triggered count: {}", count2);
    assert_eq!(count1, 0, "Service 1 all tasks should have been cancelled");
    assert_eq!(count2, 100, "Service 2 tasks should not be affected");
}
