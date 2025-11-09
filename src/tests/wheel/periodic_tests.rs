// Periodic task tests for timing wheel
//
// 时间轮周期性任务测试

use crate::config::{BatchConfig, WheelConfig};
use crate::task::{
    CallbackWrapper, CompletionReceiver, TaskCompletion, TimerTask, TimerTaskWithCompletionNotifier,
};
use crate::wheel::Wheel;
use std::time::Duration;

#[test]
fn test_periodic_task_basic() {
    let mut wheel = Wheel::new(WheelConfig::default(), BatchConfig::default());

    // Insert periodic task with 50ms interval (插入 50毫秒间隔的周期性任务)
    let callback = CallbackWrapper::new(|| async {});
    let task = TimerTask::new_periodic(
        Duration::from_millis(50), // initial delay
        Duration::from_millis(50), // interval
        Some(callback),
        None,
    );
    let (task_with_notifier, completion_receiver) =
        TimerTaskWithCompletionNotifier::from_timer_task(task);
    let handle = wheel.allocate_handle();
    let task_id = handle.task_id();
    wheel.insert(handle, task_with_notifier);

    let mut rx = match completion_receiver {
        CompletionReceiver::Periodic(receiver) => receiver,
        _ => panic!("Expected periodic completion receiver"),
    };

    // Advance 5 ticks (50ms), task should trigger for the first time
    // 前进 5 个 tick (50毫秒)，任务应该第一次触发
    for _ in 0..5 {
        wheel.advance();
    }

    // Check notification
    // 检查通知
    assert!(rx.try_recv().is_ok(), "Should receive first notification");

    // Verify task is still in the wheel (reinserted)
    // 验证任务仍在时间轮中（已重新插入）
    assert!(!wheel.is_empty(), "Periodic task should be reinserted");
    assert!(
        wheel.task_index.get(task_id.key()).is_some(),
        "Task should still be in index"
    );

    // Advance another 5 ticks (50ms), task should trigger again
    // 再前进 5 个 tick (50毫秒)，任务应该再次触发
    for _ in 0..5 {
        wheel.advance();
    }

    // Check second notification
    // 检查第二次通知
    assert!(rx.try_recv().is_ok(), "Should receive second notification");
    assert!(
        !wheel.is_empty(),
        "Periodic task should still be in the wheel"
    );

    // Cancel the periodic task
    // 取消周期性任务
    assert!(
        wheel.cancel(task_id),
        "Should be able to cancel periodic task"
    );
    assert!(wheel.is_empty(), "Wheel should be empty after cancellation");
}

#[test]
fn test_periodic_task_cancel() {
    let mut wheel = Wheel::new(WheelConfig::default(), BatchConfig::default());

    // Insert periodic task (插入周期性任务)
    let callback = CallbackWrapper::new(|| async {});
    let task = TimerTask::new_periodic(
        Duration::from_millis(100),
        Duration::from_millis(100),
        Some(callback),
        None,
    );
    let (task_with_notifier, completion_receiver) =
        TimerTaskWithCompletionNotifier::from_timer_task(task);
    let handle = wheel.allocate_handle();
    let task_id = handle.task_id();
    wheel.insert(handle, task_with_notifier);

    let mut rx = match completion_receiver {
        CompletionReceiver::Periodic(receiver) => receiver,
        _ => panic!("Expected periodic completion receiver"),
    };

    // Cancel immediately (立即取消)
    assert!(wheel.cancel(task_id), "Should successfully cancel");

    // Check cancellation notification
    // 检查取消通知
    if let Ok(reason) = rx.try_recv() {
        assert_eq!(
            reason,
            TaskCompletion::Cancelled,
            "Should receive Cancelled notification"
        );
    } else {
        panic!("Should receive cancellation notification");
    }

    // Verify wheel is empty
    // 验证时间轮为空
    assert!(wheel.is_empty(), "Wheel should be empty");

    // Advance and verify no tasks expire
    // 前进并验证没有任务过期
    for _ in 0..20 {
        let expired = wheel.advance();
        assert!(
            expired.is_empty(),
            "No tasks should expire after cancellation"
        );
    }
}

