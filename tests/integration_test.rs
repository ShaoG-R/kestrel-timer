use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use kestrel_timer::{TimerWheel, CallbackWrapper, TimerService, ServiceConfig};
use futures::future;

#[tokio::test]
async fn test_large_scale_timers() {
    // 测试大规模并发定时器（10000+ 个）
    // (Test large-scale concurrent timers (10000+))
    let timer = Arc::new(TimerWheel::with_defaults());
    let counter = Arc::new(AtomicU32::new(0));
    const TIMER_COUNT: u32 = 10_000;

    let start = Instant::now();

    // 并发创建 10000 个定时器
    // (Concurrent creation of 10000 timers)
    let mut futures = Vec::new();
    for i in 0..TIMER_COUNT {
        let timer_clone = Arc::clone(&timer);
        let counter_clone = Arc::clone(&counter);
        let delay = Duration::from_millis(10 + (i % 100) as u64);
        
        let future = async move {
            let task = TimerWheel::create_task(
                delay,
                Some(CallbackWrapper::new(move || {
                    let counter = Arc::clone(&counter_clone);
                    async move {
                        counter.fetch_add(1, Ordering::SeqCst);
                    }
                })),
            );
            timer_clone.register(task)
        };
        futures.push(future);
    }

    // 并发等待所有定时器创建完成
    // (Concurrent waiting for all timers to be created)
    future::join_all(futures).await;

    println!("创建 {} 个定时器耗时: {:?}", TIMER_COUNT, start.elapsed());
    // (Creation of 10000 timers took: {:?})

    // 等待所有定时器触发
    // (Waiting for all timers to trigger)
    tokio::time::sleep(Duration::from_millis(200)).await;

    let count = counter.load(Ordering::SeqCst);
    println!("触发的定时器数量: {}", count);
    // (Number of triggered timers: {})
    assert_eq!(count, TIMER_COUNT, "所有定时器都应该被触发");
    // (All timers should have been triggered)
}

#[tokio::test]
async fn test_timer_precision() {
    // 测试定时器的精度
    // (Test timer precision)
    let timer = TimerWheel::with_defaults();
    let start_time = Arc::new(parking_lot::Mutex::new(None::<Instant>));
    let end_time = Arc::new(parking_lot::Mutex::new(None::<Instant>));

    // (Set start time)
    *start_time.lock() = Some(Instant::now());

    // (Set end time)
    let end_clone = Arc::clone(&end_time);
    let task = TimerWheel::create_task(
        Duration::from_millis(100),
        Some(CallbackWrapper::new(move || {
            let end_time = Arc::clone(&end_clone);
            async move {
                *end_time.lock() = Some(Instant::now());
            }
        })),
    );
    let handle = timer.register(task);

    // 使用 completion_receiver 等待定时器完成，而不是固定的sleep时间
    // 这样可以避免竞态条件
    // (Wait for timer completion to avoid race conditions)
    let _ = handle.into_completion_receiver().0.await;
    
    // 额外等待一点时间确保回调完全执行完毕
    // (Additional wait to ensure callback execution is complete)
    tokio::time::sleep(Duration::from_millis(20)).await;

    let start = start_time.lock().expect("start_time should be set");
    let end = end_time.lock().expect("end_time should be set after timer completion");
    let elapsed = end.duration_since(start);

    println!("预期延迟: 100ms, 实际延迟: {:?}", elapsed);
    // (Expected delay: 100ms, actual delay: {:?})

    // 允许 ±50ms 的误差（考虑到调度延迟和系统负载）
    // (Allow ±50ms error (considering scheduling delay and system load))
    assert!(
        elapsed >= Duration::from_millis(80) && elapsed <= Duration::from_millis(180),
        "定时器精度在可接受范围内，实际延迟: {:?}", elapsed
        // (Timer precision is within acceptable range, actual delay: {:?})
    );
}

