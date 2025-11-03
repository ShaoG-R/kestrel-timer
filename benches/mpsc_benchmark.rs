use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};

/// Benchmark: Single message send and receive (mpsc)
/// 基准测试：单条消息发送和接收（mpsc）
fn bench_mpsc_single_send_recv(c: &mut Criterion) {
    let mut group = c.benchmark_group("mpsc_single_send_recv");
    
    for capacity in [1, 10, 100, 1000].iter() {
        group.bench_with_input(BenchmarkId::from_parameter(capacity), capacity, |b, &capacity| {
            let runtime = tokio::runtime::Runtime::new().unwrap();
            
            b.to_async(&runtime).iter_custom(|iters| async move {
                let mut total_duration = Duration::from_secs(0);
                
                for _ in 0..iters {
                    // Create channel (not measured)
                    // 创建通道（不计入测量）
                    let (tx, mut rx) = mpsc::channel::<u32>(capacity);
                    
                    // Measurement stage: send and receive
                    // 测量阶段：发送和接收
                    let start = std::time::Instant::now();
                    
                    tx.send(42).await.unwrap();
                    let _ = rx.recv().await.unwrap();
                    
                    total_duration += start.elapsed();
                }
                
                total_duration
            });
        });
    }
    
    group.finish();
}

/// Benchmark: Single message send only (mpsc)
/// 基准测试：单条消息发送（mpsc）
fn bench_mpsc_single_send_only(c: &mut Criterion) {
    let mut group = c.benchmark_group("mpsc_single_send_only");
    
    for capacity in [1, 10, 100, 1000].iter() {
        group.bench_with_input(BenchmarkId::from_parameter(capacity), capacity, |b, &capacity| {
            let runtime = tokio::runtime::Runtime::new().unwrap();
            
            b.to_async(&runtime).iter_custom(|iters| async move {
                let mut total_duration = Duration::from_secs(0);
                
                for _ in 0..iters {
                    // Create channel (not measured)
                    // 创建通道（不计入测量）
                    let (tx, _rx) = mpsc::channel::<u32>(capacity);
                    
                    // Measurement stage: send only
                    // 测量阶段：只测量发送
                    let start = std::time::Instant::now();
                    
                    tx.send(42).await.unwrap();
                    
                    total_duration += start.elapsed();
                }
                
                total_duration
            });
        });
    }
    
    group.finish();
}

/// Benchmark: Batch messages send and receive (mpsc)
/// 基准测试：批量消息发送和接收（mpsc）
fn bench_mpsc_batch_send_recv(c: &mut Criterion) {
    let mut group = c.benchmark_group("mpsc_batch_send_recv");
    
    for size in [10, 100, 1000].iter() {
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, &size| {
            let runtime = tokio::runtime::Runtime::new().unwrap();
            
            b.to_async(&runtime).iter_custom(|iters| async move {
                let mut total_duration = Duration::from_secs(0);
                
                for _ in 0..iters {
                    // Create channel with sufficient capacity (not measured)
                    // 创建足够容量的通道（不计入测量）
                    let (tx, mut rx) = mpsc::channel::<u32>(size);
                    
                    // Measurement stage: send and receive batch
                    // 测量阶段：批量发送和接收
                    let start = std::time::Instant::now();
                    
                    // Send all messages
                    // 发送所有消息
                    for i in 0..size {
                        tx.send(i as u32).await.unwrap();
                    }
                    
                    // Receive all messages
                    // 接收所有消息
                    for _ in 0..size {
                        let _ = rx.recv().await.unwrap();
                    }
                    
                    total_duration += start.elapsed();
                }
                
                total_duration
            });
        });
    }
    
    group.finish();
}

/// Benchmark: Cross-task message passing (mpsc)
/// 基准测试：跨任务消息传递（mpsc）
fn bench_mpsc_cross_task(c: &mut Criterion) {
    let mut group = c.benchmark_group("mpsc_cross_task");
    
    for size in [10, 100, 1000].iter() {
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, &size| {
            let runtime = tokio::runtime::Runtime::new().unwrap();
            
            b.to_async(&runtime).iter_custom(|iters| async move {
                let mut total_duration = Duration::from_secs(0);
                
                for _ in 0..iters {
                    // Create channel (not measured)
                    // 创建通道（不计入测量）
                    let (tx, mut rx) = mpsc::channel::<u32>(size);
                    
                    // Spawn receiver task (not measured)
                    // 生成接收任务（不计入测量）
                    let receiver_handle = tokio::spawn(async move {
                        let mut count = 0;
                        while let Some(_) = rx.recv().await {
                            count += 1;
                            if count == size {
                                break;
                            }
                        }
                    });
                    
                    // Measurement stage: send messages from main task
                    // 测量阶段：从主任务发送消息
                    let start = std::time::Instant::now();
                    
                    for i in 0..size {
                        tx.send(i as u32).await.unwrap();
                    }
                    
                    // Wait for receiver to finish
                    // 等待接收完成
                    receiver_handle.await.unwrap();
                    
                    total_duration += start.elapsed();
                }
                
                total_duration
            });
        });
    }
    
    group.finish();
}

