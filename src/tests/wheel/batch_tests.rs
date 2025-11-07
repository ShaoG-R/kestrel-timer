// Batch operations tests for timing wheel
// 
// 时间轮批量操作测试

use crate::config::{BatchConfig, WheelConfig};
use crate::task::{CallbackWrapper, TimerTask, TimerTaskWithCompletionNotifier};
use crate::wheel::Wheel;
use std::time::Duration;

#[test]
fn test_insert_batch() {
    let mut wheel = Wheel::new(WheelConfig::default(), BatchConfig::default());
    
    // Create batch tasks (创建批量任务)
    let handles = wheel.allocate_handles(10);
    let mut tasks = Vec::new();
    
    for i in 0..10 {
        let callback = CallbackWrapper::new(|| async {});
        let task = TimerTask::new_oneshot(Duration::from_millis(100 + i * 10), Some(callback));
        let (task_with_notifier, _completion_rx) = TimerTaskWithCompletionNotifier::from_timer_task(task);
        tasks.push(task_with_notifier);
    }
    
    wheel.insert_batch(handles, tasks).expect("insert_batch should succeed");
    assert!(!wheel.is_empty());
}

#[test]
fn test_cancel_batch() {
    let mut wheel = Wheel::new(WheelConfig::default(), BatchConfig::default());
    
    // Insert multiple tasks
    let mut task_ids = Vec::new();
    for i in 0..10 {
        let callback = CallbackWrapper::new(|| async {});
        let task = TimerTask::new_oneshot(Duration::from_millis(100 + i * 10), Some(callback));
        let (task_with_notifier, _completion_rx) = TimerTaskWithCompletionNotifier::from_timer_task(task);
        let handle = wheel.allocate_handle();
        let task_id = handle.task_id();
        wheel.insert(handle, task_with_notifier);
        task_ids.push(task_id);
    }
    
    assert_eq!(task_ids.len(), 10);
    
    // Batch cancel first 5 tasks
    let to_cancel = &task_ids[0..5];
    let cancelled_count = wheel.cancel_batch(to_cancel);
    
    assert_eq!(cancelled_count, 5);
    
    // Try to cancel the same tasks again, should return 0
    let cancelled_again = wheel.cancel_batch(to_cancel);
    assert_eq!(cancelled_again, 0);
    
    // Cancel remaining tasks
    let remaining = &task_ids[5..10];
    let cancelled_remaining = wheel.cancel_batch(remaining);
    assert_eq!(cancelled_remaining, 5);
    
    assert!(wheel.is_empty());
}

#[test]
fn test_batch_operations_same_slot() {
    let mut wheel = Wheel::new(WheelConfig::default(), BatchConfig::default());
    
    // Insert multiple tasks with the same delay (will enter the same slot) (插入多个任务，延迟相同，将进入同一个槽)
    let mut task_ids = Vec::new();
    for _ in 0..20 {
        let callback = CallbackWrapper::new(|| async {});
        let task = TimerTask::new_oneshot(Duration::from_millis(100), Some(callback));
        let (task_with_notifier, _completion_rx) = TimerTaskWithCompletionNotifier::from_timer_task(task);
        let handle = wheel.allocate_handle();
        let task_id = handle.task_id();
        wheel.insert(handle, task_with_notifier);
        task_ids.push(task_id);
    }
    
    // Batch cancel all tasks (批量取消所有任务)
    let cancelled_count = wheel.cancel_batch(&task_ids);
    assert_eq!(cancelled_count, 20);
    assert!(wheel.is_empty());
}

#[test]
fn test_batch_cancel_small_threshold() {
    // Test small batch threshold optimization (测试小批量阈值优化)
    let batch_config = BatchConfig {
        small_batch_threshold: 5,
    };
    let mut wheel = Wheel::new(WheelConfig::default(), batch_config);
    
    // Insert 10 tasks (插入 10 个任务)
    let mut task_ids = Vec::new();
    for _ in 0..10 {
        let callback = CallbackWrapper::new(|| async {});
        let task = TimerTask::new_oneshot(Duration::from_millis(100), Some(callback));
        let (task_with_notifier, _rx) = TimerTaskWithCompletionNotifier::from_timer_task(task);
        let handle = wheel.allocate_handle();
        let task_id = handle.task_id();
        wheel.insert(handle, task_with_notifier);
        task_ids.push(task_id);
    }
    
    // Small batch cancel (should use direct cancel path) (小批量取消 (应该使用直接取消路径))
    let cancelled = wheel.cancel_batch(&task_ids[0..3]);
    assert_eq!(cancelled, 3);
    
    // Large batch cancel (should use grouped optimization path) (大批量取消 (应该使用分组优化路径))
    let cancelled = wheel.cancel_batch(&task_ids[3..10]);
    assert_eq!(cancelled, 7);
    
    assert!(wheel.is_empty()); // 时间轮应该为空
}

#[test]
fn test_empty_batch_operations() {
    let mut wheel = Wheel::new(WheelConfig::default(), BatchConfig::default());
    
    // Test empty batch insert (测试空批量插入)
    wheel.insert_batch(vec![], vec![]).expect("empty insert_batch should succeed");
    
    // Test empty batch cancel (测试空批量取消)
    let cancelled = wheel.cancel_batch(&[]);
    assert_eq!(cancelled, 0);
    
    // Test empty batch postpone (测试空批量延期)
    let postponed = wheel.postpone_batch(vec![]);
    assert_eq!(postponed, 0);
}

#[test]
fn test_insert_batch_length_mismatch() {
    let mut wheel = Wheel::new(WheelConfig::default(), BatchConfig::default());
    
    // Create 3 handles but only 2 tasks (创建 3 个 handles 但只有 2 个 tasks)
    let mut handles = Vec::new();
    let mut tasks = Vec::new();
    
    for i in 0..3 {
        let handle = wheel.allocate_handle();
        handles.push(handle);
        
        if i < 2 {
            let callback = CallbackWrapper::new(|| async {});
            let task = TimerTask::new_oneshot(Duration::from_millis(100), Some(callback));
            let (task_with_notifier, _rx) = TimerTaskWithCompletionNotifier::from_timer_task(task);
            tasks.push(task_with_notifier);
        }
    }
    
    // Should return error due to length mismatch (应该由于长度不匹配返回错误)
    let result = wheel.insert_batch(handles, tasks);
    assert!(result.is_err());
    
    if let Err(crate::error::TimerError::BatchLengthMismatch { handles_len, tasks_len }) = result {
        assert_eq!(handles_len, 3);
        assert_eq!(tasks_len, 2);
    } else {
        panic!("Expected BatchLengthMismatch error");
    }
}