#[tokio::test]
async fn test_concurrent_operations() {
    // 测试并发操作（同时添加和取消定时器）
    // (Test concurrent operations (adding and canceling timers simultaneously))
    let timer = Arc::new(TimerWheel::with_defaults());
    let counter = Arc::new(AtomicU32::new(0));

    // 并发创建所有定时器（5个任务 × 1000个定时器 = 5000个）
    // (Concurrent creation of all timers (5 tasks × 1000 timers = 5000))

    let mut all_futures = Vec::new();
    
    for _ in 0..5 {
        for _ in 0..1000 {
            let timer_clone = Arc::clone(&timer);
            let counter_clone = Arc::clone(&counter);
            
            let future = async move {
                let task = TimerWheel::create_task(
                    Duration::from_millis(50),
                    Some(CallbackWrapper::new(move || {
                        let counter = Arc::clone(&counter_clone);
                        async move {
                            counter.fetch_add(1, Ordering::SeqCst);
                        }
                    })),
                );
                timer_clone.register(task)
            };
            
            all_futures.push(future);
        }
    }

    // 并发等待所有定时器创建完成
    // (Concurrent waiting for all timers to be created)
    future::join_all(all_futures).await;

    // 等待定时器触发
    tokio::time::sleep(Duration::from_millis(150)).await;

    let count = counter.load(Ordering::SeqCst);
    println!("触发的定时器数量: {}", count);
    // (Number of triggered timers: {})
    assert_eq!(count, 5000, "所有定时器都应该被触发");
    // (All timers should have been triggered)
}

#[tokio::test]
async fn test_timer_with_different_delays() {
    // 测试不同延迟的定时器
    // (Test timers with different delays)
    let timer = TimerWheel::with_defaults();
    let results = Arc::new(parking_lot::Mutex::new(Vec::new()));

    let delays = vec![10, 20, 30, 50, 100, 150, 200];
    let mut handles = Vec::new();
    
    for (idx, &delay_ms) in delays.iter().enumerate() {
        let results_clone = Arc::clone(&results);
        
        let task = TimerWheel::create_task(
            Duration::from_millis(delay_ms),
            Some(CallbackWrapper::new(move || {
                let results = Arc::clone(&results_clone);
                async move {
                    results.lock().push((idx, delay_ms));
                }
            })),
        );
        let handle = timer.register(task);
        
        handles.push(handle);
    }

    // 使用 completion_receiver 等待所有定时器完成，而不是固定的sleep时间
    // 这样可以确保所有定时器都真正触发了
    // (Wait for all timers to complete to ensure all timers have actually triggered)
    for handle in handles {
        let _ = handle.into_completion_receiver().0.await;
    }
    
    // 额外等待一点时间确保所有回调完全执行完毕
    // (Additional wait to ensure all callbacks are executed completely)
    tokio::time::sleep(Duration::from_millis(50)).await;

    let final_results = results.lock();
    println!("触发顺序: {:?}", final_results);
    // (Trigger order: {:?})
    assert_eq!(final_results.len(), delays.len(), "所有定时器都应该被触发");
    // (All timers should have been triggered)
}

#[tokio::test]
async fn test_memory_efficiency() {
    // 测试内存效率 - 创建大量定时器然后取消
    // (Test memory efficiency - create many timers and then cancel)
    let timer = Arc::new(TimerWheel::with_defaults());

    // 并发创建 5000 个定时器
    // (Concurrent creation of 5000 timers)
    let mut create_futures = Vec::new();
    for _ in 0..5000 {
        let timer_clone = Arc::clone(&timer);
        let future = async move {
            let task = TimerWheel::create_task(
                Duration::from_secs(10),
                None,
            );
            timer_clone.register(task)
        };
        create_futures.push(future);
    }

    let handles = future::join_all(create_futures).await;


    // 取消所有定时器（现在是同步操作）
    // (Cancel all timers (now synchronous operation))
    let cancelled_count = handles.into_iter()
        .map(|handle| handle.cancel())
        .filter(|&success| success)
        .count();

    println!("取消的定时器数量: {}", cancelled_count);
    // (Number of cancelled timers: {})
    assert_eq!(cancelled_count, 5000);
    // (All 5000 timers should have been cancelled)
}

