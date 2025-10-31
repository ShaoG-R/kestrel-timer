use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;
use std::hint::black_box;
use kestrel_timer::{CallbackWrapper, TimerWheel};
use kestrel_timer::config::ServiceConfig;

/// Benchmark: Single timer scheduling
/// 基准测试：单个定时器调度
fn bench_schedule_single(c: &mut Criterion) {
    let mut group = c.benchmark_group("schedule_single");
    
    group.bench_function("schedule_once", |b| {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        
        b.to_async(&runtime).iter_custom(|iters| async move {
            let mut total_duration = Duration::from_secs(0);
            
            for _ in 0..iters {
                // Preparation stage: create timer and service (not measured)
                // 准备阶段：创建 timer 和 service（不计入测量）
                let timer = TimerWheel::with_defaults();
                let service = timer.create_service(ServiceConfig::default());
                
                // Measurement stage: only measure create_task + register performance
                // 测量阶段：只测量 create_task + register 的性能
                let start = std::time::Instant::now();
                
                let task = black_box(
                    kestrel_timer::TimerService::create_task(
                        Duration::from_secs(10),
                        None
                    )
                );
                let task_id = task.get_id();
                service.register(task).unwrap();
                
                total_duration += start.elapsed();
                black_box(task_id);
            }
            
            total_duration
        });
    });
    
    group.finish();
}

/// Benchmark: Batch timer scheduling (different sizes)
/// 基准测试：批量定时器调度（不同规模）
fn bench_schedule_batch(c: &mut Criterion) {
    let mut group = c.benchmark_group("schedule_batch");
    
    for size in [10, 100, 1000].iter() {
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, &size| {
            let runtime = tokio::runtime::Runtime::new().unwrap();
            
            b.to_async(&runtime).iter_custom(|iters| async move {
                let mut total_duration = Duration::from_secs(0);
                
                for _ in 0..iters {
                    // Preparation stage: create timer and service (not measured)
                    // 准备阶段：创建 timer 和 service（不计入测量）
                    let timer = TimerWheel::with_defaults();
                    let service = timer.create_service(ServiceConfig::default());
                    
                    let delays: Vec<_> = (0..size)
                        .map(|_| Duration::from_secs(10))
                        .collect();
                    
                    // Measurement stage: only measure create_batch + register_batch performance
                    // 测量阶段：只测量 create_batch + register_batch 的性能
                    let start = std::time::Instant::now();
                    
                    let tasks = black_box(
                        kestrel_timer::TimerService::create_batch(delays)
                    );
                    let task_ids: Vec<_> = tasks.iter().map(|t| t.get_id()).collect();
                    service.register_batch(tasks).unwrap();
                    
                    total_duration += start.elapsed();
                    black_box(task_ids);
                }
                
                total_duration
            });
        });
    }
    
    group.finish();
}

/// Benchmark: single task cancellation
/// 基准测试：单个任务取消
fn bench_cancel_single(c: &mut Criterion) {
    let mut group = c.benchmark_group("cancel_single");
    
    group.bench_function("cancel_task", |b| {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        
        b.to_async(&runtime).iter_custom(|iters| async move {
            let mut total_duration = Duration::from_secs(0);
            
            for _ in 0..iters {
                // Preparation stage: create timer, service and scheduled tasks (not measured)
                // 准备阶段：创建 timer、service 和调度任务（不计入测量）
                let timer = TimerWheel::with_defaults();
                let service = timer.create_service(ServiceConfig::default());
                
                let task = kestrel_timer::TimerService::create_task(
                    Duration::from_secs(10),
                    None
                );
                let task_id = task.get_id();
                service.register(task).unwrap();
                
                // Measurement stage: only measure cancel_task performance
                // 测量阶段：只测量 cancel_task 的性能
                let start = std::time::Instant::now();
                
                let result = black_box(
                    service.cancel_task(task_id)
                );
                
                total_duration += start.elapsed();
                black_box(result);
            }
            
            total_duration
        });
    });
    
    group.finish();
}

