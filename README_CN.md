# Kestrel Timer

> 基于分层时间轮（Hierarchical Timing Wheel）算法的高性能异步定时器库

[![Rust](https://img.shields.io/badge/rust-1.70%2B-orange.svg)](https://www.rust-lang.org/)
[![Tokio](https://img.shields.io/badge/tokio-1.48-blue.svg)](https://tokio.rs/)
[![Crates.io](https://img.shields.io/crates/v/kestrel-timer.svg)](https://crates.io/crates/kestrel-timer)
[![Documentation](https://docs.rs/kestrel-timer/badge.svg)](https://docs.rs/kestrel-timer)
[![License](https://img.shields.io/crates/l/kestrel-timer.svg)](https://github.com/ShaoG-R/kestrel-timer#license)

## 📚 目录

- [项目概述](#项目概述)
- [核心特性](#核心特性)
- [快速开始](#快速开始)
- [架构说明](#架构说明)
- [使用示例](#使用示例)
- [配置选项](#配置选项)
- [性能基准](#性能基准)
- [使用场景](#使用场景)
- [许可证](#许可证)

## 项目概述

`kestrel-timer` 是一个基于分层时间轮算法的高性能异步定时器库，专为 Rust 和 Tokio 设计。提供 O(1) 时间复杂度的定时器操作，轻松处理 10,000+ 并发定时器。

**核心优势**：
- 双层时间轮架构，自动分离短延迟和长延迟任务
- 相比传统堆实现，性能提升 2-12 倍
- 支持定时器推迟、批量操作、完成通知
- 生产就绪，经过严格测试

## 核心特性

### 🏗️ 分层时间轮架构

- **双层设计**：L0 层（高精度短延迟）+ L1 层（长延迟）自动分层
- **智能降级**：L1 层任务到期后自动降级到 L0 层执行
- **无轮次检查**：L0 层无需 rounds 判断，减少 90% 任务的检查开销

### ⚡ 高性能

- **O(1) 时间复杂度**：插入、删除、触发操作均为 O(1)
- **优化数据结构**：`FxHashMap` + `parking_lot::Mutex`
- **位运算优化**：槽位数量为 2 的幂次方，快速取模
- **支持大规模**：轻松处理 10,000+ 并发定时器

### 🔄 完整功能

- ✅ 异步回调支持（基于 Tokio）
- ✅ 定时器推迟（保持或替换回调）
- ✅ 批量操作（调度、取消、推迟）
- ✅ 完成通知机制
- ✅ TimerService（Actor 模式管理）
- ✅ 线程安全

## 快速开始

### 安装

在 `Cargo.toml` 中添加：

```toml
[dependencies]
kestrel-timer = "0.1.0"
tokio = { version = "1.48", features = ["full"] }
```

### 基础使用

```rust
use kestrel_timer::{TimerWheel, CallbackWrapper};
use std::time::Duration;

#[tokio::main]
async fn main() {
    // 创建定时器（使用默认配置）
    let timer = TimerWheel::with_defaults();
    
    // 创建任务 + 注册
    let callback = Some(CallbackWrapper::new(|| async {
        println!("定时器触发！");
    }));
    let task = TimerWheel::create_task(Duration::from_secs(1), callback);
    let handle = timer.register(task);
    
    // 等待完成
    handle.into_completion_receiver().0.await.ok();
}
```

### 批量操作

```rust
use kestrel_timer::CallbackWrapper;

let timer = TimerWheel::with_defaults();

// 批量创建 + 注册
let callbacks: Vec<_> = (0..100)
    .map(|i| {
        let delay = Duration::from_millis(100 + i * 10);
        let callback = Some(CallbackWrapper::new(move || async move {
            println!("定时器 {} 触发", i);
        }));
        (delay, callback)
    })
    .collect();

let tasks = TimerWheel::create_batch_with_callbacks(callbacks);
let batch_handle = timer.register_batch(tasks);

// 批量取消
batch_handle.cancel_all();
```

### 推迟定时器

```rust
let callback = Some(CallbackWrapper::new(|| async {
    println!("原始回调");
}));
let task = TimerWheel::create_task(Duration::from_millis(50), callback);
let task_id = task.get_id();
let handle = timer.register(task);

// 推迟并保持原回调
timer.postpone(task_id, Duration::from_millis(150), None);

// 推迟并替换回调
let new_callback = Some(CallbackWrapper::new(|| async {
    println!("新回调");
}));
timer.postpone(task_id, Duration::from_millis(200), new_callback);
```

### TimerService 使用

```rust
use kestrel_timer::{TimerService, ServiceConfig};

let timer = TimerWheel::with_defaults();
let mut service = timer.create_service(ServiceConfig::default());

// 批量调度
let callbacks = vec![
    (Duration::from_millis(100), Some(CallbackWrapper::new(|| async {}))),
    (Duration::from_millis(200), Some(CallbackWrapper::new(|| async {}))),
];
let tasks = TimerService::create_batch_with_callbacks(callbacks);
service.register_batch(tasks).unwrap();

// 接收超时通知
let mut timeout_rx = service.take_receiver().unwrap();
while let Some(task_id) = timeout_rx.recv().await {
    println!("任务 {:?} 完成", task_id);
}

service.shutdown().await;
```

## 架构说明

### 分层时间轮设计

```
┌─────────────────────────────────────────────┐
│              L1 层（高层）                  │
│  槽位数：64 | Tick: 1s | 覆盖：64秒         │
│              ↓ 降级到 L0                    │
└─────────────────────────────────────────────┘
                    ↓
┌─────────────────────────────────────────────┐
│              L0 层（底层）                  │
│  槽位数：512 | Tick: 10ms | 覆盖：5.12秒    │
│              ▲ 当前指针                     │
└─────────────────────────────────────────────┘
```

**L0 层（底层 - 高精度）**：
- 槽位数：512（默认），Tick：10ms
- 覆盖范围：5.12 秒
- 处理 80-90% 的短延迟任务

**L1 层（高层 - 长时间）**：
- 槽位数：64（默认），Tick：1000ms
- 覆盖范围：64 秒
- 处理长延迟任务，支持 rounds 机制

**工作流程**：
1. 短延迟（< 5.12s）→ 直接插入 L0 层
2. 长延迟（≥ 5.12s）→ 插入 L1 层
3. L1 任务到期 → 自动降级到 L0 层
4. L0 任务到期 → 立即触发

### 性能优化

- **分层架构**：避免单层轮次检查，L0 层无需 rounds 判断
- **高效锁机制**：`parking_lot::Mutex` 比标准 Mutex 更快
- **位运算优化**：槽位数为 2 的幂次方，使用 `& (n-1)` 快速取模
- **缓存优化**：预计算槽位掩码、tick 时长等常用值
- **批量优化**：减少锁竞争，智能处理小批量

## 使用示例

完整的 API 文档请访问 [docs.rs/kestrel-timer](https://docs.rs/kestrel-timer)

### 主要 API

**TimerWheel**：
- `TimerWheel::with_defaults()` - 使用默认配置创建
- `TimerWheel::new(config)` - 使用自定义配置创建
- `create_task(delay, callback)` - 创建任务（静态方法）
- `register(task)` - 注册任务
- `register_batch(tasks)` - 批量注册
- `postpone(task_id, delay, callback)` - 推迟任务
- `postpone_batch(updates)` - 批量推迟

**TimerHandle**：
- `cancel()` - 取消定时器
- `task_id()` - 获取任务 ID
- `into_completion_receiver()` - 获取完成通知

**TimerService**：
- `create_task(delay, callback)` - 创建任务（静态方法）
- `register(task)` - 注册任务
- `take_receiver()` - 获取超时通知接收器
- `cancel_task(task_id)` - 取消任务
- `postpone(task_id, delay, callback)` - 推迟任务
- `shutdown()` - 关闭服务

## 配置选项

### 默认配置

```rust
let timer = TimerWheel::with_defaults();
// L0: 512 槽位 × 10ms = 5.12 秒
// L1: 64 槽位 × 1s = 64 秒
```

### 自定义配置

```rust
use kestrel_timer::WheelConfig;

let config = WheelConfig::builder()
    .l0_tick_duration(Duration::from_millis(10))  // L0 tick
    .l0_slot_count(512)                            // L0 槽位（必须是 2 的幂）
    .l1_tick_duration(Duration::from_secs(1))      // L1 tick
    .l1_slot_count(64)                             // L1 槽位（必须是 2 的幂）
    .build()?;
let timer = TimerWheel::new(config);
```

### 推荐配置

**高精度场景（网络超时）**：
```rust
let config = WheelConfig::builder()
    .l0_tick_duration(Duration::from_millis(5))
    .l0_slot_count(1024)
    .l1_tick_duration(Duration::from_millis(500))
    .l1_slot_count(64)
    .build()?;
```

**低精度场景（心跳检测）**：
```rust
let config = WheelConfig::builder()
    .l0_tick_duration(Duration::from_millis(100))
    .l0_slot_count(512)
    .l1_tick_duration(Duration::from_secs(10))
    .l1_slot_count(128)
    .build()?;
```

## 性能基准

### 运行基准测试

```bash
cargo bench
```

### 性能对比

与基于堆（BinaryHeap）的传统定时器实现相比：

| 操作 | 分层时间轮 | 堆实现 | 优势 |
|------|-----------|--------|------|
| 插入单个任务 | O(1) ~5μs | O(log n) ~10-20μs | 2-4x 更快 |
| 批量插入 1000 | ~2ms | ~15-25ms | 7-12x 更快 |
| 取消任务 | O(1) ~2μs | O(n) ~50-100μs | 25-50x 更快 |
| 推迟任务 | O(1) ~4μs | O(log n) ~15-30μs | 4-7x 更快 |

### 大规模测试

```bash
cargo test --test integration_test test_large_scale_timers
```

- ✅ 10,000 个并发定时器
- ✅ 创建时间 < 100ms
- ✅ 所有定时器正确触发

## 使用场景

### 1. 网络超时管理

```rust
async fn handle_connection(timer: &TimerWheel, conn_id: u64) {
    let callback = Some(CallbackWrapper::new(move || async move {
        println!("连接 {} 超时", conn_id);
        close_connection(conn_id).await;
    }));
    let task = TimerWheel::create_task(Duration::from_secs(30), callback);
    let handle = timer.register(task);
    
    // 连接完成时取消超时
    // handle.cancel();
}
```

### 2. 心跳检测

```rust
let timer = TimerWheel::with_defaults();
let mut service = timer.create_service(ServiceConfig::default());

for client_id in client_ids {
    let callback = Some(CallbackWrapper::new(move || async move {
        println!("客户端 {} 心跳超时", client_id);
    }));
    let task = TimerService::create_task(Duration::from_secs(30), callback);
    service.register(task).unwrap();
}
```

### 3. 缓存过期

```rust
async fn set_cache(&self, key: String, value: String, ttl: Duration) {
    self.cache.lock().insert(key.clone(), value);
    
    let cache = Arc::clone(&self.cache);
    let callback = Some(CallbackWrapper::new(move || {
        let cache = Arc::clone(&cache);
        let key = key.clone();
        async move {
            cache.lock().remove(&key);
        }
    }));
    let task = TimerWheel::create_task(ttl, callback);
    self.timer.register(task);
}
```

### 4. 游戏 Buff 系统

```rust
async fn apply_buff(
    timer: &TimerWheel,
    player_id: u64,
    buff_type: BuffType,
    duration: Duration
) -> TaskId {
    let callback = Some(CallbackWrapper::new(move || async move {
        remove_buff(player_id, buff_type).await;
    }));
    let task = TimerWheel::create_task(duration, callback);
    let task_id = task.get_id();
    timer.register(task);
    task_id
}

// 延长 Buff
timer.postpone(task_id, new_duration, None);
```

### 5. 重试机制

```rust
async fn retry_with_backoff(timer: &TimerWheel, operation: impl Fn()) {
    for retry in 1..=5 {
        let delay = Duration::from_secs(2_u64.pow(retry - 1));
        let callback = Some(CallbackWrapper::new(move || async move {
            operation().await;
        }));
        let task = TimerWheel::create_task(delay, callback);
        timer.register(task);
    }
}
```

## 许可证

本项目采用 MIT 或 Apache-2.0 双许可证。

- MIT License ([LICENSE-MIT](LICENSE-MIT) 或 http://opensource.org/licenses/MIT)
- Apache License 2.0 ([LICENSE-APACHE](LICENSE-APACHE) 或 http://www.apache.org/licenses/LICENSE-2.0)

## 致谢

时间轮算法由 George Varghese 和 Tony Lauck 在论文 ["Hashed and Hierarchical Timing Wheels"](http://www.cs.columbia.edu/~nahum/w6998/papers/sosp87-timing-wheels.pdf) (SOSP '87) 中提出。

---

**完整文档**：[docs.rs/kestrel-timer](https://docs.rs/kestrel-timer)