#[tokio::test]
async fn test_batch_schedule() {
    // 测试批量调度定时器
    // (Test batch scheduling timers)
    let timer = TimerWheel::with_defaults();
    let counter = Arc::new(AtomicU32::new(0));
    
    const BATCH_SIZE: usize = 100;
    let start = Instant::now();
    
    // 创建批量回调
    // (Create batch callbacks)
    let callbacks: Vec<(Duration, _)> = (0..BATCH_SIZE)
        .map(|i| {
            let counter_clone = Arc::clone(&counter);
            let delay = Duration::from_millis(50 + (i % 10) as u64);
            let callback = Some(CallbackWrapper::new(move || {
                let counter = Arc::clone(&counter_clone);
                async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                }
                }));
            (delay, callback)
        })
        .collect();
    
    // 批量调度
    // (Batch schedule)
    let tasks = TimerWheel::create_batch_with_callbacks(callbacks);
    let batch = timer.register_batch(tasks);
    
    println!("批量调度 {} 个定时器耗时: {:?}", BATCH_SIZE, start.elapsed());
    // (Batch scheduling of 100 timers took: {:?})
    assert_eq!(batch.len(), BATCH_SIZE);
    
    // 等待所有定时器触发
    tokio::time::sleep(Duration::from_millis(150)).await;
    
    let count = counter.load(Ordering::SeqCst);
    println!("触发的定时器数量: {}", count);
    // (Number of triggered timers: {})
    assert_eq!(count, BATCH_SIZE as u32, "所有批量调度的定时器都应该被触发");
    // (All 100 timers should have been triggered)
}

#[tokio::test]
async fn test_batch_cancel() {
    // 测试批量取消定时器
    // (Test batch canceling timers)
    let timer = Arc::new(TimerWheel::with_defaults());
    const TIMER_COUNT: usize = 500;
    
    // 批量创建定时器
    // (Create batch timers)
    let delays: Vec<Duration> = (0..TIMER_COUNT)
        .map(|_| Duration::from_secs(10))
        .collect();
    
    let tasks = TimerWheel::create_batch(delays);
    let batch = timer.register_batch(tasks);
    assert_eq!(batch.len(), TIMER_COUNT);
    
    // 批量取消（使用 BatchHandle 的 cancel_all 方法）
    // (Batch cancel (using BatchHandle's cancel_all method))
    let start = Instant::now();
    let cancelled = batch.cancel_all();
    let elapsed = start.elapsed();
    
    println!("批量取消 {} 个定时器耗时: {:?}", TIMER_COUNT, elapsed);
    // (Batch canceling 500 timers took: {:?})
    assert_eq!(cancelled, TIMER_COUNT, "所有定时器都应该被成功取消");
    // (All 500 timers should have been successfully cancelled)
}

#[tokio::test]
async fn test_batch_cancel_partial() {
    // 测试部分批量取消
    // (Test partial batch canceling)
    let timer = TimerWheel::with_defaults();
    
    // 创建 10 个定时器
    // (Create 10 timers)
    let delays: Vec<Duration> = (0..10)
        .map(|_| Duration::from_millis(100))
        .collect();
    
    let tasks = TimerWheel::create_batch(delays);
    let batch = timer.register_batch(tasks);
    
    // 转换为独立的句柄
    // (Convert to individual handles)
    let mut handles = batch.into_handles();
    
    // 分离：取出前 5 个，保留后 5 个
    // (Split: take first 5, keep last 5)
    let remaining_handles = handles.split_off(5);
    
    // 取消前 5 个
    // (Cancel first 5)
    let mut cancelled_count = 0;
    for handle in handles {
        if handle.cancel() {
            cancelled_count += 1;
        }
    }
    assert_eq!(cancelled_count, 5);
    // (5 timers should have been cancelled)
    
    // 等待剩余的定时器触发 - 增加等待时间到200ms以确保定时器真正触发
    // (Wait for remaining timers to trigger - increase wait time to 200ms to ensure timers actually trigger)
    // tick_duration是10ms，100ms延迟需要10个tick，加上调度延迟和回调执行时间
    // (tick_duration is 10ms, 100ms delay needs 10 ticks, plus scheduling delay and callback execution time)
    tokio::time::sleep(Duration::from_millis(200)).await;
    
    // 尝试取消已经触发的定时器
    // (Try to cancel timers that have already triggered)
    let mut cancelled_after = 0;
    for handle in remaining_handles {
        if handle.cancel() {
            cancelled_after += 1;
        }
    }
    assert_eq!(cancelled_after, 0, "已触发的定时器不应该被取消");
    // (Triggered timers should not be cancelled)
}

