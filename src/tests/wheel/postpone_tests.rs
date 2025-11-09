// Postpone functionality tests for timing wheel
//
// 时间轮延期功能测试

use crate::config::{BatchConfig, WheelConfig};
use crate::task::{CallbackWrapper, TaskId, TimerTask, TimerTaskWithCompletionNotifier};
use crate::wheel::Wheel;
use std::time::Duration;

#[test]
fn test_postpone_single_task() {
    let mut wheel = Wheel::new(WheelConfig::default(), BatchConfig::default());

    // Insert task, delay 100ms (插入任务，延迟 100毫秒)
    let callback = CallbackWrapper::new(|| async {});
    let task = TimerTask::new_oneshot(Duration::from_millis(100), Some(callback));
    let (task_with_notifier, _completion_rx) =
        TimerTaskWithCompletionNotifier::from_timer_task(task);
    let handle = wheel.allocate_handle();
    let task_id = handle.task_id();
    wheel.insert(handle, task_with_notifier);

    // Postpone task to 200ms (keep original callback) (延期任务到 200毫秒 (保留原始回调))
    let postponed = wheel.postpone(task_id, Duration::from_millis(200), None);
    assert!(postponed);

    // Verify task is still in the timing wheel (验证任务仍在时间轮中)
    assert!(!wheel.is_empty());

    // Advance 100ms (10 ticks), task should not trigger (前进 100毫秒 (10个tick)，任务不应该触发)
    for _ in 0..10 {
        let expired = wheel.advance();
        assert!(expired.is_empty());
    }

    // Advance 100ms (10 ticks), task should trigger (前进 100毫秒 (10个tick)，任务应该触发)
    let mut triggered = false;
    for _ in 0..10 {
        let expired = wheel.advance();
        if !expired.is_empty() {
            assert_eq!(expired.len(), 1);
            assert_eq!(expired[0].id, task_id);
            triggered = true;
            break;
        }
    }
    assert!(triggered);
}

#[test]
fn test_postpone_with_new_callback() {
    let mut wheel = Wheel::new(WheelConfig::default(), BatchConfig::default());

    // Insert task, with original callback (插入任务，原始回调)
    let old_callback = CallbackWrapper::new(|| async {});
    let task = TimerTask::new_oneshot(Duration::from_millis(100), Some(old_callback.clone()));
    let (task_with_notifier, _completion_rx) =
        TimerTaskWithCompletionNotifier::from_timer_task(task);
    let handle = wheel.allocate_handle();
    let task_id = handle.task_id();
    wheel.insert(handle, task_with_notifier);

    // Postpone task and replace callback (延期任务并替换回调)
    let new_callback = CallbackWrapper::new(|| async {});
    let postponed = wheel.postpone(task_id, Duration::from_millis(50), Some(new_callback));
    assert!(postponed);

    // Advance 50ms (5 ticks), task should trigger (前进 50毫秒 (5个tick)，任务应该触发)
    // 注意：任务在第 5 个 tick 触发（current_tick 从 0 推进到 5）
    let mut triggered = false;
    for i in 0..5 {
        let expired = wheel.advance();
        if !expired.is_empty() {
            assert_eq!(
                expired.len(),
                1,
                "On the {}th advance, there should be 1 task triggered",
                i + 1
            );
            assert_eq!(expired[0].id, task_id);
            triggered = true;
            break;
        }
    }
    assert!(triggered, "Task should be triggered within 5 ticks"); // 任务应该在 5 个 tick 内触发
}

#[test]
fn test_postpone_nonexistent_task() {
    let mut wheel = Wheel::new(WheelConfig::default(), BatchConfig::default());

    // Try to postpone nonexistent task (尝试延期不存在任务)
    let fake_task_id = TaskId::from_key(u64::MAX);
    let postponed = wheel.postpone(fake_task_id, Duration::from_millis(100), None);
    assert!(!postponed);
}

