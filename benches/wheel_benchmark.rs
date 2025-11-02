use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use std::hint::black_box;
use std::time::Duration;
use kestrel_timer::{TimerWheel, TimerTask};

/// Benchmark: single task insertion (through TimerWheel API)
/// 基准测试：单个任务插入（通过 TimerWheel API）
fn bench_wheel_insert(c: &mut Criterion) {
    let mut group = c.benchmark_group("wheel_insert");
    
    group.bench_function("insert_single", |b| {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        
        b.to_async(&runtime).iter_custom(|iters| async move {
            let mut total_duration = Duration::from_secs(0);
            
            for _ in 0..iters {
                // Preparation stage: create timer (not measured)
                // 准备阶段：创建 timer（不计入测量）
                let timer = TimerWheel::with_defaults();
                
                // Measurement stage: only measure the performance of the insertion operation
                // 测量阶段：只测量插入操作的性能
                let start = std::time::Instant::now();
                
                let task = TimerTask::new_oneshot(Duration::from_millis(100), None);
                let _handle = black_box(
                    timer.register(task)
                );
                
                total_duration += start.elapsed();
            }
            
            total_duration
        });
    });
    
    group.finish();
}

/// Benchmark: batch task insertion
/// 基准测试：批量任务插入（通过 TimerWheel API）
fn bench_wheel_insert_batch(c: &mut Criterion) {
    let mut group = c.benchmark_group("wheel_insert_batch");
    
    for size in [10, 100, 1000].iter() {
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, &size| {
            let runtime = tokio::runtime::Runtime::new().unwrap();
            
            b.to_async(&runtime).iter_custom(|iters| async move {
                let mut total_duration = Duration::from_secs(0);
                
                for _ in 0..iters {
                    // Preparation stage: create timer and tasks (not measured)
                    // 准备阶段：创建 timer 和任务（不计入测量）
                    let timer = TimerWheel::with_defaults();
                    
                    let tasks: Vec<TimerTask> = (0..size)
                        .map(|i| TimerTask::new_oneshot(Duration::from_millis(100 + i as u64 * 10), None))
                        .collect();
                    
                    // Measurement stage: only measure the performance of the batch insertion
                    // 测量阶段：只测量批量插入操作的性能
                    let start = std::time::Instant::now();
                    
                    let _batch = black_box(
                        timer.register_batch(tasks)
                    );
                    
                    total_duration += start.elapsed();
                }
                
                total_duration
            });
        });
    }
    
    group.finish();
}

/// Benchmark: single task cancellation
/// 基准测试：单个任务取消（通过 TimerWheel API）
fn bench_wheel_cancel(c: &mut Criterion) {
    let mut group = c.benchmark_group("wheel_cancel");
    
    group.bench_function("cancel_single", |b| {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        
        b.to_async(&runtime).iter_custom(|iters| async move {
            let mut total_duration = Duration::from_secs(0);
            
            for _ in 0..iters {
                // Preparation stage: create timer and insert tasks (not measured)
                // 准备阶段：创建 timer 和插入任务（不计入测量）
                let timer = TimerWheel::with_defaults();
                let task = TimerTask::new_oneshot(Duration::from_millis(100), None);
                let task_id = task.get_id();
                let _handle = timer.register(task);
                
                // Measurement stage: only measure the performance of the cancellation operation
                // 测量阶段：只测量取消操作的性能
                let start = std::time::Instant::now();
                
                let result = black_box(
                    timer.cancel(task_id)
                );
                
                total_duration += start.elapsed();
                black_box(result);
            }
            
            total_duration
        });
    });
    
    group.finish();
}