#[tokio::test]
async fn test_batch_cancel_no_wait() {
    // 测试无需等待结果的批量取消
    // (Test batch canceling without waiting for results)
    let timer = TimerWheel::with_defaults();
    
    // 批量创建定时器
    // (Create 100 timers)
    let delays: Vec<Duration> = (0..100)
        .map(|_| Duration::from_secs(10))
        .collect();
    
    let tasks = TimerWheel::create_batch(delays);
    let batch = timer.register_batch(tasks);
    
    // 批量取消（使用 BatchHandle 的 cancel_all 方法，现在是同步的）
    // (Batch cancel (using BatchHandle's cancel_all method, now synchronous))
    let start = Instant::now();
    let _ = batch.cancel_all();
    let elapsed = start.elapsed();
    
    println!("批量取消（无等待）耗时: {:?}", elapsed);
    // (Batch cancel (without waiting) took: {:?})
    
    // 不等待结果的操作应该非常快（几微秒）
    // (Operation without waiting should be very fast (a few microseconds))
    assert!(elapsed < Duration::from_millis(10), "无等待的批量取消应该非常快");
}

#[tokio::test]
async fn test_postpone_single_timer() {
    // 测试单个定时器的推迟功能
    // (Test postponing a single timer)
    let timer = TimerWheel::with_defaults();
    let counter = Arc::new(AtomicU32::new(0));
    let counter_clone = Arc::clone(&counter);

    let task = TimerWheel::create_task(
        Duration::from_millis(50),
        Some(CallbackWrapper::new(move || {
            let counter = Arc::clone(&counter_clone);
            async move {
                counter.fetch_add(1, Ordering::SeqCst);
            }
        })),
    );
    let task_id = task.get_id();
    let handle = timer.register(task);

    // 推迟任务到 150ms
    let postponed = timer.postpone(task_id, Duration::from_millis(150), None);
    assert!(postponed, "任务应该成功推迟");
    // (Task should have been successfully postponed)

    // 等待原定时间 50ms，任务不应该触发
    tokio::time::sleep(Duration::from_millis(70)).await;
    assert_eq!(counter.load(Ordering::SeqCst), 0, "任务不应在原定时间触发");
    // (Task should not trigger at original time)

    // 等待新的触发时间
    let result = tokio::time::timeout(
        Duration::from_millis(200),
        handle.into_completion_receiver().0
    ).await;
    assert!(result.is_ok(), "任务应该在推迟后的时间触发");
    // (Task should trigger at postponed time)
    
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert_eq!(counter.load(Ordering::SeqCst), 1, "任务应该被执行一次");
    // (Task should have been executed once)
}

#[tokio::test]
async fn test_postpone_with_new_callback() {
    // 测试推迟并替换回调函数
    // (Test postponing and replacing callback function)
    let timer = TimerWheel::with_defaults();
    let counter = Arc::new(AtomicU32::new(0));
    let counter_clone1 = Arc::clone(&counter);
    let counter_clone2 = Arc::clone(&counter);

    // 创建任务，原始回调增加 1
    // (Create task, original callback adds 1)
    let task = TimerWheel::create_task(
        Duration::from_millis(50),
        Some(CallbackWrapper::new(move || {
            let counter = Arc::clone(&counter_clone1);
            async move {
                counter.fetch_add(1, Ordering::SeqCst);
            }
        })),
    );
    let task_id = task.get_id();
    let handle = timer.register(task);

    // 推迟任务并替换回调，新回调增加 10
    // (Postpone task and replace callback, new callback adds 10)
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
    assert!(postponed, "任务应该成功推迟并替换回调");
    // (Task should have been successfully postponed and replaced callback)

    // 等待任务触发
    let result = tokio::time::timeout(
        Duration::from_millis(200),
        handle.into_completion_receiver().0
    ).await;
    assert!(result.is_ok(), "任务应该触发");
    
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert_eq!(counter.load(Ordering::SeqCst), 10, "新回调应该被执行（增加10而不是1）");
    // (New callback should have been executed (add 10 instead of 1))
}