#[test]
fn test_postpone_batch() {
    let mut wheel = Wheel::new(WheelConfig::default(), BatchConfig::default());

    // Insert 5 tasks, delay 50ms (5 ticks) (插入 5 个任务，延迟 50毫秒 (5个tick))
    let mut task_ids = Vec::new();
    for _ in 0..5 {
        let callback = CallbackWrapper::new(|| async {});
        let task = TimerTask::new_oneshot(Duration::from_millis(50), Some(callback));
        let (task_with_notifier, _completion_rx) =
            TimerTaskWithCompletionNotifier::from_timer_task(task);
        let handle = wheel.allocate_handle();
        let task_id = handle.task_id();
        wheel.insert(handle, task_with_notifier);
        task_ids.push(task_id);
    }

    // Batch postpone all tasks to 150ms (15 ticks) (批量延期所有任务到 150毫秒 (15个tick))
    let updates: Vec<_> = task_ids
        .iter()
        .map(|&id| (id, Duration::from_millis(150)))
        .collect();
    let postponed_count = wheel.postpone_batch(updates);
    assert_eq!(postponed_count, 5);

    // Advance 5 ticks (50ms), task should not trigger (前进 50毫秒 (5个tick)，任务不应该触发)
    for _ in 0..5 {
        let expired = wheel.advance();
        assert!(
            expired.is_empty(),
            "The first 5 ticks should not have tasks triggered"
        );
    }

    // Continue advancing 10 ticks (from tick 5 to tick 15), all tasks should trigger on the 15th tick (继续前进 10个tick (从tick 5到tick 15)，所有任务应该在第 15 个 tick 触发)
    let mut total_triggered = 0;
    for _ in 0..10 {
        let expired = wheel.advance();
        total_triggered += expired.len();
    }
    assert_eq!(
        total_triggered, 5,
        "There should be 5 tasks triggered on the 15th tick"
    ); // 第 15 个 tick 应该有 5 个任务触发
}

#[test]
fn test_postpone_batch_partial() {
    let mut wheel = Wheel::new(WheelConfig::default(), BatchConfig::default());

    // Insert 10 tasks, delay 50ms (5 ticks) (插入 10 个任务，延迟 50毫秒 (5个tick))
    let mut task_ids = Vec::new();
    for _ in 0..10 {
        let callback = CallbackWrapper::new(|| async {});
        let task = TimerTask::new_oneshot(Duration::from_millis(50), Some(callback));
        let (task_with_notifier, _completion_rx) =
            TimerTaskWithCompletionNotifier::from_timer_task(task);
        let handle = wheel.allocate_handle();
        let task_id = handle.task_id();
        wheel.insert(handle, task_with_notifier);
        task_ids.push(task_id);
    }

    // Only postpone the first 5 tasks to 150ms, including a nonexistent task (只延期前 5 个任务到 150毫秒，包括一个不存在任务)
    let fake_task_id = TaskId::from_key(u64::MAX);
    let mut updates: Vec<_> = task_ids[0..5]
        .iter()
        .map(|&id| (id, Duration::from_millis(150)))
        .collect();
    updates.push((fake_task_id, Duration::from_millis(150)));

    let postponed_count = wheel.postpone_batch(updates);
    assert_eq!(
        postponed_count, 5,
        "There should be 5 tasks successfully postponed (fake_task_id failed)"
    ); // 应该有 5 个任务成功延期 (fake_task_id 失败)

    // Advance 5 ticks (50ms), the last 5 tasks that were not postponed should trigger (前进 50毫秒 (5个tick)，最后一个没有延期的任务应该触发)
    let mut triggered_at_50ms = 0;
    for _ in 0..5 {
        let expired = wheel.advance();
        triggered_at_50ms += expired.len();
    }
    assert_eq!(
        triggered_at_50ms, 5,
        "There should be 5 tasks that were not postponed triggered on the 5th tick"
    ); // 第 5 个 tick 应该有 5 个任务没有延期触发

    // Continue advancing 10 ticks (from tick 5 to tick 15), the first 5 tasks that were postponed should trigger (继续前进 10个tick (从tick 5到tick 15)，第一个 5 个任务应该触发)
    let mut triggered_at_150ms = 0;
    for _ in 0..10 {
        let expired = wheel.advance();
        triggered_at_150ms += expired.len();
    }
    assert_eq!(
        triggered_at_150ms, 5,
        "There should be 5 tasks that were postponed triggered on the 15th tick"
    ); // 第 15 个 tick 应该有 5 个任务延期触发
}