#[test]
fn test_periodic_task_multiple_triggers() {
    let mut wheel = Wheel::new(WheelConfig::default(), BatchConfig::default());

    // Insert periodic task with 30ms interval (插入 30毫秒间隔的周期性任务)
    let callback = CallbackWrapper::new(|| async {});
    let task = TimerTask::new_periodic(
        Duration::from_millis(30),
        Duration::from_millis(30),
        Some(callback),
        None,
    );
    let (task_with_notifier, completion_receiver) =
        TimerTaskWithCompletionNotifier::from_timer_task(task);
    let handle = wheel.allocate_handle();
    let task_id = handle.task_id();
    wheel.insert(handle, task_with_notifier);

    let mut rx = match completion_receiver {
        CompletionReceiver::Periodic(receiver) => receiver,
        _ => panic!("Expected periodic completion receiver"),
    };

    // Advance and count triggers (前进并计数触发次数)
    let mut trigger_count = 0;
    for _ in 0..100 {
        wheel.advance();

        // Collect all notifications
        // 收集所有通知
        while let Ok(_) = rx.try_recv() {
            trigger_count += 1;
        }
    }

    // Task should trigger approximately 100ms / 30ms = 3 times
    // 任务应该触发大约 100毫秒 / 30毫秒 = 3 次
    assert!(
        trigger_count >= 3,
        "Should trigger at least 3 times, got {}",
        trigger_count
    );

    // Cancel the task
    // 取消任务
    wheel.cancel(task_id);
}

#[test]
fn test_periodic_task_cross_layer() {
    let mut wheel = Wheel::new(WheelConfig::default(), BatchConfig::default());

    // Insert periodic task with long interval (exceeds L0 range)
    // 插入长间隔的周期性任务（超过 L0 范围）
    // L0 range: 512 slots * 10ms = 5120ms
    let callback = CallbackWrapper::new(|| async {});
    let task = TimerTask::new_periodic(
        Duration::from_secs(10), // 10000ms > 5120ms
        Duration::from_secs(10),
        Some(callback),
        None,
    );
    let (task_with_notifier, completion_receiver) =
        TimerTaskWithCompletionNotifier::from_timer_task(task);
    let handle = wheel.allocate_handle();
    let task_id = handle.task_id();
    wheel.insert(handle, task_with_notifier);

    let mut rx = match completion_receiver {
        CompletionReceiver::Periodic(receiver) => receiver,
        _ => panic!("Expected periodic completion receiver"),
    };

    // Verify task is in L1 layer
    // 验证任务在 L1 层
    let location = wheel.task_index.get(task_id.key()).unwrap();
    assert_eq!(location.level, 1, "Long interval task should be in L1");

    // Advance to trigger the task (10000ms / 10ms = 1000 ticks)
    // 前进到触发任务 (10000毫秒 / 10毫秒 = 1000个tick)
    for _ in 0..1001 {
        wheel.advance();
    }

    // Check notification
    // 检查通知
    assert!(rx.try_recv().is_ok(), "Should receive notification");

    // Verify task is reinserted (should be in L1 again)
    // 验证任务已重新插入（应该再次在 L1 中）
    assert!(
        wheel.task_index.get(task_id.key()).is_some(),
        "Task should be reinserted"
    );
    let location = wheel.task_index.get(task_id.key()).unwrap();
    assert_eq!(location.level, 1, "Reinserted task should still be in L1");

    // Cancel the task
    // 取消任务
    wheel.cancel(task_id);
}

#[test]
fn test_periodic_task_batch_cancel() {
    let mut wheel = Wheel::new(WheelConfig::default(), BatchConfig::default());

    // Insert multiple periodic tasks (插入多个周期性任务)
    let mut task_ids = Vec::new();
    let mut receivers = Vec::new();

    for i in 0..5 {
        let callback = CallbackWrapper::new(|| async {});
        let task = TimerTask::new_periodic(
            Duration::from_millis(100 + i * 10),
            Duration::from_millis(100),
            Some(callback),
            None,
        );
        let (task_with_notifier, completion_receiver) =
            TimerTaskWithCompletionNotifier::from_timer_task(task);
        let handle = wheel.allocate_handle();
        let task_id = handle.task_id();
        wheel.insert(handle, task_with_notifier);
        task_ids.push(task_id);

        if let CompletionReceiver::Periodic(rx) = completion_receiver {
            receivers.push(rx);
        }
    }

    // Batch cancel all tasks (批量取消所有任务)
    let cancelled_count = wheel.cancel_batch(&task_ids);
    assert_eq!(cancelled_count, 5, "Should cancel all 5 tasks");

    // Verify all receive cancellation notifications
    // 验证所有接收到取消通知
    for mut rx in receivers {
        if let Ok(reason) = rx.try_recv() {
            assert_eq!(reason, TaskCompletion::Cancelled);
        } else {
            panic!("Should receive cancellation notification");
        }
    }

    assert!(wheel.is_empty(), "Wheel should be empty");
}