/// Benchmark: batch task cancellation (using optimized batch API)
/// 基准测试：批量任务取消（使用优化的批量 API）
fn bench_cancel_batch(c: &mut Criterion) {
    let mut group = c.benchmark_group("cancel_batch");
    
    for size in [10, 100, 1000].iter() {
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, &size| {
            let runtime = tokio::runtime::Runtime::new().unwrap();
            
            b.to_async(&runtime).iter_custom(|iters| async move {
                let mut total_duration = Duration::from_secs(0);
                
                for _ in 0..iters {
                    // Preparation stage: create timer, service and scheduled tasks (not measured)
                    // 准备阶段：创建 timer、service 和调度任务（不计入测量）
                    let timer = TimerWheel::with_defaults();
                    let service = timer.create_service(ServiceConfig::default());
                    
                    let delays: Vec<_> = (0..size)
                        .map(|_| Duration::from_secs(10))
                        .collect();
                    let tasks = kestrel_timer::TimerService::create_batch(delays);
                    let task_ids: Vec<_> = tasks.iter().map(|t| t.get_id()).collect();
                    service.register_batch(tasks).unwrap();
                    
                    // Measurement stage: only measure cancel_batch performance
                    // 测量阶段：只测量 cancel_batch 的性能
                    let start = std::time::Instant::now();
                    
                    let cancelled = black_box(
                        service.cancel_batch(&task_ids)
                    );
                    
                    total_duration += start.elapsed();
                    black_box(cancelled);
                }
                
                total_duration
            });
        });
    }
    
    group.finish();
}

/// Benchmark: concurrent scheduling (using optimized batch API)
/// 基准测试：并发调度
fn bench_concurrent_schedule(c: &mut Criterion) {
    let mut group = c.benchmark_group("concurrent_schedule");
    
    for concurrent_ops in [10, 50].iter() {
        group.bench_with_input(BenchmarkId::from_parameter(concurrent_ops), concurrent_ops, |b, &concurrent_ops| {
            let runtime = tokio::runtime::Runtime::new().unwrap();
            
            b.to_async(&runtime).iter_custom(|iters| async move {
                let mut total_duration = Duration::from_secs(0);
                
                for _ in 0..iters {
                    // Preparation stage: create timer and service (not measured)
                    // 准备阶段：创建 timer 和 service（不计入测量）
                    let timer = TimerWheel::with_defaults();
                    let service = Arc::new(timer.create_service(ServiceConfig::default()));
                    
                    // Measurement stage: only measure concurrent scheduling performance
                    // 测量阶段：只测量并发调度的性能
                    let start = std::time::Instant::now();
                    
                    // Concurrent execution of multiple scheduling operations
                    // 并发执行多个调度操作
                    let mut handles = Vec::new();
                    for _ in 0..concurrent_ops {
                        let service_clone = Arc::clone(&service);
                        let fut = async move {
                            let delays: Vec<_> = (0..10)
                                .map(|_| Duration::from_secs(10))
                                .collect();
                            let tasks = kestrel_timer::TimerService::create_batch(delays);
                            service_clone.register_batch(tasks).unwrap();
                        };
                        handles.push(fut);
                    }
                    
                    // Wait for all scheduling to complete
                    // 等待所有调度完成
                    let results = futures::future::join_all(handles).await;
                    
                    total_duration += start.elapsed();
                    black_box(results);
                }
                
                total_duration
            });
        });
    }
    
    group.finish();
}