/// Benchmark: batch task cancellation
/// 基准测试：批量任务取消（通过 TimerWheel API）
fn bench_wheel_cancel_batch(c: &mut Criterion) {
    let mut group = c.benchmark_group("wheel_cancel_batch");
    
    for size in [10, 100, 1000].iter() {
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, &size| {
            let runtime = tokio::runtime::Runtime::new().unwrap();
            
            b.to_async(&runtime).iter_custom(|iters| async move {
                let mut total_duration = Duration::from_secs(0);
                
                for _ in 0..iters {
                    // Preparation stage: create timer and insert tasks (not measured)
                    // 准备阶段：创建 timer 和插入任务（不计入测量）
                    let timer = TimerWheel::with_defaults();
                    
                    let tasks: Vec<TimerTask> = (0..size)
                        .map(|i| TimerTask::new_oneshot(Duration::from_millis(100 + i as u64 * 10), None))
                        .collect();
                    let batch = timer.register_batch(tasks);
                    let task_ids: Vec<_> = batch.task_ids().to_vec();
                    
                    // Measurement stage: only measure the performance of the batch cancellation
                    // 测量阶段：只测量批量取消操作的性能
                    let start = std::time::Instant::now();
                    
                    let cancelled = black_box(
                        timer.cancel_batch(&task_ids)
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

/// Benchmark: same slot batch cancellation performance
/// 基准测试：同槽批量取消性能（通过 TimerWheel API）
fn bench_wheel_cancel_batch_same_slot(c: &mut Criterion) {
    let mut group = c.benchmark_group("wheel_cancel_batch_same_slot");
    
    for size in [10, 100, 1000].iter() {
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, &size| {
            let runtime = tokio::runtime::Runtime::new().unwrap();
            
            b.to_async(&runtime).iter_custom(|iters| async move {
                let mut total_duration = Duration::from_secs(0);
                
                for _ in 0..iters {
                    // Preparation stage: create timer and insert tasks with the same delay (will enter the same slot)
                    // 准备阶段：创建 timer 和插入任务（不计入测量）
                    let timer = TimerWheel::with_defaults();
                    
                    let tasks: Vec<TimerTask> = (0..size)
                        .map(|_| TimerTask::new_oneshot(Duration::from_millis(100), None))
                        .collect();
                    let batch = timer.register_batch(tasks);
                    let task_ids: Vec<_> = batch.task_ids().to_vec();
                    
                    // Measurement stage: only measure the performance of the batch cancellation
                    // 测量阶段：只测量批量取消操作的性能
                    let start = std::time::Instant::now();
                    
                    let cancelled = black_box(
                        timer.cancel_batch(&task_ids)
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

/// Benchmark: empty slot advance
/// 基准测试：空槽前进（通过 TimerWheel API）
/// Note: since advance is an internal operation, we use sleep to trigger the tick
/// 注意：由于前进是内部操作，我们使用 sleep 触发 tick
fn bench_wheel_advance_empty(c: &mut Criterion) {
    let mut group = c.benchmark_group("wheel_advance_empty");
    
    group.bench_function("advance_empty_slot", |b| {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        
        b.to_async(&runtime).iter_custom(|iters| async move {
            let mut total_duration = Duration::from_secs(0);
            
            for _ in 0..iters {
                // Preparation stage: create timer (not measured)
                // 准备阶段：创建 timer（不计入测量）
                let _timer = TimerWheel::with_defaults();
                
                // Measurement stage: let tick loop run once
                // 测量阶段：让 tick 循环运行一次
                let start = std::time::Instant::now();
                
                // Wait for one tick period (10ms)
                // 等待一个 tick 周期（10ms）
                tokio::time::sleep(Duration::from_millis(10)).await;
                
                total_duration += start.elapsed();
            }
            
            total_duration
        });
    });
    
    group.finish();
}

/// Benchmark: slot advance with tasks
/// 基准测试：带任务的槽前进（通过 TimerWheel API）
fn bench_wheel_advance_with_tasks(c: &mut Criterion) {
    let mut group = c.benchmark_group("wheel_advance_with_tasks");
    
    for size in [10, 100, 1000].iter() {
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, &size| {
            let runtime = tokio::runtime::Runtime::new().unwrap();
            
            b.to_async(&runtime).iter_custom(|iters| async move {
                let mut total_duration = Duration::from_secs(0);
                
                for _ in 0..iters {
                    // Preparation stage: create timer and insert tasks (not measured)
                    // 准备阶段：创建 timer 和插入任务（不计入测量）
                    let timer = TimerWheel::with_defaults();
                    
                    // Insert tasks, delay set to 20ms (ensure it will expire when advancing)
                    // 插入任务，延迟设置为 20ms（确保在前进时过期）
                    let tasks: Vec<TimerTask> = (0..size)
                        .map(|_| TimerTask::new_oneshot(Duration::from_millis(20), None))
                        .collect();
                    let _batch = timer.register_batch(tasks);
                    
                    // Measurement stage: wait for tasks to expire
                    // 测量阶段：等待任务过期
                    let start = std::time::Instant::now();
                    
                    // Wait for tasks to expire (20ms + some margin)
                    // 等待任务过期（20ms + 一些余量）
                    tokio::time::sleep(Duration::from_millis(25)).await;
                    
                    total_duration += start.elapsed();
                }
                
                total_duration
            });
        });
    }
    
    group.finish();
}

/// Benchmark: mixed rounds advance
/// 基准测试：混合轮次前进（通过 TimerWheel API）
fn bench_wheel_advance_mixed_rounds(c: &mut Criterion) {
    let mut group = c.benchmark_group("wheel_advance_mixed_rounds");
    
    group.bench_function("advance_mixed_rounds", |b| {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        
        b.to_async(&runtime).iter_custom(|iters| async move {
            let mut total_duration = Duration::from_secs(0);
            
            for _ in 0..iters {
                // Preparation stage: create timer and insert tasks with different rounds
                // 准备阶段：创建 timer 和插入任务（不计入测量）
                let timer = TimerWheel::with_defaults();
                
                // Insert tasks with different delays, produce different rounds
                // 插入任务，延迟设置为不同的值，产生不同的轮次
                let mut tasks: Vec<TimerTask> = Vec::new();
                for i in 0..100 {
                    // Delay from 20ms to 200ms
                    tasks.push(TimerTask::new_oneshot(Duration::from_millis(20 + (i % 20) * 10), None));
                }
                let _batch = timer.register_batch(tasks);
                
                // Measurement stage: advance multiple ticks
                // 测量阶段：前进多个 tick
                let start = std::time::Instant::now();
                
                // Wait for the task with the longest delay to expire
                // 等待最长时间的任务过期（210ms）
                tokio::time::sleep(Duration::from_millis(210)).await;
                
                total_duration += start.elapsed();
            }
            
            total_duration
        });
    });
    
    group.finish();
}

/// Benchmark: mixed operations (insert, cancel, advance)
/// 基准测试：混合操作（插入、取消、前进）（通过 TimerWheel API）
fn bench_wheel_mixed_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("wheel_mixed_operations");
    
    group.bench_function("insert_cancel_advance_mixed", |b| {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        
        b.to_async(&runtime).iter_custom(|iters| async move {
            let mut total_duration = Duration::from_secs(0);
            
            for _ in 0..iters {
                // Preparation stage: create timer
                // 准备阶段：创建 timer
                let timer = TimerWheel::with_defaults();
                
                // Measurement stage: perform mixed operations
                // 测量阶段：执行混合操作
                let start = std::time::Instant::now();
                
                for round in 0..10 {
                    // Insert 50 tasks
                    // 插入 50 个任务
                    let tasks: Vec<TimerTask> = (0..50)
                        .map(|_| TimerTask::new_oneshot(Duration::from_millis(50 + round * 10), None))
                        .collect();
                    let batch = timer.register_batch(tasks);
                    
                    // Cancel half of them
                    // 取消一半的任务
                    let to_cancel: Vec<_> = batch.task_ids().iter().step_by(2).copied().collect();
                    let _ = timer.cancel_batch(&to_cancel);
                    
                    // Wait for one tick
                    // 等待一个 tick
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
                
                total_duration += start.elapsed();
            }
            
            total_duration
        });
    });
    
    group.finish();
}

/// Benchmark: small batch cancel (≤10) - test optimization path
/// 基准测试：小批量取消（≤10） - 测试优化路径
fn bench_wheel_cancel_small_batch(c: &mut Criterion) {
    let mut group = c.benchmark_group("wheel_cancel_small_batch");
    
    for size in [1, 5, 10].iter() {
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, &size| {
            let runtime = tokio::runtime::Runtime::new().unwrap();
            
            b.to_async(&runtime).iter_custom(|iters| async move {
                let mut total_duration = Duration::from_secs(0);
                
                for _ in 0..iters {
                    // Preparation stage: create timer and insert tasks
                    // 准备阶段：创建 timer 和插入任务
                    let timer = TimerWheel::with_defaults();
                    
                    let tasks: Vec<TimerTask> = (0..size)
                        .map(|i| TimerTask::new_oneshot(Duration::from_millis(100 + i as u64 * 10), None))
                        .collect();
                    let batch = timer.register_batch(tasks);
                    let task_ids: Vec<_> = batch.task_ids().to_vec();
                    
                    // Measurement stage: only measure the performance of the batch cancellation
                    // 测量阶段：只测量批量取消操作的性能
                    let start = std::time::Instant::now();
                    
                    let cancelled = black_box(
                        timer.cancel_batch(&task_ids)
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

/// Benchmark: batch operations across multiple slots
/// 基准测试：跨多个槽的批量操作
fn bench_wheel_batch_multiple_slots(c: &mut Criterion) {
    let mut group = c.benchmark_group("wheel_batch_multiple_slots");
    
    for size in [100, 500, 1000].iter() {
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, &size| {
            let runtime = tokio::runtime::Runtime::new().unwrap();
            
            b.to_async(&runtime).iter_custom(|iters| async move {
                let mut total_duration = Duration::from_secs(0);
                
                for _ in 0..iters {
                    // Preparation stage: create timer and insert tasks across multiple slots
                    // 准备阶段：创建 timer 和插入任务（不计入测量）
                    let timer = TimerWheel::with_defaults();
                    
                    // Uniformly distributed across different slots
                    let tasks: Vec<TimerTask> = (0..size)
                        .map(|i| TimerTask::new_oneshot(Duration::from_millis(10 + i as u64), None))
                        .collect();
                    let batch = timer.register_batch(tasks);
                    let task_ids: Vec<_> = batch.task_ids().to_vec();
                    
                    // Measurement stage: batch cancel
                    // 测量阶段：批量取消
                    let start = std::time::Instant::now();
                    
                    let cancelled = black_box(
                        timer.cancel_batch(&task_ids)
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

/// Benchmark: single task postpone
/// 基准测试：单个任务推迟（通过 TimerWheel API）
fn bench_wheel_postpone(c: &mut Criterion) {
    let mut group = c.benchmark_group("wheel_postpone");
    
    group.bench_function("postpone_single", |b| {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        
        b.to_async(&runtime).iter_custom(|iters| async move {
            let mut total_duration = Duration::from_secs(0);
            
            for _ in 0..iters {
                // Preparation stage: create timer and insert tasks (not measured)
                // 准备阶段：创建 timer 和插入任务（不计入测量）
                let timer = TimerWheel::with_defaults();
                let task = TimerTask::new_oneshot(Duration::from_millis(100), None);
                let task_id = task.get_id();
                let _handle = timer.register(task);
                
                // Measurement stage: only measure the performance of the postpone operation
                // 测量阶段：只测量推迟操作的性能
                let start = std::time::Instant::now();
                
                let result = black_box(
                    timer.postpone(task_id, Duration::from_millis(200), None)
                );
                
                total_duration += start.elapsed();
                black_box(result);
            }
            
            total_duration
        });
    });
    
    group.finish();
}

/// Benchmark: batch task postpone
/// 基准测试：批量任务推迟（通过 TimerWheel API）
fn bench_wheel_postpone_batch(c: &mut Criterion) {
    let mut group = c.benchmark_group("wheel_postpone_batch");
    
    for size in [10, 100, 1000].iter() {
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, &size| {
            let runtime = tokio::runtime::Runtime::new().unwrap();
            
            b.to_async(&runtime).iter_custom(|iters| async move {
                let mut total_duration = Duration::from_secs(0);
                
                for _ in 0..iters {
                    // Preparation stage: create timer and insert tasks (not measured)
                    // 准备阶段：创建 timer 和插入任务（不计入测量）
                    let timer = TimerWheel::with_defaults();
                    
                    let tasks: Vec<TimerTask> = (0..size)
                        .map(|i| TimerTask::new_oneshot(Duration::from_millis(100 + i as u64 * 10), None))
                        .collect();
                    let batch = timer.register_batch(tasks);
                    let task_ids: Vec<_> = batch.task_ids().to_vec();
                    
                    // Preparation stage: prepare postpone parameters
                    let postpone_updates: Vec<_> = task_ids
                        .iter()
                        .map(|&id| (id, Duration::from_millis(200)))
                        .collect();
                    
                    // Measurement stage: only measure the performance of the batch postpone
                    // 测量阶段：只测量批量推迟操作的性能
                    let start = std::time::Instant::now();
                    
                    let postponed = black_box(
                        timer.postpone_batch(postpone_updates)
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

/// Benchmark: postpone and replace callback
/// 基准测试：推迟并替换回调（通过 TimerWheel API）
fn bench_wheel_postpone_with_callback(c: &mut Criterion) {
    let mut group = c.benchmark_group("wheel_postpone_with_callback");
    
    group.bench_function("postpone_single_with_callback", |b| {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        
        b.to_async(&runtime).iter_custom(|iters| async move {
            let mut total_duration = Duration::from_secs(0);
            
            for _ in 0..iters {
                // Preparation stage: create timer and insert tasks (not measured)
                // 准备阶段：创建 timer 和插入任务（不计入测量）
                let timer = TimerWheel::with_defaults();
                let task = TimerTask::new_oneshot(Duration::from_millis(100), None);
                let task_id = task.get_id();
                let _handle = timer.register(task);
                
                // Measurement stage: only measure the performance of the postpone and replace callback operation
                // 测量阶段：只测量推迟并替换回调操作的性能
                let start = std::time::Instant::now();
                
                let result = black_box(
                    timer.postpone(
                        task_id,
                        Duration::from_millis(200),
                        None
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

/// Benchmark: batch postpone and replace callback
/// 基准测试：批量推迟并替换回调（通过 TimerWheel API）
fn bench_wheel_postpone_batch_with_callbacks(c: &mut Criterion) {
    let mut group = c.benchmark_group("wheel_postpone_batch_with_callbacks");
    
    for size in [10, 100, 1000].iter() {
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, &size| {
            let runtime = tokio::runtime::Runtime::new().unwrap();
            
            b.to_async(&runtime).iter_custom(|iters| async move {
                let mut total_duration = Duration::from_secs(0);
                
                for _ in 0..iters {
                    // Preparation stage: create timer and insert tasks (not measured)
                    // 准备阶段：创建 timer 和插入任务（不计入测量）
                    let timer = TimerWheel::with_defaults();
                    
                    let tasks: Vec<TimerTask> = (0..size)
                        .map(|i| TimerTask::new_oneshot(Duration::from_millis(100 + i as u64 * 10), None))
                        .collect();
                    let batch = timer.register_batch(tasks);
                    let task_ids: Vec<_> = batch.task_ids().to_vec();
                    
                    // Preparation stage: prepare postpone parameters (include new callback)
                    let postpone_updates: Vec<_> = task_ids
                        .into_iter()
                        .map(|id| (id, Duration::from_millis(200)))
                        .collect();
                    
                    // Measurement stage: only measure the performance of the batch postpone and replace callback operation
                    // 测量阶段：只测量批量推迟并替换回调操作的性能
                    let start = std::time::Instant::now();
                    
                    let postponed = black_box(
                        timer.postpone_batch(postpone_updates)
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

/// Benchmark: same slot batch postpone performance
/// 基准测试：同槽批量推迟性能（通过 TimerWheel API）
fn bench_wheel_postpone_batch_same_slot(c: &mut Criterion) {
    let mut group = c.benchmark_group("wheel_postpone_batch_same_slot");
    
    for size in [10, 100, 1000].iter() {
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, &size| {
            let runtime = tokio::runtime::Runtime::new().unwrap();
            
            b.to_async(&runtime).iter_custom(|iters| async move {
                let mut total_duration = Duration::from_secs(0);
                
                for _ in 0..iters {
                    // Preparation stage: create timer and insert tasks with the same delay (will enter the same slot)
                    // 准备阶段：创建 timer 和插入任务（不计入测量）
                    let timer = TimerWheel::with_defaults();
                    
                    let tasks: Vec<TimerTask> = (0..size)
                        .map(|_| TimerTask::new_oneshot(Duration::from_millis(100), None))
                        .collect();
                    let batch = timer.register_batch(tasks);
                    let task_ids: Vec<_> = batch.task_ids().to_vec();
                    
                    // Preparation stage: prepare postpone parameters (same postpone to the same slot)
                    // 准备阶段：准备推迟参数（相同的推迟进入相同的槽）
                    let postpone_updates: Vec<_> = task_ids
                        .iter()
                        .map(|&id| (id, Duration::from_millis(200)))
                        .collect();
                    
                    // Measurement stage: only measure the performance of the batch postpone operation
                    // 测量阶段：只测量批量推迟操作的性能
                    let start = std::time::Instant::now();
                    
                    let postponed = black_box(
                        timer.postpone_batch(postpone_updates)
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

criterion_group!(
    benches,
    bench_wheel_insert,
    bench_wheel_insert_batch,
    bench_wheel_cancel,
    bench_wheel_cancel_batch,
    bench_wheel_postpone,
    bench_wheel_postpone_batch,
    bench_wheel_cancel_batch_same_slot,
    bench_wheel_advance_empty,
    bench_wheel_advance_with_tasks,
    bench_wheel_advance_mixed_rounds,
    bench_wheel_mixed_operations,
    bench_wheel_cancel_small_batch,
    bench_wheel_batch_multiple_slots,
    bench_wheel_postpone_with_callback,
    bench_wheel_postpone_batch_with_callbacks,
    bench_wheel_postpone_batch_same_slot,
);

criterion_main!(benches);