#[test]
fn test_periodic_task_with_initial_delay() {
    let mut wheel = Wheel::new(WheelConfig::default(), BatchConfig::default());

    // Insert periodic task with different initial delay and interval
    // 插入具有不同初始延迟和间隔的周期性任务
    let callback = CallbackWrapper::new(|| async {});
    let task = TimerTask::new_periodic(
        Duration::from_millis(100), // initial delay 100ms
        Duration::from_millis(50),  // interval 50ms
        Some(callback),
        None,
    );
    let (task_with_notifier, completion_receiver) =
        TimerTaskWithCompletionNotifier::from_timer_task(task);
    let handle = wheel.allocate_handle();
    let task_id = handle.task_id();
    wheel.insert(handle, task_with_notifier);

    let mut rx = match completion_receiver {
        CompletionReceiver::Periodic(receiver) => receiver,
        _ => panic!("Expected periodic completion receiver"),
    };

    // Advance 10 ticks (100ms), task should trigger for the first time
    // 前进 10 个 tick (100毫秒)，任务应该第一次触发
    for _ in 0..10 {
        wheel.advance();
    }

    assert!(
        rx.try_recv().is_ok(),
        "Should receive first notification after initial delay"
    );

    // Advance another 5 ticks (50ms), task should trigger again (using interval)
    // 再前进 5 个 tick (50毫秒)，任务应该再次触发（使用间隔）
    for _ in 0..5 {
        wheel.advance();
    }

    assert!(
        rx.try_recv().is_ok(),
        "Should receive second notification after interval"
    );

    // Cancel the task
    // 取消任务
    wheel.cancel(task_id);
}

#[test]
fn test_periodic_task_postpone() {
    let mut wheel = Wheel::new(WheelConfig::default(), BatchConfig::default());

    // Insert periodic task with 50ms interval
    // 插入 50毫秒间隔的周期性任务
    let callback = CallbackWrapper::new(|| async {});
    let task = TimerTask::new_periodic(
        Duration::from_millis(50),
        Duration::from_millis(50),
        Some(callback),
        None,
    );
    let (task_with_notifier, completion_receiver) =
        TimerTaskWithCompletionNotifier::from_timer_task(task);
    let handle = wheel.allocate_handle();
    let task_id = handle.task_id();
    wheel.insert(handle, task_with_notifier);

    let mut rx = match completion_receiver {
        CompletionReceiver::Periodic(receiver) => receiver,
        _ => panic!("Expected periodic completion receiver"),
    };

    // Postpone the periodic task to 100ms
    // 延期周期任务到 100毫秒
    assert!(
        wheel.postpone(task_id, Duration::from_millis(100), None),
        "Should postpone periodic task"
    );

    // Advance 5 ticks (50ms), task should not trigger yet
    // 前进 5 个 tick (50毫秒)，任务还不应该触发
    for _ in 0..5 {
        wheel.advance();
    }
    assert!(
        rx.try_recv().is_err(),
        "Should not receive notification before postponed time"
    );

    // Advance another 5 ticks (total 100ms), task should trigger
    // 再前进 5 个 tick (总共 100毫秒)，任务应该触发
    for _ in 0..5 {
        wheel.advance();
    }
    assert!(
        rx.try_recv().is_ok(),
        "Should receive notification at postponed time"
    );

    // Verify task is reinserted and will trigger again at interval (50ms)
    // 验证任务已重新插入，并将在间隔时间 (50毫秒) 后再次触发
    for _ in 0..5 {
        wheel.advance();
    }
    assert!(
        rx.try_recv().is_ok(),
        "Should receive second notification after interval"
    );

    wheel.cancel(task_id);
}