#[tokio::test]
async fn test_batch_postpone() {
    // 测试批量推迟定时器
    // (Test batch postponing timers)
    let timer = TimerWheel::with_defaults();
    let counter = Arc::new(AtomicU32::new(0));
    const BATCH_SIZE: usize = 100;

    // 创建批量任务
    // (Create batch tasks)
    let mut task_ids = Vec::new();
    for _ in 0..BATCH_SIZE {
        let counter_clone = Arc::clone(&counter);
        let task = TimerWheel::create_task(
            Duration::from_millis(50),
            Some(CallbackWrapper::new(move || {
                let counter = Arc::clone(&counter_clone);
                async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                }
            })),
        );
        task_ids.push((task.get_id(), Duration::from_millis(150)));
        timer.register(task);
    }

    let start = Instant::now();
    let postponed = timer.postpone_batch(task_ids);
    let elapsed = start.elapsed();
    
    println!("批量推迟 {} 个定时器耗时: {:?}", BATCH_SIZE, elapsed);
    // (Batch postponing 100 timers took: {:?})
    assert_eq!(postponed, BATCH_SIZE, "所有任务都应该成功推迟");
    // (All 100 tasks should have been successfully postponed)

    // 等待原定时间 50ms，任务不应该触发
    tokio::time::sleep(Duration::from_millis(70)).await;
    assert_eq!(counter.load(Ordering::SeqCst), 0, "任务不应在原定时间触发");
    // (Tasks should not trigger at original time)
    // 等待新的触发时间
    // (Wait for new trigger time)
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(counter.load(Ordering::SeqCst), BATCH_SIZE as u32, "所有任务都应该被执行");
    // (All 100 tasks should have been executed)
}

#[tokio::test]
async fn test_postpone_batch_with_callbacks() {
    // 测试批量推迟并替换回调
    // (Test batch postponing and replacing callbacks)
    let timer = TimerWheel::with_defaults();
    let counter = Arc::new(AtomicU32::new(0));
    const BATCH_SIZE: usize = 50;

    // 创建批量任务（初始回调为空）
    // (Create batch tasks (initial callbacks are empty))
    let mut task_ids = Vec::new();
    for _ in 0..BATCH_SIZE {
        let task = TimerWheel::create_task(
            Duration::from_millis(50),
            None,
        );
        task_ids.push(task.get_id());
        timer.register(task);
    }

    // 批量推迟并替换回调
    // (Batch postpone and replace callbacks)
    let updates: Vec<_> = task_ids
        .into_iter()
        .map(|id| {
            let counter = Arc::clone(&counter);
                (id, Duration::from_millis(150), Some(CallbackWrapper::new(move || {
                    let counter = Arc::clone(&counter);
                    async move {
                        counter.fetch_add(1, Ordering::SeqCst);
                    }
                })))
        })
        .collect();

    let start = Instant::now();
    let postponed = timer.postpone_batch_with_callbacks(updates);
    let elapsed = start.elapsed();
    
    println!("批量推迟并替换回调 {} 个定时器耗时: {:?}", BATCH_SIZE, elapsed);
    // (Batch postponing and replacing callbacks 50 timers took: {:?})
    assert_eq!(postponed, BATCH_SIZE, "所有任务都应该成功推迟");
    // (All 50 tasks should have been successfully postponed)

    // 等待原定时间 50ms，任务不应该触发
    tokio::time::sleep(Duration::from_millis(70)).await;
    assert_eq!(counter.load(Ordering::SeqCst), 0, "任务不应在原定时间触发");
    // (Tasks should not trigger at original time)

    // 等待新的触发时间
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(counter.load(Ordering::SeqCst), BATCH_SIZE as u32, "所有新回调都应该被执行");
    // (All 50 new callbacks should have been executed)
}