/// Benchmark: high frequency cancel (using optimized batch API)
/// 基准测试：高频取消（使用优化的批量 API）
fn bench_high_frequency_cancel(c: &mut Criterion) {
    let mut group = c.benchmark_group("high_frequency_cancel");
    
    group.bench_function("cancel_1000_tasks_batch", |b| {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        
        b.to_async(&runtime).iter_custom(|iters| async move {
            let mut total_duration = Duration::from_secs(0);
            
            for _ in 0..iters {
                // Preparation stage: create timer, service and scheduled tasks (not measured)
                // 准备阶段：创建 timer、service 和调度任务（不计入测量）
                let timer = TimerWheel::with_defaults();
                let service = timer.create_service(ServiceConfig::default());
                
                let delays: Vec<_> = (0..1000)
                    .map(|_| Duration::from_secs(10))
                    .collect();
                let tasks = kestrel_timer::TimerService::create_batch(delays);
                let task_ids: Vec<_> = tasks.iter().map(|t| t.get_id()).collect();
                service.register_batch(tasks).unwrap();
                
                // Measurement stage: only measure cancel_batch performance
                // 测量阶段：只测量 cancel_batch 的性能
                let start = std::time::Instant::now();
                
                let cancelled = black_box(
                    service.cancel_batch(&task_ids)
                );
                
                total_duration += start.elapsed();
                black_box(cancelled);
            }
            
            total_duration
        });
    });
    
    group.finish();
}

/// Benchmark: mixed operations (schedule and cancel, using optimized batch API)
/// 基准测试：混合操作（调度和取消，使用优化的批量 API）
fn bench_mixed_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("mixed_operations");
    
    group.bench_function("schedule_and_cancel_interleaved", |b| {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        
        b.to_async(&runtime).iter_custom(|iters| async move {
            let mut total_duration = Duration::from_secs(0);
            
            for _ in 0..iters {
                // Preparation stage: create timer and service (not measured)
                // 准备阶段：创建 timer 和 service（不计入测量）
                let timer = TimerWheel::with_defaults();
                let service = timer.create_service(ServiceConfig::default());
                
                // Measurement stage: measure mixed operation performance
                // 测量阶段：测量混合操作的性能
                let start = std::time::Instant::now();
                
                // Alternate execution of scheduling and cancellation operations
                // 交替执行调度和取消操作
                for _ in 0..50 {
                    // 调度10个任务
                    let delays: Vec<_> = (0..10)
                        .map(|_| Duration::from_secs(10))
                        .collect();
                    let tasks = kestrel_timer::TimerService::create_batch(delays);
                    let task_ids: Vec<_> = tasks.iter().map(|t| t.get_id()).collect();
                    service.register_batch(tasks).unwrap();
                    
                    // Use batch cancel for first 5 tasks
                    // 使用批量取消前5个任务
                    let to_cancel: Vec<_> = task_ids.iter().take(5).copied().collect();
                    let cancelled = service.cancel_batch(&to_cancel);
                    
                    black_box(cancelled);
                }
                
                total_duration += start.elapsed();
            }
            
            total_duration
        });
    });
    
    group.finish();
}

/// Benchmark: schedule only notify timers
/// 基准测试：调度仅通知的定时器
fn bench_schedule_notify(c: &mut Criterion) {
    let mut group = c.benchmark_group("schedule_notify");
    
    group.bench_function("schedule_once_notify", |b| {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        
        b.to_async(&runtime).iter_custom(|iters| async move {
            let mut total_duration = Duration::from_secs(0);
            
            for _ in 0..iters {
                // Preparation stage: create timer and service (not measured)
                // 准备阶段：创建 timer 和 service（不计入测量）
                let timer = TimerWheel::with_defaults();
                let service = timer.create_service(ServiceConfig::default());
                
                // Measurement stage: only measure create_task + register performance for notify timers
                // 测量阶段：只测量仅通知定时器的创建和注册性能
                let start = std::time::Instant::now();
                
                let task = black_box(
                    kestrel_timer::TimerService::create_task(Duration::from_secs(10), None)
                );
                let task_id = task.get_id();
                service.register(task).unwrap();
                
                total_duration += start.elapsed();
                black_box(task_id);
            }
            
            total_duration
        });
    });
    
    for size in [100, 1000].iter() {
        group.bench_with_input(BenchmarkId::new("batch_notify", size), size, |b, &size| {
            let runtime = tokio::runtime::Runtime::new().unwrap();
            
            b.to_async(&runtime).iter_custom(|iters| async move {
                let mut total_duration = Duration::from_secs(0);
                
                for _ in 0..iters {
                    // Preparation stage: create timer and service (not measured)
                    // 准备阶段：创建 timer 和 service（不计入测量）
                    let timer = TimerWheel::with_defaults();
                    let service = timer.create_service(ServiceConfig::default());
                    
                    // Measurement stage: measure batch notify scheduling performance
                    // 测量阶段：测量批量通知调度的性能
                    let start = std::time::Instant::now();
                    
                    let mut tasks = Vec::new();
                    let mut task_ids = Vec::new();
                    for _ in 0..size {
                        let task = kestrel_timer::TimerService::create_task(Duration::from_secs(10), None);
                        task_ids.push(task.get_id());
                        tasks.push(task);
                    }
                    service.register_batch(tasks).unwrap();
                    
                    total_duration += start.elapsed();
                    black_box(task_ids);
                }
                
                total_duration
            });
        });
    }
    
    group.finish();
}