#[test]
fn test_periodic_task_postpone_cross_layer() {
    let mut wheel = Wheel::new(WheelConfig::default(), BatchConfig::default());

    // Insert periodic task in L0 (short interval)
    // 在 L0 层插入周期性任务（短间隔）
    let callback = CallbackWrapper::new(|| async {});
    let task = TimerTask::new_periodic(
        Duration::from_millis(100),
        Duration::from_millis(100),
        Some(callback),
        None,
    );
    let (task_with_notifier, completion_receiver) =
        TimerTaskWithCompletionNotifier::from_timer_task(task);
    let handle = wheel.allocate_handle();
    let task_id = handle.task_id();
    wheel.insert(handle, task_with_notifier);

    let mut rx = match completion_receiver {
        CompletionReceiver::Periodic(receiver) => receiver,
        _ => panic!("Expected periodic completion receiver"),
    };

    // Verify task is in L0
    // 验证任务在 L0 层
    assert_eq!(wheel.task_index.get(task_id.key()).unwrap().level, 0);

    // Postpone to long delay (should migrate to L1)
    // 延期到长延迟（应该迁移到 L1）
    // L0 range: 512 slots * 10ms = 5120ms
    assert!(wheel.postpone(task_id, Duration::from_secs(10), None));

    // Verify task migrated to L1
    // 验证任务迁移到 L1
    assert_eq!(wheel.task_index.get(task_id.key()).unwrap().level, 1);

    // Postpone back to short delay (should migrate back to L0)
    // 延期回短延迟（应该迁移回 L0）
    assert!(wheel.postpone(task_id, Duration::from_millis(200), None));

    // Verify task migrated back to L0
    // 验证任务迁移回 L0
    assert_eq!(wheel.task_index.get(task_id.key()).unwrap().level, 0);

    // Advance to trigger
    // 前进到触发
    for _ in 0..20 {
        wheel.advance();
    }
    assert!(rx.try_recv().is_ok(), "Should receive notification");

    wheel.cancel(task_id);
}

#[test]
fn test_periodic_task_batch_insert() {
    let mut wheel = Wheel::new(WheelConfig::default(), BatchConfig::default());

    // Create batch of periodic tasks
    // 创建批量周期性任务
    let handles = wheel.allocate_handles(10);
    let task_ids: Vec<_> = handles.iter().map(|h| h.task_id()).collect();
    let mut tasks = Vec::new();

    for i in 0..10 {
        let callback = CallbackWrapper::new(|| async {});
        let task = TimerTask::new_periodic(
            Duration::from_millis(100 + i * 10),
            Duration::from_millis(50),
            Some(callback),
            None,
        );
        let (task_with_notifier, _rx) = TimerTaskWithCompletionNotifier::from_timer_task(task);
        tasks.push(task_with_notifier);
    }

    wheel
        .insert_batch(handles, tasks)
        .expect("insert_batch should succeed");

    assert_eq!(wheel.task_index.len(), 10, "All tasks should be in index");

    // Advance and verify tasks trigger
    // 前进并验证任务触发
    for _ in 0..200 {
        let _expired = wheel.advance();
        // Some tasks should trigger during advancement
        // 某些任务应该在推进过程中触发
    }

    // All tasks should still be in wheel (periodic tasks reinsert)
    // 所有任务应该仍在时间轮中（周期性任务会重新插入）
    assert_eq!(
        wheel.task_index.len(),
        10,
        "All periodic tasks should still be in wheel"
    );

    // Batch cancel all
    // 批量取消所有
    let cancelled = wheel.cancel_batch(&task_ids);
    assert_eq!(cancelled, 10, "Should cancel all periodic tasks");
    assert!(wheel.is_empty());
}

#[test]
fn test_periodic_task_batch_postpone() {
    let mut wheel = Wheel::new(WheelConfig::default(), BatchConfig::default());

    // Insert multiple periodic tasks
    // 插入多个周期性任务
    let mut task_ids = Vec::new();
    for _ in 0..5 {
        let callback = CallbackWrapper::new(|| async {});
        let task = TimerTask::new_periodic(
            Duration::from_millis(50),
            Duration::from_millis(50),
            Some(callback),
            None,
        );
        let (task_with_notifier, _rx) = TimerTaskWithCompletionNotifier::from_timer_task(task);
        let handle = wheel.allocate_handle();
        let task_id = handle.task_id();
        wheel.insert(handle, task_with_notifier);
        task_ids.push(task_id);
    }

    // Batch postpone all tasks to 150ms
    // 批量延期所有任务到 150毫秒
    let updates: Vec<_> = task_ids
        .iter()
        .map(|&id| (id, Duration::from_millis(150)))
        .collect();
    let postponed_count = wheel.postpone_batch(updates);
    assert_eq!(postponed_count, 5, "Should postpone all 5 periodic tasks");

    // Advance 5 ticks (50ms), tasks should not trigger
    // 前进 5 个 tick (50毫秒)，任务不应该触发
    for _ in 0..5 {
        let expired = wheel.advance();
        assert!(
            expired.is_empty(),
            "Tasks should not trigger before postponed time"
        );
    }

    // Advance to 15 ticks (150ms), all tasks should trigger
    // 前进到 15 个 tick (150毫秒)，所有任务应该触发
    let mut total_triggered = 0;
    for _ in 0..10 {
        let expired = wheel.advance();
        total_triggered += expired.len();
    }
    assert_eq!(
        total_triggered, 5,
        "All 5 tasks should trigger at postponed time"
    );

    // Clean up
    // 清理
    wheel.cancel_batch(&task_ids);
}

