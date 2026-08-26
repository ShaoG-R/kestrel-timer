# Kestrel Timer

> 基于分层时间轮（Hierarchical Timing Wheel）算法的高性能异步定时器库

[![Rust](https://img.shields.io/badge/rust-1.88%2B-orange.svg)](https://www.rust-lang.org/)
[![Tokio](https://img.shields.io/badge/tokio-1.48-blue.svg)](https://tokio.rs/)
[![Crates.io](https://img.shields.io/crates/v/kestrel-timer.svg)](https://crates.io/crates/kestrel-timer)
[![Documentation](https://docs.rs/kestrel-timer/badge.svg)](https://docs.rs/kestrel-timer)
[![License](https://img.shields.io/crates/l/kestrel-timer.svg)](https://github.com/shaogme/kestrel-timer#license)

## 目录

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

### 分层时间轮架构

- **双层设计**：L0 层（高精度短延迟）+ L1 层（长延迟）自动分层
- **智能降级**：L1 层任务到期后自动降级到 L0 层执行
- **无轮次检查**：L0 层无需 rounds 判断，减少 90% 任务的检查开销

### 高性能

- **O(1) 时间复杂度**：插入、删除、触发操作均为 O(1)
- **优化数据结构**：
  - `DeferredMap`（代数竞技场）进行任务索引，O(1) 操作
  - `parking_lot::Mutex` 提供高效锁机制
- **位运算优化**：槽位数量为 2 的幂次方，快速取模
- **支持大规模**：轻松处理 10,000+ 并发定时器

### 完整功能

- 异步回调支持（基于 Tokio）
- 定时器推迟（保持或替换回调）
- 批量操作（调度、取消、推迟）
- 完成通知机制
- TimerService（Actor 模式管理）
- 线程安全

## 快速开始

### 安装

在 `Cargo.toml` 中添加：

```toml
[dependencies]
kestrel-timer = "0.4"
tokio = { version = "1.48", features = ["full"] }
```

### 基础使用

```rust,no_run
use kestrel_timer::{TimerWheel, CallbackWrapper, TimerTask};
use std::time::Duration;

#[tokio::main]
async fn main() {
    // 创建定时器（使用默认配置）
    let timer = TimerWheel::with_defaults();
    
    // 步骤 1: 分配 handle
    let handle = timer.allocate_handle();
    let _task_id = handle.task_id();
    
    // 步骤 2: 创建任务
    let callback = Some(CallbackWrapper::new(|| async {
        println!("定时器触发！");
    }));
    let task = TimerTask::new_oneshot(Duration::from_secs(1), callback);
    
    // 步骤 3: 注册任务
    let _timer_handle = timer.register(handle, task).unwrap();
    
    // 等待完成或取消
    // timer_handle.cancel();
}
```

### 批量操作

```rust,no_run
use kestrel_timer::{TimerWheel, CallbackWrapper, TimerTask};
use std::time::Duration;

let timer = TimerWheel::with_defaults();

// 步骤 1: 批量分配 handles
let handles = timer.allocate_handles(100);
let _task_ids: Vec<_> = handles.iter().map(|h| h.task_id()).collect();

// 步骤 2: 创建任务
let tasks: Vec<_> = (0..100)
    .map(|i| {
        let delay = Duration::from_millis(100 + i * 10);
        let callback = Some(CallbackWrapper::new(move || async move {
            println!("定时器 {} 触发", i);
        }));
        TimerTask::new_oneshot(delay, callback)
    })
    .collect();

// 步骤 3: 批量注册
let batch_handle = timer.register_batch(handles, tasks).unwrap();

// 批量取消
batch_handle.cancel_all().unwrap();
```

### 推迟定时器

```rust,no_run
use kestrel_timer::{TimerWheel, CallbackWrapper, TimerTask};
use std::time::Duration;

let timer = TimerWheel::with_defaults();

// 步骤 1: 分配 handle 并获取 task_id
let handle = timer.allocate_handle();
let task_id = handle.task_id();

// 步骤 2: 创建并注册任务
let callback = Some(CallbackWrapper::new(|| async {
    println!("原始回调");
}));
let task = TimerTask::new_oneshot(Duration::from_millis(50), callback);
let _timer_handle = timer.register(handle, task).unwrap();

// 推迟并保持原回调
timer.postpone(task_id, Duration::from_millis(150), None).unwrap();

// 推迟并替换回调
let new_callback = Some(CallbackWrapper::new(|| async {
    println!("新回调");
}));
timer.postpone(task_id, Duration::from_millis(200), new_callback).unwrap();
```

### TimerService 使用

```rust,no_run
use kestrel_timer::{TimerWheel, TimerTask, CallbackWrapper, TaskNotification};
use kestrel_timer::config::ServiceConfig;
use std::time::Duration;

#[tokio::main]
async fn main() {
let timer = TimerWheel::with_defaults();
let mut service = timer.create_service(ServiceConfig::default());

// 步骤 1: 分配 handles
let handles = service.allocate_handles(2);

// 步骤 2: 创建任务
let tasks: Vec<_> = vec![
    TimerTask::new_oneshot(Duration::from_millis(100), Some(CallbackWrapper::new(|| async {}))),
    TimerTask::new_oneshot(Duration::from_millis(200), Some(CallbackWrapper::new(|| async {}))),
];

// 步骤 3: 批量注册
service.register_batch(handles, tasks).unwrap();

// 接收两个超时通知，然后关闭服务
let mut timeout_rx = service.take_receiver().unwrap();
for _ in 0..2 {
    if let Some(notification) = timeout_rx.recv().await {
        match notification {
            TaskNotification::OneShot(task_id) => {
                println!("一次性任务 {:?} 完成", task_id);
            },
            TaskNotification::Periodic(task_id) => {
                println!("周期性任务 {:?} 被调用", task_id);
            },
        }
    }
}

service.shutdown().await;
}
```

## 架构说明

### 分层时间轮设计

```text
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

### 基于 DeferredMap 的任务索引

使用 `DeferredMap`（代数竞技场）实现高效任务管理：

- **两步注册流程**：
  - 分配 handle 获取任务 ID（轻量操作，无需准备任务值）
  - 使用 handle 插入任务（携带完成通知器）

- **代数安全**：每个任务 ID 包含：
  - 低 32 位：槽位索引
  - 高 32 位：代数计数器
  - 防止释放后使用和 ABA 问题

- **内存高效**：基于联合体的槽位存储
  - 已占用槽位：存储任务数据
  - 空闲槽位：存储空闲链表指针

### 性能优化

- **分层架构**：避免单层轮次检查，L0 层无需 rounds 判断
- **DeferredMap**：O(1) 任务查找、插入和删除，提供代数安全
- **高效锁机制**：`parking_lot::Mutex` 比标准 Mutex 更快
- **位运算优化**：槽位数为 2 的幂次方，使用 `& (n-1)` 快速取模
- **缓存优化**：预计算槽位掩码、tick 时长等常用值
- **批量优化**：减少锁竞争，智能处理小批量

## 使用示例

完整的 API 文档请访问 [docs.rs/kestrel-timer](https://docs.rs/kestrel-timer)

### 主要 API

**TimerTask**：

- `TimerTask::new_oneshot(delay, callback)` - 创建一次性任务
- `TimerTask::new_periodic(initial_delay, interval, callback, buffer_size)` - 创建周期性任务
- `get_task_type()` - 获取任务类型
- `get_interval()` - 获取周期任务的间隔时间

**TaskHandle**（预分配的句柄）：

- `task_id()` - 从句柄获取任务 ID

**TimerWheel**：

- `TimerWheel::with_defaults()` - 使用默认配置创建
- `TimerWheel::new(config, batch_config)` - 使用自定义配置创建并返回 `Result`
- `allocate_handle()` - 分配单个 handle
- `allocate_handles(count)` - 批量分配 handles
- `register(handle, task)` - 使用 handle 注册任务
- `register_batch(handles, tasks)` - 批量注册任务
- `cancel(task_id)` - 取消任务
- `cancel_batch(task_ids)` - 批量取消
- `postpone(task_id, delay, callback)` - 推迟任务
- `postpone_batch(updates)` - 批量推迟

**TimerHandle**（注册后返回的句柄）：

- `cancel()` - 取消定时器

**TimerService**：

- `allocate_handle()` - 分配单个 handle
- `allocate_handles(count)` - 批量分配 handles
- `register(handle, task)` - 使用 handle 注册任务
- `register_batch(handles, tasks)` - 批量注册任务
- `take_receiver()` - 获取超时通知接收器
- `cancel_task(task_id)` - 取消任务
- `cancel_batch(task_ids)` - 批量取消
- `postpone(task_id, delay, callback)` - 推迟任务
- `postpone_batch(updates)` - 批量推迟
- `shutdown()` - 关闭服务

## 配置选项

### 默认配置

```rust,no_run
use kestrel_timer::TimerWheel;

let _timer = TimerWheel::with_defaults();
// L0: 512 槽位 × 10ms = 5.12 秒
// L1: 64 槽位 × 1s = 64 秒
```

### 自定义配置

```rust,no_run
use kestrel_timer::{config::{BatchConfig, WheelConfig}, TimerWheel};
use std::time::Duration;

let config = WheelConfig::builder()
    .l0_tick_duration(Duration::from_millis(10))  // L0 tick
    .l0_slot_count(512)                            // L0 槽位（必须是 2 的幂）
    .l1_tick_duration(Duration::from_secs(1))      // L1 tick
    .l1_slot_count(64)                             // L1 槽位（必须是 2 的幂）
    .build()
    .unwrap();
let _timer = TimerWheel::new(config, BatchConfig::default()).unwrap();
```

### 推荐配置

**高精度场景（网络超时）**：

```rust,no_run
use kestrel_timer::config::WheelConfig;
use std::time::Duration;

let config = WheelConfig::builder()
    .l0_tick_duration(Duration::from_millis(5))
    .l0_slot_count(1024)
    .l1_tick_duration(Duration::from_millis(500))
    .l1_slot_count(64)
    .build()
    .unwrap();
```

**低精度场景（心跳检测）**：

```rust,no_run
use kestrel_timer::config::WheelConfig;
use std::time::Duration;

let config = WheelConfig::builder()
    .l0_tick_duration(Duration::from_millis(100))
    .l0_slot_count(512)
    .l1_tick_duration(Duration::from_secs(10))
    .l1_slot_count(128)
    .build()
    .unwrap();
```

## 性能基准

### 运行基准测试

```bash
cargo bench
```

### 性能对比

与基于堆（BinaryHeap）的传统定时器实现相比：

| 操作 | 分层时间轮 | 堆实现 | 优势 |
| --- | --- | --- | --- |
| 插入单个任务 | O(1) ~5μs | O(log n) ~10-20μs | 2-4x 更快 |
| 批量插入 1000 | ~2ms | ~15-25ms | 7-12x 更快 |
| 取消任务 | O(1) ~2μs | O(n) ~50-100μs | 25-50x 更快 |
| 推迟任务 | O(1) ~4μs | O(log n) ~15-30μs | 4-7x 更快 |

### 大规模测试

```bash
cargo test --test integration_test test_large_scale_timers
```

- 10,000 个并发定时器
- 创建时间 < 100ms
- 所有定时器正确触发

## 使用场景

### 1. 网络超时管理

```rust,no_run
use kestrel_timer::{TimerWheel, CallbackWrapper, TimerTask};
use std::time::Duration;

# async fn close_connection(_conn_id: u64) {}

async fn handle_connection(timer: &TimerWheel, conn_id: u64) {
    // 先分配 handle
    let handle = timer.allocate_handle();
    
    // 创建任务
    let callback = Some(CallbackWrapper::new(move || async move {
        println!("连接 {} 超时", conn_id);
        close_connection(conn_id).await;
    }));
    let task = TimerTask::new_oneshot(Duration::from_secs(30), callback);
    
    // 注册任务
    let _timer_handle = timer.register(handle, task).unwrap();
    
    // 连接完成时取消超时
    // timer_handle.cancel();
}
```

### 2. 心跳检测

```rust,no_run
use kestrel_timer::{TimerWheel, TimerTask, CallbackWrapper};
use kestrel_timer::config::ServiceConfig;
use std::time::Duration;

# let client_ids = [1_u64, 2_u64];
let timer = TimerWheel::with_defaults();
let service = timer.create_service(ServiceConfig::default());

for client_id in client_ids {
    // 分配 handle
    let handle = service.allocate_handle();
    
    // 创建并注册任务
    let callback = Some(CallbackWrapper::new(move || async move {
        println!("客户端 {} 心跳超时", client_id);
    }));
    let task = TimerTask::new_oneshot(Duration::from_secs(30), callback);
    service.register(handle, task).unwrap();
}
```

### 3. 缓存过期

```rust,no_run
use kestrel_timer::{CallbackWrapper, TimerTask, TimerWheel};
use parking_lot::Mutex;
use std::sync::Arc;
use std::time::Duration;
use std::collections::HashMap;

# struct Cache {
#     cache: Arc<Mutex<HashMap<String, String>>>,
#     timer: TimerWheel,
# }
#
# impl Cache {
async fn set_cache(&self, key: String, value: String, ttl: Duration) {
    self.cache.lock().insert(key.clone(), value);
    
    // 分配 handle
    let handle = self.timer.allocate_handle();
    
    // 创建带回调的任务
    let cache = Arc::clone(&self.cache);
    let callback = Some(CallbackWrapper::new(move || {
        let cache = Arc::clone(&cache);
        let key = key.clone();
        async move {
            cache.lock().remove(&key);
        }
    }));
    let task = TimerTask::new_oneshot(ttl, callback);
    
    // 注册任务
    self.timer.register(handle, task).unwrap();
}
# }
```

### 4. 游戏 Buff 系统

```rust,no_run
use kestrel_timer::{TimerWheel, TimerTask, CallbackWrapper, TaskId};
use std::time::Duration;

# type BuffType = u8;
# async fn remove_buff(_player_id: u64, _buff_type: BuffType) {}

async fn apply_buff(
    timer: &TimerWheel,
    player_id: u64,
    buff_type: BuffType,
    duration: Duration
) -> TaskId {
    // 分配 handle 并获取 task_id
    let handle = timer.allocate_handle();
    let task_id = handle.task_id();
    
    // 创建并注册任务
    let callback = Some(CallbackWrapper::new(move || async move {
        remove_buff(player_id, buff_type).await;
    }));
    let task = TimerTask::new_oneshot(duration, callback);
    timer.register(handle, task).unwrap();
    
    task_id
}

# async fn extend_buff(timer: &TimerWheel, task_id: TaskId, new_duration: Duration) {
// 延长 Buff
timer.postpone(task_id, new_duration, None).unwrap();
# }
```

### 5. 重试机制

```rust,no_run
use kestrel_timer::{TimerWheel, TimerTask, CallbackWrapper};
use std::sync::Arc;
use std::time::Duration;

async fn retry_with_backoff(
    timer: &TimerWheel,
    operation: Arc<dyn Fn() + Send + Sync>,
) {
    for retry in 1..=5 {
        let delay = Duration::from_secs(2_u64.pow(retry - 1));
        
        // 分配 handle
        let handle = timer.allocate_handle();
        
        // 创建并注册任务
        let operation = Arc::clone(&operation);
        let callback = Some(CallbackWrapper::new(move || {
            let operation = Arc::clone(&operation);
            async move {
                operation();
            }
        }));
        let task = TimerTask::new_oneshot(delay, callback);
        timer.register(handle, task).unwrap();
    }
}
```

## 许可证

本项目采用 MIT 或 Apache-2.0 双许可证。

- MIT License ([LICENSE-MIT](LICENSE-MIT) 或 [opensource.org](https://opensource.org/licenses/MIT))
- Apache License 2.0 ([LICENSE-APACHE](LICENSE-APACHE) 或 [apache.org](https://www.apache.org/licenses/LICENSE-2.0))

## 致谢

时间轮算法由 George Varghese 和 Tony Lauck 在论文 ["Hashed and Hierarchical Timing Wheels"](http://www.cs.columbia.edu/~nahum/w6998/papers/sosp87-timing-wheels.pdf) (SOSP '87) 中提出。

---

**完整文档**：[docs.rs/kestrel-timer](https://docs.rs/kestrel-timer)