#[test]
fn test_postpone_same_task_multiple_times() {
    let mut wheel = Wheel::new(WheelConfig::default(), BatchConfig::default());

    // Insert task (插入任务)
    let callback = CallbackWrapper::new(|| async {});
    let task = TimerTask::new_oneshot(Duration::from_millis(100), Some(callback));
    let (task_with_notifier, _completion_rx) =
        TimerTaskWithCompletionNotifier::from_timer_task(task);
    let handle = wheel.allocate_handle();
    let task_id = handle.task_id();
    wheel.insert(handle, task_with_notifier);

    // First postpone (第一次延期)
    let postponed = wheel.postpone(task_id, Duration::from_millis(200), None);
    assert!(postponed, "First postpone should succeed");

    // Second postpone (第二次延期)
    let postponed = wheel.postpone(task_id, Duration::from_millis(300), None);
    assert!(postponed, "Second postpone should succeed");

    // Third postpone (第三次延期)
    let postponed = wheel.postpone(task_id, Duration::from_millis(50), None);
    assert!(postponed, "Third postpone should succeed");

    // Verify task is triggered at the last postpone time (50ms = 5 ticks) (验证任务在最后一次延期时触发 (50毫秒 = 5个tick))
    let mut triggered = false;
    for _ in 0..5 {
        let expired = wheel.advance();
        if !expired.is_empty() {
            assert_eq!(expired.len(), 1);
            assert_eq!(expired[0].id, task_id);
            triggered = true;
            break;
        }
    }
    assert!(
        triggered,
        "Task should be triggered at the last postpone time"
    ); // 任务应该在最后一次延期时触发
}

#[test]
fn test_cancel_after_postpone() {
    let mut wheel = Wheel::new(WheelConfig::default(), BatchConfig::default());

    // Insert task (插入任务)
    let callback = CallbackWrapper::new(|| async {});
    let task = TimerTask::new_oneshot(Duration::from_millis(100), Some(callback));
    let (task_with_notifier, _completion_rx) =
        TimerTaskWithCompletionNotifier::from_timer_task(task);
    let handle = wheel.allocate_handle();
    let task_id = handle.task_id();
    wheel.insert(handle, task_with_notifier);

    // Postpone task (延期任务)
    let postponed = wheel.postpone(task_id, Duration::from_millis(200), None);
    assert!(postponed, "Postpone should succeed");

    // Cancel postponed task (取消延期的任务)
    let cancelled = wheel.cancel(task_id);
    assert!(cancelled, "Cancel should succeed");

    // Advance to original time, task should not trigger (前进到原始时间，任务不应该触发)
    for _ in 0..20 {
        let expired = wheel.advance();
        assert!(expired.is_empty(), "Cancelled task should not trigger"); // 取消的任务不应该触发
    }

    assert!(wheel.is_empty(), "Wheel should be empty"); // 时间轮应该为空
}

#[test]
fn test_cross_layer_postpone() {
    let config = WheelConfig::default();

    let mut wheel = Wheel::new(config, BatchConfig::default());

    // Insert L0 task (100ms) (插入 L0 任务 (100毫秒))
    let callback = CallbackWrapper::new(|| async {});
    let task = TimerTask::new_oneshot(Duration::from_millis(100), Some(callback));
    let (task_with_notifier, _rx) = TimerTaskWithCompletionNotifier::from_timer_task(task);
    let handle = wheel.allocate_handle();
    let task_id = handle.task_id();
    wheel.insert(handle, task_with_notifier);

    // Verify in L0 layer (验证在 L0 层)
    assert_eq!(wheel.task_index.get(task_id.key()).unwrap().level, 0);

    // Postpone to 10 seconds (should migrate to L1) (延期到 10 秒 (应该迁移到 L1))
    assert!(wheel.postpone(task_id, Duration::from_secs(10), None));

    // Verify migrated to L1 layer (验证迁移到 L1 层)
    assert_eq!(wheel.task_index.get(task_id.key()).unwrap().level, 1);

    // Postpone back to 200ms (should migrate back to L0) (延期回 200毫秒 (应该迁移回 L0))
    assert!(wheel.postpone(task_id, Duration::from_millis(200), None));

    // Verify migrated back to L0 layer (验证迁移回 L0 层)
    assert_eq!(wheel.task_index.get(task_id.key()).unwrap().level, 0);
}