/// Benchmark: timer with callback performance
/// 基准测试：带回调的定时器性能
fn bench_schedule_with_callback(c: &mut Criterion) {
    let mut group = c.benchmark_group("schedule_with_callback");
    
    group.bench_function("schedule_with_simple_callback", |b| {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        
        b.to_async(&runtime).iter_custom(|iters| async move {
            let mut total_duration = Duration::from_secs(0);
            
            for _ in 0..iters {
                // Preparation stage: create timer, service and counter (not measured)
                // 准备阶段：创建 timer、service 和 counter（不计入测量）
                let timer = TimerWheel::with_defaults();
                let service = timer.create_service(ServiceConfig::default());
                let counter = Arc::new(AtomicU32::new(0));
                
                // Measurement stage: only measure create_task + register performance
                // 测量阶段：只测量 create_task + register 的性能
                let start = std::time::Instant::now();
                
                let counter_clone = Arc::clone(&counter);
                let task = black_box(
                    kestrel_timer::TimerService::create_task(
                        Duration::from_secs(10),
                        Some(CallbackWrapper::new(move || {
                            let counter = Arc::clone(&counter_clone);
                            async move {
                                counter.fetch_add(1, Ordering::SeqCst);
                            }
                        }))
                    )
                );
                let task_id = task.get_id();
                service.register(task).unwrap();
                
                total_duration += start.elapsed();
                black_box(task_id);
            }
            
            total_duration
        });
    });
    
    group.finish();
}


/// Benchmark: single task postpone (using TimerService)
/// 基准测试：单个任务推迟（通过 TimerService）
fn bench_postpone_single(c: &mut Criterion) {
    let mut group = c.benchmark_group("postpone_single");
    
    group.bench_function("postpone_task", |b| {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        
        b.to_async(&runtime).iter_custom(|iters| async move {
            let mut total_duration = Duration::from_secs(0);
            
            for _ in 0..iters {
                // Preparation stage: create timer, service and scheduled tasks (not measured)
                // 准备阶段：创建 timer、service 和调度任务（不计入测量）
                let timer = TimerWheel::with_defaults();
                let service = timer.create_service(ServiceConfig::default());
                
                let task = kestrel_timer::TimerService::create_task(
                    Duration::from_millis(100),
                    None
                );
                let task_id = task.get_id();
                service.register(task).unwrap();
                
                // Measurement stage: only measure postpone_task performance
                // 测量阶段：只测量 postpone_task 的性能
                let start = std::time::Instant::now();
                
                let result = black_box(
                    service.postpone(task_id, Duration::from_millis(200), None)
                );
                
                total_duration += start.elapsed();
                black_box(result);
            }
            
            total_duration
        });
    });
    
    group.finish();
}

