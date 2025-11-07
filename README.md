# Kestrel Timer

> High-performance async timer library based on Hierarchical Timing Wheel algorithm

[![Rust](https://img.shields.io/badge/rust-1.70%2B-orange.svg)](https://www.rust-lang.org/)
[![Tokio](https://img.shields.io/badge/tokio-1.48-blue.svg)](https://tokio.rs/)
[![Crates.io](https://img.shields.io/crates/v/kestrel-timer.svg)](https://crates.io/crates/kestrel-timer)
[![Documentation](https://docs.rs/kestrel-timer/badge.svg)](https://docs.rs/kestrel-timer)
[![License](https://img.shields.io/crates/l/kestrel-timer.svg)](https://github.com/ShaoG-R/kestrel-timer#license)

[中文文档](README_CN.md)

## 📚 Table of Contents

- [Overview](#overview)
- [Key Features](#key-features)
- [Quick Start](#quick-start)
- [Architecture](#architecture)
- [Usage Examples](#usage-examples)
- [Configuration](#configuration)
- [Benchmarks](#benchmarks)
- [Use Cases](#use-cases)
- [License](#license)

## Overview

`kestrel-timer` is a high-performance async timer library based on the Hierarchical Timing Wheel algorithm, designed for Rust and Tokio runtime. It provides O(1) time complexity for timer operations and easily handles 10,000+ concurrent timers.

**Core Advantages**:
- Hierarchical timing wheel architecture that automatically separates short-delay and long-delay tasks
- 2-12x performance improvement over traditional heap-based implementations
- Support for timer postponement, batch operations, and completion notifications
- Production-ready with comprehensive testing

## Key Features

### 🏗️ Hierarchical Timing Wheel

- **Dual-layer design**: L0 layer (high-precision short-delay) + L1 layer (long-delay) with automatic layering
- **Smart demotion**: L1 tasks automatically demote to L0 layer when due
- **No round checks**: L0 layer eliminates rounds checking for 90% of tasks

### ⚡ High Performance

- **O(1) time complexity**: Insert, delete, and trigger operations are all O(1)
- **Optimized data structures**: 
  - `DeferredMap` (generational arena) for task indexing with O(1) operations
  - `parking_lot::Mutex` for efficient locking
- **Bitwise optimization**: Slot count is power of 2 for fast modulo operations
- **Large-scale support**: Easily handles 10,000+ concurrent timers

### 🔄 Complete Features

- ✅ Async callback support (based on Tokio)
- ✅ Timer postponement (keep or replace callback)
- ✅ Batch operations (schedule, cancel, postpone)
- ✅ Completion notification mechanism
- ✅ TimerService (Actor-based management)
- ✅ Thread-safe

## Quick Start

### Installation

Add to your `Cargo.toml`:

```toml
[dependencies]
kestrel-timer = "0.2.0"
tokio = { version = "1.48", features = ["full"] }
```

### Basic Usage

```rust
use kestrel_timer::{TimerWheel, CallbackWrapper, TimerTask};
use std::time::Duration;

#[tokio::main]
async fn main() {
    // Create timer with default config
    let timer = TimerWheel::with_defaults();
    
    // Step 1: Allocate handle
    let handle = timer.allocate_handle();
    let task_id = handle.task_id();
    
    // Step 2: Create task
    let callback = Some(CallbackWrapper::new(|| async {
        println!("Timer fired!");
    }));
    let task = TimerTask::new_oneshot(Duration::from_secs(1), callback);
    
    // Step 3: Register task
    let timer_handle = timer.register(handle, task).unwrap();
    
    // Wait for completion or cancel
    // timer_handle.cancel();
}
```

### Batch Operations

```rust
use kestrel_timer::{TimerWheel, CallbackWrapper, TimerTask};
use std::time::Duration;

let timer = TimerWheel::with_defaults();

// Step 1: Batch allocate handles
let handles = timer.allocate_handles(100);
let task_ids: Vec<_> = handles.iter().map(|h| h.task_id()).collect();

// Step 2: Create tasks
let tasks: Vec<_> = (0..100)
    .map(|i| {
        let delay = Duration::from_millis(100 + i * 10);
        let callback = Some(CallbackWrapper::new(move || async move {
            println!("Timer {} fired", i);
        }));
        TimerTask::new_oneshot(delay, callback)
    })
    .collect();

// Step 3: Batch register
let batch_handle = timer.register_batch(handles, tasks).unwrap();

// Batch cancel
batch_handle.cancel_all();
```

### Postpone Timer

```rust
use kestrel_timer::{TimerWheel, CallbackWrapper, TimerTask};
use std::time::Duration;

let timer = TimerWheel::with_defaults();

// Step 1: Allocate handle and get task_id
let handle = timer.allocate_handle();
let task_id = handle.task_id();

// Step 2: Create and register task
let callback = Some(CallbackWrapper::new(|| async {
    println!("Original callback");
}));
let task = TimerTask::new_oneshot(Duration::from_millis(50), callback);
let timer_handle = timer.register(handle, task).unwrap();

// Postpone and keep original callback
timer.postpone(task_id, Duration::from_millis(150), None);

// Postpone and replace callback
let new_callback = Some(CallbackWrapper::new(|| async {
    println!("New callback");
}));
timer.postpone(task_id, Duration::from_millis(200), new_callback);
```

### TimerService Usage

```rust
use kestrel_timer::{TimerWheel, TimerService, TimerTask, CallbackWrapper, TaskNotification};
use kestrel_timer::config::ServiceConfig;
use std::time::Duration;

let timer = TimerWheel::with_defaults();
let mut service = timer.create_service(ServiceConfig::default());

// Step 1: Allocate handles
let handles = service.allocate_handles(2);

// Step 2: Create tasks
let tasks: Vec<_> = vec![
    TimerTask::new_oneshot(Duration::from_millis(100), Some(CallbackWrapper::new(|| async {}))),
    TimerTask::new_oneshot(Duration::from_millis(200), Some(CallbackWrapper::new(|| async {}))),
];

// Step 3: Batch register
service.register_batch(handles, tasks).unwrap();

// Receive timeout notifications
let mut timeout_rx = service.take_receiver().unwrap();
while let Some(notification) = timeout_rx.recv().await {
    match notification {
        TaskNotification::OneShot(task_id) => {
            println!("One-shot task {:?} completed", task_id);
        },
        TaskNotification::Periodic(task_id) => {
            println!("Periodic task {:?} called", task_id);
        },
    }
}

service.shutdown().await;
```

## Architecture

### Hierarchical Timing Wheel Design

```
┌─────────────────────────────────────────────┐
│              L1 Layer (Upper)               │
│  Slots: 64 | Tick: 1s | Range: 64s          │
│              ↓ Demote to L0                 │
└─────────────────────────────────────────────┘
                    ↓
┌─────────────────────────────────────────────┐
│              L0 Layer (Lower)               │
│  Slots: 512 | Tick: 10ms | Range: 5.12s     │
│              ▲ Current Pointer              │
└─────────────────────────────────────────────┘
```

**L0 Layer (Lower - High Precision)**:
- Slots: 512 (default), Tick: 10ms
- Coverage: 5.12 seconds
- Handles 80-90% of short-delay tasks

**L1 Layer (Upper - Long Duration)**:
- Slots: 64 (default), Tick: 1000ms
- Coverage: 64 seconds
- Handles long-delay tasks with rounds mechanism

**Workflow**:
1. Short delay (< 5.12s) → Insert directly into L0 layer
2. Long delay (≥ 5.12s) → Insert into L1 layer
3. L1 task due → Automatically demote to L0 layer
4. L0 task due → Trigger immediately

### Task Indexing with DeferredMap

Uses `DeferredMap` (a generational arena) for efficient task management:

- **Two-Step Registration**: 
  - Allocate handle to get task ID (cheap, no value needed)
  - Insert task using the handle (with completion notifiers)

- **Generational Safety**: Each task ID includes:
  - Lower 32 bits: Slot index
  - Upper 32 bits: Generation counter
  - Prevents use-after-free and ABA problems

- **Memory Efficiency**: Union-based slot storage
  - Occupied slots: Store task data
  - Vacant slots: Store free-list pointer

### Performance Optimizations

- **Hierarchical architecture**: Avoids single-layer round checks, L0 layer requires no rounds checking
- **DeferredMap**: O(1) task lookup, insertion, and removal with generational safety
- **Efficient locking**: `parking_lot::Mutex` is faster than standard Mutex
- **Bitwise optimization**: Slot count is power of 2, uses `& (n-1)` for fast modulo
- **Cache optimization**: Pre-compute slot masks, tick durations, and other frequently used values
- **Batch optimization**: Reduce lock contention with smart small-batch handling

## Usage Examples

Full API documentation at [docs.rs/kestrel-timer](https://docs.rs/kestrel-timer)

### Main APIs

**TimerTask**:
- `TimerTask::new_oneshot(delay, callback)` - Create one-shot task
- `TimerTask::new_periodic(initial_delay, interval, callback, buffer_size)` - Create periodic task
- `get_task_type()` - Get task type
- `get_interval()` - Get interval for periodic tasks

**TaskHandle** (Pre-allocated handle):
- `task_id()` - Get task ID from handle

**TimerWheel**:
- `TimerWheel::with_defaults()` - Create with default config
- `TimerWheel::new(config)` - Create with custom config
- `allocate_handle()` - Allocate single handle
- `allocate_handles(count)` - Batch allocate handles
- `register(handle, task)` - Register task with handle
- `register_batch(handles, tasks)` - Batch register tasks
- `cancel(task_id)` - Cancel task
- `cancel_batch(task_ids)` - Batch cancel
- `postpone(task_id, delay, callback)` - Postpone task
- `postpone_batch(updates)` - Batch postpone

**TimerHandle** (Returned after registration):
- `cancel()` - Cancel timer
- `task_id()` - Get task ID

**TimerService**:
- `allocate_handle()` - Allocate single handle
- `allocate_handles(count)` - Batch allocate handles
- `register(handle, task)` - Register task with handle
- `register_batch(handles, tasks)` - Batch register tasks
- `take_receiver()` - Get timeout notification receiver
- `cancel_task(task_id)` - Cancel task
- `cancel_batch(task_ids)` - Batch cancel
- `postpone(task_id, delay, callback)` - Postpone task
- `postpone_batch(updates)` - Batch postpone
- `shutdown()` - Shutdown service

## Configuration

### Default Configuration

```rust
let timer = TimerWheel::with_defaults();
// L0: 512 slots × 10ms = 5.12 seconds
// L1: 64 slots × 1s = 64 seconds
```

### Custom Configuration

```rust
use kestrel_timer::WheelConfig;

let config = WheelConfig::builder()
    .l0_tick_duration(Duration::from_millis(10))  // L0 tick
    .l0_slot_count(512)                            // L0 slots (must be power of 2)
    .l1_tick_duration(Duration::from_secs(1))      // L1 tick
    .l1_slot_count(64)                             // L1 slots (must be power of 2)
    .build()?;
let timer = TimerWheel::new(config);
```

### Recommended Configurations

**High Precision (Network Timeouts)**:
```rust
let config = WheelConfig::builder()
    .l0_tick_duration(Duration::from_millis(5))
    .l0_slot_count(1024)
    .l1_tick_duration(Duration::from_millis(500))
    .l1_slot_count(64)
    .build()?;
```

**Low Precision (Heartbeat Detection)**:
```rust
let config = WheelConfig::builder()
    .l0_tick_duration(Duration::from_millis(100))
    .l0_slot_count(512)
    .l1_tick_duration(Duration::from_secs(10))
    .l1_slot_count(128)
    .build()?;
```

## Benchmarks

### Run Benchmarks

```bash
cargo bench
```

### Performance Comparison

Compared to traditional heap-based (BinaryHeap) timer implementations:

| Operation | Hierarchical Wheel | Heap Implementation | Advantage |
|-----------|-------------------|---------------------|-----------|
| Insert Single | O(1) ~5μs | O(log n) ~10-20μs | 2-4x faster |
| Batch Insert 1000 | ~2ms | ~15-25ms | 7-12x faster |
| Cancel Task | O(1) ~2μs | O(n) ~50-100μs | 25-50x faster |
| Postpone Task | O(1) ~4μs | O(log n) ~15-30μs | 4-7x faster |

### Large Scale Testing

```bash
cargo test --test integration_test test_large_scale_timers
```

- ✅ 10,000 concurrent timers
- ✅ Creation time < 100ms
- ✅ All timers fire correctly

## Use Cases

### 1. Network Timeout Management

```rust
use kestrel_timer::{TimerWheel, CallbackWrapper, TimerTask};
use std::time::Duration;

async fn handle_connection(timer: &TimerWheel, conn_id: u64) {
    // Allocate handle first
    let handle = timer.allocate_handle();
    
    // Create task
    let callback = Some(CallbackWrapper::new(move || async move {
        println!("Connection {} timed out", conn_id);
        close_connection(conn_id).await;
    }));
    let task = TimerTask::new_oneshot(Duration::from_secs(30), callback);
    
    // Register task
    let timer_handle = timer.register(handle, task).unwrap();
    
    // Cancel timeout when connection completes
    // timer_handle.cancel();
}
```

### 2. Heartbeat Detection

```rust
use kestrel_timer::{TimerWheel, TimerTask, CallbackWrapper};
use kestrel_timer::config::ServiceConfig;
use std::time::Duration;

let timer = TimerWheel::with_defaults();
let mut service = timer.create_service(ServiceConfig::default());

for client_id in client_ids {
    // Allocate handle
    let handle = service.allocate_handle();
    
    // Create and register task
    let callback = Some(CallbackWrapper::new(move || async move {
        println!("Client {} heartbeat timeout", client_id);
    }));
    let task = TimerTask::new_oneshot(Duration::from_secs(30), callback);
    service.register(handle, task).unwrap();
}
```

### 3. Cache Expiration

```rust
use kestrel_timer::{TimerTask, CallbackWrapper};
use std::sync::Arc;
use std::time::Duration;

async fn set_cache(&self, key: String, value: String, ttl: Duration) {
    self.cache.lock().insert(key.clone(), value);
    
    // Allocate handle
    let handle = self.timer.allocate_handle();
    
    // Create task with callback
    let cache = Arc::clone(&self.cache);
    let callback = Some(CallbackWrapper::new(move || {
        let cache = Arc::clone(&cache);
        let key = key.clone();
        async move {
            cache.lock().remove(&key);
        }
    }));
    let task = TimerTask::new_oneshot(ttl, callback);
    
    // Register task
    self.timer.register(handle, task).unwrap();
}
```

### 4. Game Buff System

```rust
use kestrel_timer::{TimerWheel, TimerTask, CallbackWrapper, TaskId};
use std::time::Duration;

async fn apply_buff(
    timer: &TimerWheel,
    player_id: u64,
    buff_type: BuffType,
    duration: Duration
) -> TaskId {
    // Allocate handle and get task_id
    let handle = timer.allocate_handle();
    let task_id = handle.task_id();
    
    // Create and register task
    let callback = Some(CallbackWrapper::new(move || async move {
        remove_buff(player_id, buff_type).await;
    }));
    let task = TimerTask::new_oneshot(duration, callback);
    timer.register(handle, task).unwrap();
    
    task_id
}

// Extend buff duration
timer.postpone(task_id, new_duration, None);
```

### 5. Retry Mechanism

```rust
use kestrel_timer::{TimerWheel, TimerTask, CallbackWrapper};
use std::time::Duration;

async fn retry_with_backoff(timer: &TimerWheel, operation: impl Fn()) {
    for retry in 1..=5 {
        let delay = Duration::from_secs(2_u64.pow(retry - 1));
        
        // Allocate handle
        let handle = timer.allocate_handle();
        
        // Create and register task
        let callback = Some(CallbackWrapper::new(move || async move {
            operation().await;
        }));
        let task = TimerTask::new_oneshot(delay, callback);
        timer.register(handle, task).unwrap();
    }
}
```

## License

This project is licensed under either of:

- MIT License ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)
- Apache License 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)

## Acknowledgments

The timing wheel algorithm was first proposed by George Varghese and Tony Lauck in the paper ["Hashed and Hierarchical Timing Wheels"](http://www.cs.columbia.edu/~nahum/w6998/papers/sosp87-timing-wheels.pdf) (SOSP '87).

---

**Full Documentation**: [docs.rs/kestrel-timer](https://docs.rs/kestrel-timer)

