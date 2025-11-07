// Advanced tests for timing wheel
// 
// 时间轮高级测试

use crate::config::{BatchConfig, WheelConfig};
use crate::task::{CallbackWrapper, TimerTask, TimerTaskWithCompletionNotifier};
use crate::wheel::Wheel;
use std::time::Duration;

#[test]
fn test_hierarchical_l1_to_l0_demotion() {
    let config = WheelConfig::builder()
        .l0_tick_duration(Duration::from_millis(10))
        .l0_slot_count(512)
        .l1_tick_duration(Duration::from_millis(100)) // L1 tick = 100ms (L1 tick = 100毫秒)
        .l1_slot_count(64)
        .build()
        .unwrap();
    
    let mut wheel = Wheel::new(config, BatchConfig::default());
    let l1_tick_ratio = wheel.l1_tick_ratio;
    assert_eq!(l1_tick_ratio, 10); // 100ms / 10ms = 10 (100毫秒 / 10毫秒 = 10)
    
    // Insert task, delay 6000ms (exceeds L0 range 5120ms) (插入任务，延迟 6000毫秒 (超过 L0 范围 5120毫秒))
    let callback = CallbackWrapper::new(|| async {});
    let task = TimerTask::new_oneshot(Duration::from_millis(6000), Some(callback));
    let (task_with_notifier, _completion_rx) = TimerTaskWithCompletionNotifier::from_timer_task(task);
    let handle = wheel.allocate_handle();
    let task_id = handle.task_id();
    wheel.insert(handle, task_with_notifier);
    
    // Verify task is in L1 layer (验证任务在 L1 层)
    let location = wheel.task_index.get(task_id.key()).unwrap();
    assert_eq!(location.level, 1);
    
    // Advance to L1 slot expiration (6000ms / 100ms = 60 L1 ticks) (前进到 L1 槽过期 (6000毫秒 / 100毫秒 = 60个L1 tick))
    // 60 L1 ticks = 600 L0 ticks (60个L1 tick = 600个L0 tick)
    let mut demoted = false;
    for i in 0..610 {
        wheel.advance();
        
        // Check if task is demoted to L0 (检查任务是否降级到 L0)
        if let Some(location) = wheel.task_index.get(task_id.key()) {
            if location.level == 0 && !demoted {
                demoted = true;
                println!("Task demoted to L0 at L0 tick {}", i); // 任务降级到 L0 在 L0 tick {i}
            }
        }
    }
    
    assert!(demoted, "Task should have been demoted from L1 to L0"); // 任务应该从 L1 降级到 L0
}

#[test]
fn test_multi_round_tasks() {
    let mut wheel = Wheel::new(WheelConfig::default(), BatchConfig::default());
    
    // Test multi-round tasks in L1 layer in hierarchical mode (在分层模式下测试 L1 层的多轮任务)
    // L1: 64 slots * 1000ms = 64000ms
    // Insert a task that exceeds one L1 circle, delay 120000ms (120 seconds) (插入一个超过一个 L1 圈的任务，延迟 120000毫秒 (120秒))
    let callback = CallbackWrapper::new(|| async {});
    let task = TimerTask::new_oneshot(Duration::from_secs(120), Some(callback));
    let (task_with_notifier, _completion_rx) = TimerTaskWithCompletionNotifier::from_timer_task(task);
    let handle = wheel.allocate_handle();
    let task_id = handle.task_id();
    wheel.insert(handle, task_with_notifier);
    
    // 120000ms / 1000ms = 120 L1 ticks (120000毫秒 / 1000毫秒 = 120个L1 tick)
    // 120 ticks / 64 slots = 1 round + 56 ticks (120个tick / 64个槽 = 1轮 + 56个tick)
    // Task should be in L1 layer, slot 56, rounds = 1 (任务应该在 L1 层，槽 56，轮数 = 1)
    
    // Verify task is in L1 layer (验证任务在 L1 层)
    let location = wheel.task_index.get(task_id.key()).unwrap();
    assert_eq!(location.level, 1);
    
    // L1 tick advances every 100 L0 ticks (L1 tick 每 100 个 L0 tick 前进一次)
    // Advance 64 * 100 = 6400 L0 ticks (complete the first round of L1) (前进 64 * 100 = 6400 L0 tick (完成 L1 的第一轮))
    for _ in 0..6400 {
        let _expired = wheel.advance();
        // During the first round of L1, the task should not be demoted or triggered (在 L1 的第一轮中，任务不应该被降级或触发)
    }
    
    // Task should still be in L1 layer, but rounds has decreased (任务应该仍在 L1 层，但轮数已经减少)
    let location = wheel.task_index.get(task_id.key());
    if let Some(loc) = location {
        assert_eq!(loc.level, 1);
    }
    
    // Continue advancing until the task is triggered (approximately another 56 * 100 = 5600 L0 ticks) (继续前进，直到任务被触发 (大约另一个 56 * 100 = 5600 L0 tick))
    let mut triggered = false;
    for _ in 0..6000 {
        let expired = wheel.advance();
        if expired.iter().any(|t| t.id == task_id) {
            triggered = true;
            break;
        }
    }
    assert!(triggered, "Task should be triggered in the second round of L1"); // 任务应该在 L1 的第二轮中触发
}