/// Benchmark: batch task postpone (using TimerService)
/// 基准测试：批量任务推迟（通过 TimerService）
fn bench_postpone_batch(c: &mut Criterion) {
    let mut group = c.benchmark_group("postpone_batch");
    
    for size in [10, 100, 1000].iter() {
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, &size| {
            let runtime = tokio::runtime::Runtime::new().unwrap();
            
            b.to_async(&runtime).iter_custom(|iters| async move {
                let mut total_duration = Duration::from_secs(0);
                
                for _ in 0..iters {
                    // Preparation stage: create timer, service and scheduled tasks (not measured)
                    // 准备阶段：创建 timer、service 和调度任务（不计入测量）
                    let timer = TimerWheel::with_defaults();
                    let service = timer.create_service(ServiceConfig::default());
                    
                    let delays: Vec<_> = (0..size)
                        .map(|_| Duration::from_millis(100))
                        .collect();
                    let tasks = kestrel_timer::TimerService::create_batch(delays);
                    let task_ids: Vec<_> = tasks.iter().map(|t| t.get_id()).collect();
                    service.register_batch(tasks).unwrap();
                    
                    // Preparation stage: prepare postpone parameters
                    // 准备推迟参数
                    let postpone_updates: Vec<_> = task_ids
                        .iter()
                        .map(|&id| (id, Duration::from_millis(200)))
                        .collect();
                    
                    // Measurement stage: only measure postpone_batch performance
                    // 测量阶段：只测量 postpone_batch 的性能
                    let start = std::time::Instant::now();
                    
                    let postponed = black_box(
                        service.postpone_batch(postpone_updates)
                    );
                    
                    total_duration += start.elapsed();
                    black_box(postponed);
                }
                
                total_duration
            });
        });
    }
    
    group.finish();
}

/// Benchmark: postpone with callback (using TimerService)
/// 基准测试：推迟并替换回调（通过 TimerService）
fn bench_postpone_with_callback(c: &mut Criterion) {
    let mut group = c.benchmark_group("postpone_with_callback");
    
    group.bench_function("postpone_task_with_callback", |b| {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        
        b.to_async(&runtime).iter_custom(|iters| async move {
            let mut total_duration = Duration::from_secs(0);
            
            for _ in 0..iters {
                // Preparation stage: create timer, service and scheduled tasks (not measured)
                // 准备阶段：创建 timer、service 和调度任务（不计入测量）
                let timer = TimerWheel::with_defaults();
                let service = timer.create_service(ServiceConfig::default());
                let counter = Arc::new(AtomicU32::new(0));
                
                let task = kestrel_timer::TimerService::create_task(
                    Duration::from_millis(100),
                    None
                );
                let task_id = task.get_id();
                service.register(task).unwrap();
                
                // Measurement stage: only measure postpone_task_with_callback performance
                // 测量阶段：只测量 postpone_task_with_callback 的性能
                let start = std::time::Instant::now();
                
                let counter_clone = Arc::clone(&counter);
                let result = black_box(
                    service.postpone(
                        task_id,
                        Duration::from_millis(200),
                        Some(CallbackWrapper::new(move || {
                            let counter = Arc::clone(&counter_clone);
                            async move {
                                counter.fetch_add(1, Ordering::SeqCst);
                            }
                        }))
                    )
                );
                
                total_duration += start.elapsed();
                black_box(result);
            }
            
            total_duration
        });
    });
    
    group.finish();
}