/// Benchmark: oneshot channel send and receive
/// 基准测试：oneshot 通道发送和接收
fn bench_oneshot_send_recv(c: &mut Criterion) {
    let mut group = c.benchmark_group("oneshot_send_recv");
    
    group.bench_function("oneshot", |b| {
        let runtime = tokio::runtime::Runtime::new().unwrap();
        
        b.to_async(&runtime).iter_custom(|iters| async move {
            let mut total_duration = Duration::from_secs(0);
            
            for _ in 0..iters {
                // Create oneshot channel (not measured)
                // 创建 oneshot 通道（不计入测量）
                let (tx, rx) = oneshot::channel::<u32>();
                
                // Measurement stage: send and receive
                // 测量阶段：发送和接收
                let start = std::time::Instant::now();
                
                tx.send(42).unwrap();
                let _ = rx.await.unwrap();
                
                total_duration += start.elapsed();
            }
            
            total_duration
        });
    });
    
    group.finish();
}

/// Benchmark: oneshot batch (multiple channels)
/// 基准测试：批量 oneshot（多个通道）
fn bench_oneshot_batch(c: &mut Criterion) {
    let mut group = c.benchmark_group("oneshot_batch");
    
    for size in [10, 100, 1000].iter() {
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, &size| {
            let runtime = tokio::runtime::Runtime::new().unwrap();
            
            b.to_async(&runtime).iter_custom(|iters| async move {
                let mut total_duration = Duration::from_secs(0);
                
                for _ in 0..iters {
                    // Create channels (not measured)
                    // 创建通道（不计入测量）
                    let mut channels = Vec::new();
                    for _ in 0..size {
                        channels.push(oneshot::channel::<u32>());
                    }
                    
                    // Measurement stage: send and receive all
                    // 测量阶段：全部发送和接收
                    let start = std::time::Instant::now();
                    
                    // Send all
                    // 全部发送
                    let mut receivers = Vec::new();
                    for (tx, rx) in channels {
                        tx.send(42).unwrap();
                        receivers.push(rx);
                    }
                    
                    // Receive all
                    // 全部接收
                    for rx in receivers {
                        let _ = rx.await.unwrap();
                    }
                    
                    total_duration += start.elapsed();
                }
                
                total_duration
            });
        });
    }
    
    group.finish();
}

/// Benchmark: mpsc try_send (non-blocking)
/// 基准测试：mpsc try_send（非阻塞）
fn bench_mpsc_try_send(c: &mut Criterion) {
    let mut group = c.benchmark_group("mpsc_try_send");
    
    for capacity in [10, 100, 1000].iter() {
        group.bench_with_input(BenchmarkId::from_parameter(capacity), capacity, |b, &capacity| {
            let runtime = tokio::runtime::Runtime::new().unwrap();
            
            b.to_async(&runtime).iter_custom(|iters| async move {
                let mut total_duration = Duration::from_secs(0);
                
                for _ in 0..iters {
                    // Create channel (not measured)
                    // 创建通道（不计入测量）
                    let (tx, _rx) = mpsc::channel::<u32>(capacity);
                    
                    // Measurement stage: try_send only
                    // 测量阶段：只测量 try_send
                    let start = std::time::Instant::now();
                    
                    tx.try_send(42).unwrap();
                    
                    total_duration += start.elapsed();
                }
                
                total_duration
            });
        });
    }
    
    group.finish();
}

/// Benchmark: Channel creation overhead
/// 基准测试：通道创建开销
fn bench_channel_creation(c: &mut Criterion) {
    let mut group = c.benchmark_group("channel_creation");
    
    // mpsc channel creation
    // mpsc 通道创建
    for capacity in [1, 10, 100, 1000].iter() {
        group.bench_with_input(BenchmarkId::new("mpsc", capacity), capacity, |b, &capacity| {
            b.iter(|| {
                let (_tx, _rx) = mpsc::channel::<u32>(capacity);
            });
        });
    }
    
    // oneshot channel creation
    // oneshot 通道创建
    group.bench_function("oneshot", |b| {
        b.iter(|| {
            let (_tx, _rx) = oneshot::channel::<u32>();
        });
    });
    
    group.finish();
}

/// Benchmark: Full pipeline (send + receive in separate tasks)
/// 基准测试：完整流水线（在独立任务中发送+接收）
fn bench_mpsc_pipeline(c: &mut Criterion) {
    let mut group = c.benchmark_group("mpsc_pipeline");
    
    for size in [10, 100, 1000].iter() {
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, &size| {
            let runtime = tokio::runtime::Runtime::new().unwrap();
            
            b.to_async(&runtime).iter_custom(|iters| async move {
                let mut total_duration = Duration::from_secs(0);
                
                for _ in 0..iters {
                    let (tx, mut rx) = mpsc::channel::<u32>(size);
                    
                    // Measurement stage: full pipeline
                    // 测量阶段：完整流水线
                    let start = std::time::Instant::now();
                    
                    // Spawn sender task
                    // 生成发送任务
                    let sender = tokio::spawn(async move {
                        for i in 0..size {
                            tx.send(i as u32).await.unwrap();
                        }
                    });
                    
                    // Spawn receiver task
                    // 生成接收任务
                    let receiver = tokio::spawn(async move {
                        let mut count = 0;
                        while let Some(_) = rx.recv().await {
                            count += 1;
                            if count == size {
                                break;
                            }
                        }
                    });
                    
                    // Wait for both tasks
                    // 等待两个任务完成
                    let _ = tokio::join!(sender, receiver);
                    
                    total_duration += start.elapsed();
                }
                
                total_duration
            });
        });
    }
    
    group.finish();
}

criterion_group!(
    benches,
    bench_mpsc_single_send_recv,
    bench_mpsc_single_send_only,
    bench_mpsc_batch_send_recv,
    bench_mpsc_cross_task,
    bench_oneshot_send_recv,
    bench_oneshot_batch,
    bench_mpsc_try_send,
    bench_channel_creation,
    bench_mpsc_pipeline,
);

criterion_main!(benches);