#[tokio::test]
async fn test_postpone_multiple_times() {
    // 测试多次推迟同一个定时器
    // (Test postponing the same timer multiple times)
    let timer = TimerWheel::with_defaults();
    let counter = Arc::new(AtomicU32::new(0));
    let counter_clone = Arc::clone(&counter);

    let task = TimerWheel::create_task(
        Duration::from_millis(50),
        Some(CallbackWrapper::new(move || {
            let counter = Arc::clone(&counter_clone);
            async move {
                counter.fetch_add(1, Ordering::SeqCst);
            }
        })),
    );
    let task_id = task.get_id();
    let handle = timer.register(task);

    // 第一次推迟到 100ms
    // (First postpone to 100ms)
    assert!(timer.postpone(task_id, Duration::from_millis(100), None));
    tokio::time::sleep(Duration::from_millis(60)).await;
    assert_eq!(counter.load(Ordering::SeqCst), 0, "第一次推迟后不应触发");

    // 第二次推迟到 150ms
    // (Second postpone to 150ms)
    assert!(timer.postpone(task_id, Duration::from_millis(150), None));
    tokio::time::sleep(Duration::from_millis(60)).await;
    assert_eq!(counter.load(Ordering::SeqCst), 0, "第二次推迟后不应触发");

    // 第三次推迟到 100ms
    // (Third postpone to 100ms)
    assert!(timer.postpone(task_id, Duration::from_millis(100), None));
    
    // 等待最终触发
    // (Wait for final trigger)
    let result = tokio::time::timeout(
        Duration::from_millis(200),
        handle.into_completion_receiver().0
    ).await;
    assert!(result.is_ok(), "任务应该最终触发");
    // (Task should finally trigger)
    
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert_eq!(counter.load(Ordering::SeqCst), 1, "任务应该只执行一次");
    // (Task should have been executed only once)
}

#[tokio::test]
async fn test_postpone_with_service() {
    // 测试通过 TimerService 推迟定时器
    // (Test postponing a timer through TimerService)
    let timer = TimerWheel::with_defaults();
    let mut service = timer.create_service(ServiceConfig::default());
    let counter = Arc::new(AtomicU32::new(0));
    // (Create counter)
    let counter_clone = Arc::clone(&counter);

    let task = TimerService::create_task(
        Duration::from_millis(50),
        Some(CallbackWrapper::new(move || {
            let counter = Arc::clone(&counter_clone);
            async move {
                counter.fetch_add(1, Ordering::SeqCst);
            }
        })),
    );
    let task_id = task.get_id();
    service.register(task).unwrap();

    // 推迟任务
    // (Postpone task)
    let postponed = service.postpone(task_id, Duration::from_millis(150), None);
    assert!(postponed, "任务应该成功推迟");
    // (Task should have been successfully postponed)

    // 等待原定时间，任务不应该触发
    tokio::time::sleep(Duration::from_millis(70)).await;
    assert_eq!(counter.load(Ordering::SeqCst), 0, "任务不应在原定时间触发");
    // (Task should not trigger at original time)

    // 接收超时通知
    // (Receive timeout notification)
    let mut rx = service.take_receiver().unwrap();
    let result = tokio::time::timeout(Duration::from_millis(200), rx.recv()).await;
    assert!(result.is_ok(), "应该收到超时通知");
    // (Should receive timeout notification)
    
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert_eq!(counter.load(Ordering::SeqCst), 1, "任务应该被执行");
    // (Task should have been executed)
}