#[test]
fn test_mixed_oneshot_and_periodic_tasks() {
    let mut wheel = Wheel::new(WheelConfig::default(), BatchConfig::default());

    // Insert oneshot tasks
    // 插入一次性任务
    let mut oneshot_ids = Vec::new();
    for i in 0..5 {
        let callback = CallbackWrapper::new(|| async {});
        let task = TimerTask::new_oneshot(Duration::from_millis(100 + i * 10), Some(callback));
        let (task_with_notifier, _rx) = TimerTaskWithCompletionNotifier::from_timer_task(task);
        let handle = wheel.allocate_handle();
        let task_id = handle.task_id();
        wheel.insert(handle, task_with_notifier);
        oneshot_ids.push(task_id);
    }

    // Insert periodic tasks
    // 插入周期性任务
    let mut periodic_ids = Vec::new();
    let mut periodic_receivers = Vec::new();
    for _ in 0..5 {
        let callback = CallbackWrapper::new(|| async {});
        let task = TimerTask::new_periodic(
            Duration::from_millis(100),
            Duration::from_millis(100),
            Some(callback),
            None,
        );
        let (task_with_notifier, completion_receiver) =
            TimerTaskWithCompletionNotifier::from_timer_task(task);
        let handle = wheel.allocate_handle();
        let task_id = handle.task_id();
        wheel.insert(handle, task_with_notifier);
        periodic_ids.push(task_id);

        if let CompletionReceiver::Periodic(rx) = completion_receiver {
            periodic_receivers.push(rx);
        }
    }

    assert_eq!(wheel.task_index.len(), 10, "Should have 10 tasks total");

    // Advance 15 ticks (150ms)
    // 前进 15 个 tick (150毫秒)
    let mut total_expired = 0;
    for _ in 0..15 {
        let expired = wheel.advance();
        total_expired += expired.len();
    }

    // All oneshot tasks (5) + first trigger of periodic tasks (5) = 10
    // 所有一次性任务 (5) + 周期性任务的第一次触发 (5) = 10
    assert_eq!(
        total_expired, 10,
        "Should have triggered oneshot and periodic tasks"
    );

    // Oneshot tasks should be removed, periodic tasks should remain
    // 一次性任务应该被移除，周期性任务应该保留
    assert_eq!(
        wheel.task_index.len(),
        5,
        "Only periodic tasks should remain"
    );

    // Verify all oneshot tasks are removed
    // 验证所有一次性任务已移除
    for id in &oneshot_ids {
        assert!(
            wheel.task_index.get(id.key()).is_none(),
            "Oneshot task should be removed"
        );
    }

    // Verify all periodic tasks are still present
    // 验证所有周期性任务仍然存在
    for id in &periodic_ids {
        assert!(
            wheel.task_index.get(id.key()).is_some(),
            "Periodic task should still be present"
        );
    }

    // Clean up periodic tasks
    // 清理周期性任务
    wheel.cancel_batch(&periodic_ids);
    assert!(wheel.is_empty());
}

#[test]
fn test_periodic_task_postpone_with_callback() {
    let mut wheel = Wheel::new(WheelConfig::default(), BatchConfig::default());

    // Insert periodic task
    // 插入周期性任务
    let old_callback = CallbackWrapper::new(|| async {});
    let task = TimerTask::new_periodic(
        Duration::from_millis(50),
        Duration::from_millis(50),
        Some(old_callback),
        None,
    );
    let (task_with_notifier, completion_receiver) =
        TimerTaskWithCompletionNotifier::from_timer_task(task);
    let handle = wheel.allocate_handle();
    let task_id = handle.task_id();
    wheel.insert(handle, task_with_notifier);

    let mut rx = match completion_receiver {
        CompletionReceiver::Periodic(receiver) => receiver,
        _ => panic!("Expected periodic completion receiver"),
    };

    // Postpone with new callback
    // 使用新回调延期
    let new_callback = CallbackWrapper::new(|| async {});
    assert!(wheel.postpone(task_id, Duration::from_millis(100), Some(new_callback)));

    // Advance to trigger
    // 前进到触发
    for _ in 0..10 {
        wheel.advance();
    }

    assert!(
        rx.try_recv().is_ok(),
        "Should receive notification with new callback"
    );

    wheel.cancel(task_id);
}