/// Benchmark: batch postpone with callbacks (using TimerService)
/// 基准测试：批量推迟并替换回调（通过 TimerService）
fn bench_postpone_batch_with_callbacks(c: &mut Criterion) {
    let mut group = c.benchmark_group("postpone_batch_with_callbacks");
    
    for size in [10, 100, 1000].iter() {
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, &size| {
            let runtime = tokio::runtime::Runtime::new().unwrap();
            
            b.to_async(&runtime).iter_custom(|iters| async move {
                let mut total_duration = Duration::from_secs(0);
                
                for _ in 0..iters {
                    // Preparation stage: create timer, service and scheduled tasks (not measured)
                    // 准备阶段：创建 timer、service 和调度任务（不计入测量）
                    let timer = TimerWheel::with_defaults();
                    let service = timer.create_service(ServiceConfig::default());
                    let counter = Arc::new(AtomicU32::new(0));
                    
                    let delays: Vec<_> = (0..size)
                        .map(|_| Duration::from_millis(100))
                        .collect();
                    let tasks = kestrel_timer::TimerService::create_batch(delays);
                    let task_ids: Vec<_> = tasks.iter().map(|t| t.get_id()).collect();
                    service.register_batch(tasks).unwrap();
                    
                    // Preparation stage: prepare postpone parameters (include new callback)
                    // 准备推迟参数（包含新回调）
                    let postpone_updates: Vec<_> = task_ids
                        .into_iter()
                        .map(|id| {
                            let counter = Arc::clone(&counter);
                            (id, Duration::from_millis(200), Some(CallbackWrapper::new(move || {
                                let counter = Arc::clone(&counter);
                                async move {
                                    counter.fetch_add(1, Ordering::SeqCst);
                                }
                            })))
                        })
                        .collect();
                    
                    // Measurement stage: only measure postpone_batch_with_callbacks performance
                    // 测量阶段：只测量 postpone_batch_with_callbacks 的性能
                    let start = std::time::Instant::now();
                    
                    let postponed = black_box(
                        service.postpone_batch_with_callbacks(postpone_updates)
                    );
                    
                    total_duration += start.elapsed();
                    black_box(postponed);
                }
                
                total_duration
            });
        });
    }
    
    group.finish();
}

/// Benchmark: mixed operations (schedule, postpone and cancel)
/// 基准测试：混合操作（调度、推迟和取消）
fn bench_mixed_operations_with_postpone(c: &mut Criterion) {
    let mut group = c.benchmark_group("mixed_operations_with_postpone");
    
    group.bench_function("schedule_postpone_cancel_mixed", |b| {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        
        b.to_async(&runtime).iter_custom(|iters| async move {
            let mut total_duration = Duration::from_secs(0);
            
            for _ in 0..iters {
                // Preparation stage: create timer and service (not measured)
                // 准备阶段：创建 timer 和 service（不计入测量）
                let timer = TimerWheel::with_defaults();
                let service = timer.create_service(ServiceConfig::default());
                
                // Measurement stage: measure mixed operation performance
                // 测量阶段：测量混合操作的性能
                let start = std::time::Instant::now();
                
                // Alternate execution of scheduling, postponing and canceling operations
                // 交替执行调度、推迟和取消操作
                for _ in 0..30 {
                    // Schedule 15 tasks
                    // 调度15个任务
                    let delays: Vec<_> = (0..15)
                        .map(|_| Duration::from_secs(10))
                        .collect();
                    let tasks = kestrel_timer::TimerService::create_batch(delays);
                    let task_ids: Vec<_> = tasks.iter().map(|t| t.get_id()).collect();
                    service.register_batch(tasks).unwrap();
                    
                    // Postpone first 5 tasks
                    // 推迟前5个任务
                    let to_postpone: Vec<_> = task_ids.iter().take(5)
                        .map(|&id| (id, Duration::from_secs(20)))
                        .collect();
                    let postponed = service.postpone_batch(to_postpone);
                    
                    // Cancel middle 5 tasks
                    // 取消中间5个任务
                    let to_cancel: Vec<_> = task_ids.iter().skip(5).take(5).copied().collect();
                    let cancelled = service.cancel_batch(&to_cancel);
                    
                    // Measurement stage: only measure mixed operation performance
                    // 测量阶段：只测量混合操作的性能
                    black_box((postponed, cancelled));
                }
                
                total_duration += start.elapsed();
            }
            
            total_duration
        });
    });
    
    group.finish();
}

criterion_group!(
    benches,
    bench_schedule_single,
    bench_schedule_batch,
    bench_cancel_single,
    bench_cancel_batch,
    bench_postpone_single,
    bench_postpone_batch,
    bench_concurrent_schedule,
    bench_high_frequency_cancel,
    bench_mixed_operations,
    bench_schedule_notify,
    bench_schedule_with_callback,
    bench_postpone_with_callback,
    bench_postpone_batch_with_callbacks,
    bench_mixed_operations_with_postpone,
);

criterion_main!(benches);