#[tokio::test]
async fn test_single_wheel_multiple_services() {
    // 测试单个 TimerWheel 被多个 TimerService 共享时的竞态问题
    // (Test race condition when a single TimerWheel is shared by multiple TimerServices)
    let timer = Arc::new(TimerWheel::with_defaults());
    let counter = Arc::new(AtomicU32::new(0));
    const SERVICE_COUNT: usize = 10;
    const TASKS_PER_SERVICE: usize = 100;
    
    let start = Instant::now();
    // (Start time)
    // 创建多个 TimerService，共享同一个 TimerWheel
    // (Create multiple TimerServices, sharing the same TimerWheel)
    let mut services = Vec::new();
    for _ in 0..SERVICE_COUNT {
        let service = timer.create_service(ServiceConfig::default());
        services.push(service);
        // (Push service to vector)
    }
    
    // 并发地从多个 service 注册定时器
    // (Register timers from multiple services concurrently)
    let mut handles = Vec::new();
    for mut service in services {
        let counter_clone = Arc::clone(&counter);
        let handle = tokio::spawn(async move {
            for i in 0..TASKS_PER_SERVICE {
                let counter_inner = Arc::clone(&counter_clone);
                let task = TimerService::create_task(
                    Duration::from_millis(50 + (i % 20) as u64),
                    Some(CallbackWrapper::new(move || {
                        let counter = Arc::clone(&counter_inner);
                        async move {
                            counter.fetch_add(1, Ordering::SeqCst);
                        }
                    })),
                );
                
                if let Ok(_) = service.register(task) {
                    // 成功注册
                    // (Successfully registered)
                }
            }
            
            // 返回 service 的接收器以便后续检查
            // (Return service receiver for subsequent check)
            service.take_receiver()
        });
        handles.push(handle);
    }
    
    // 等待所有 service 完成注册
    // (Wait for all services to complete registration)
    let mut all_receivers = Vec::new();
    for handle in handles {
        if let Ok(receiver) = handle.await {
            if let Some(rx) = receiver {
                all_receivers.push(rx);
            }
        }
    }
    
    println!("从 {} 个 service 并发创建 {} 个定时器耗时: {:?}", 
             SERVICE_COUNT, SERVICE_COUNT * TASKS_PER_SERVICE, start.elapsed());
    // (From 10 services concurrently creating 1000 timers took: {:?})

    // 等待所有定时器触发（最长延迟是 50+19=69ms，等待 200ms 足够）
    // (Wait for all timers to trigger (maximum delay is 50+19=69ms, 200ms is enough))
    tokio::time::sleep(Duration::from_millis(200)).await;

    let count = counter.load(Ordering::SeqCst);
    println!("触发的定时器数量: {} / {}", count, SERVICE_COUNT * TASKS_PER_SERVICE);
    // (Triggered timers: {} / {})
    assert_eq!(count, (SERVICE_COUNT * TASKS_PER_SERVICE) as u32, 
               "所有来自不同 service 的定时器都应该被正确触发");
    // (All timers from different services should have been correctly triggered)
}

#[tokio::test]
async fn test_multiple_services_concurrent_operations() {
    // 测试多个 Service 并发操作（注册、取消、推迟）的竞态问题
    // (Test race condition when multiple Services perform concurrent operations (register, cancel, postpone))
    let timer = Arc::new(TimerWheel::with_defaults());
    let counter = Arc::new(AtomicU32::new(0));
    const SERVICE_COUNT: usize = 5;
    
    let start = Instant::now();
    
    // 创建多个 service 并发执行不同操作
    // (Create multiple services to perform concurrent operations)
    let mut handles = Vec::new();
    
    for service_idx in 0..SERVICE_COUNT {
        let timer_clone = Arc::clone(&timer);
        let counter_clone = Arc::clone(&counter);
        
        let handle = tokio::spawn(async move {
            let service = timer_clone.create_service(ServiceConfig::default());
            let mut task_ids = Vec::new();
            
            // 每个 service 注册 100 个定时器
            // (Register 100 timers for each service)
            for _ in 0..100 {
                let counter_inner = Arc::clone(&counter_clone);
                let task = TimerService::create_task(
                    Duration::from_millis(100),
                    Some(CallbackWrapper::new(move || {
                        let counter = Arc::clone(&counter_inner);
                        async move {
                            counter.fetch_add(1, Ordering::SeqCst);
                        }
                    })),
                );
                let task_id = task.get_id();
                task_ids.push(task_id);
                
                if let Ok(_) = service.register(task) {
                    // 成功注册
                    // (Successfully registered)
                }
            }
            
            // 根据 service 索引执行不同操作
            // (Perform different operations based on service index)
            match service_idx % 3 {
                0 => {
                    // Service 0, 3: 推迟部分定时器
                    // (Postpone some timers for Service 0, 3)
                    for task_id in task_ids.iter().step_by(2) {
                        service.postpone(*task_id, Duration::from_millis(150), None);
                    }
                }
                1 => {
                    // Service 1, 4: 取消部分定时器
                    // (Cancel some timers for Service 1, 4)
                    for task_id in task_ids.iter().step_by(3) {
                        timer_clone.cancel(*task_id);
                    }
                }
                _ => {
                    // Service 2: 不做额外操作，让定时器正常触发
                    // (Do not perform extra operations, let timers trigger normally)
                }
            }
            
            (service_idx, task_ids.len())
        });
        
        handles.push(handle);
    }
    
    // 等待所有操作完成
    // (Wait for all operations to complete)
    for handle in handles {
        let _ = handle.await;
    }
    
    println!("多个 service 并发操作耗时: {:?}", start.elapsed());
    // (Multiple services concurrent operations took: {:?})

    // 等待所有定时器触发
    // (Wait for all timers to trigger)
    tokio::time::sleep(Duration::from_millis(250)).await;
    
    let count = counter.load(Ordering::SeqCst);
    println!("最终触发的定时器数量: {}", count);
    // (Final triggered timer count: {})

    // 验证：
    // Service 0: 100 个任务，50 个被推迟但仍会触发 = 100 个触发
    // Service 1: 100 个任务，约 34 个被取消 = 约 66 个触发
    // Service 2: 100 个任务，全部触发 = 100 个触发
    // Service 3: 100 个任务，50 个被推迟但仍会触发 = 100 个触发
    // Service 4: 100 个任务，约 34 个被取消 = 约 66 个触发
    // 预期: 100 + 66 + 100 + 100 + 66 = 432 (大约)
    // (Expected: 100 + 66 + 100 + 100 + 66 = 432 (approximately))
    assert!(count >= 400 && count <= 450, 
            "考虑到取消操作，触发的定时器数量应该在合理范围内，实际: {}", count);
    // (Considering cancellation operations, the number of triggered timers should be in a reasonable range, actual: {})
}

#[tokio::test]
async fn test_service_isolation() {
    // 测试多个 Service 的隔离性（一个 Service 的操作不应影响其他 Service）
    // (Test isolation of multiple Services (an operation of one Service should not affect other Services))
    let timer = Arc::new(TimerWheel::with_defaults());
    let counter1 = Arc::new(AtomicU32::new(0));
    let counter2 = Arc::new(AtomicU32::new(0));
    
    // 创建两个 service
    // (Create two services)
    let service1 = timer.create_service(ServiceConfig::default());
    let service2 = timer.create_service(ServiceConfig::default());
    
    // Service 1 注册 100 个定时器
    // (Service 1 registers 100 timers)
    let mut task_ids_1 = Vec::new();
    for _ in 0..100 {
        let counter = Arc::clone(&counter1);
        let task = TimerService::create_task(
            Duration::from_millis(80),
            Some(CallbackWrapper::new(move || {
                let counter = Arc::clone(&counter);
                async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                }
            })),
        );
        task_ids_1.push(task.get_id());
        service1.register(task).unwrap();
    }
    
    // Service 2 注册 100 个定时器
    // (Service 2 registers 100 timers)
    for _ in 0..100 {
        let counter = Arc::clone(&counter2);
        let task = TimerService::create_task(
            Duration::from_millis(80),
            Some(CallbackWrapper::new(move || {
                let counter = Arc::clone(&counter);
                async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                }
            })),
        );
        service2.register(task).unwrap();
    }
    
    // 取消 service1 的所有任务
    // (Cancel all tasks of service1)
    for task_id in task_ids_1 {
        timer.cancel(task_id);
    }
    
    // 等待定时器触发
    // (Wait for timers to trigger)
    tokio::time::sleep(Duration::from_millis(150)).await;
    
    let count1 = counter1.load(Ordering::SeqCst);
    let count2 = counter2.load(Ordering::SeqCst);
    
    println!("Service 1 触发数量: {}", count1);
    // (Service 1 triggered count: {})
    println!("Service 2 触发数量: {}", count2);
    // (Service 2 triggered count: {})
    assert_eq!(count1, 0, "Service 1 的所有任务都应该被取消");
    // (Service 1 tasks should have been cancelled)
    assert_eq!(count2, 100, "Service 2 的任务不应受影响");
    // (Service 2 tasks should not be affected)
}

