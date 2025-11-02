use crate::{TaskCompletionReasonPeriodic, TimerTask};
use crate::config::{BatchConfig, WheelConfig};
use crate::task::{CompletionNotifier, TaskCompletionReasonOneShot, TaskId, TaskLocation, TaskTypeWithCompletionNotifier, TimerTaskWithCompletionNotifier};
use rustc_hash::FxHashMap;
use std::time::Duration;

/// Timing wheel single layer data structure
/// 
/// 时间轮单层数据结构
struct WheelLayer {
    /// Slot array, each slot stores a group of timer tasks
    /// 
    /// 槽数组，每个槽存储一组定时器任务
    slots: Vec<Vec<TimerTaskWithCompletionNotifier>>,
    
    /// Current time pointer (tick index)
    /// 
    /// 当前时间指针（tick 索引）
    current_tick: u64,
    
    /// Slot count
    /// 
    /// 槽数量
    slot_count: usize,
    
    /// Duration of each tick
    /// 
    /// 每个 tick 的持续时间
    tick_duration: Duration,
    
    /// Cache: tick duration in milliseconds (u64) - avoid repeated conversion
    /// 
    /// 缓存：tick 持续时间（毫秒，u64）- 避免重复转换
    tick_duration_ms: u64,
    
    /// Cache: slot mask (slot_count - 1) - for fast modulo operation
    /// 
    /// 缓存：槽掩码（slot_count - 1）- 用于快速取模运算
    slot_mask: usize,
}

impl WheelLayer {
    /// Create a new wheel layer
    /// 
    /// 创建一个新的时间轮层
    fn new(slot_count: usize, tick_duration: Duration) -> Self {
        let mut slots = Vec::with_capacity(slot_count);
        // Pre-allocate capacity for each slot to reduce subsequent reallocation during push
        // Most slots typically contain 0-4 tasks, pre-allocate capacity of 4
        // 
        // 为每个槽预分配容量以减少后续 push 时的重新分配
        // 大多数槽通常包含 0-4 个任务，预分配容量为 4
        for _ in 0..slot_count {
            slots.push(Vec::with_capacity(4));
        }
        
        let tick_duration_ms = tick_duration.as_millis() as u64;
        let slot_mask = slot_count - 1;
        
        Self {
            slots,
            current_tick: 0,
            slot_count,
            tick_duration,
            tick_duration_ms,
            slot_mask,
        }
    }
    
    /// Calculate the number of ticks corresponding to the delay
    /// 
    /// 计算延迟对应的 tick 数量
    fn delay_to_ticks(&self, delay: Duration) -> u64 {
        let ticks = delay.as_millis() as u64 / self.tick_duration.as_millis() as u64;
        ticks.max(1) // at least 1 tick (至少 1 个 tick)
    }
}

/// Timing wheel data structure (hierarchical mode)
/// 
/// 时间轮数据结构（分层模式）
pub struct Wheel {
    /// L0 layer (bottom layer)
    /// 
    /// L0 层（底层）
    l0: WheelLayer,
    
    /// L1 layer (top layer)
    /// 
    /// L1 层（顶层）
    l1: WheelLayer,
    
    /// L1 tick ratio relative to L0 tick
    /// 
    /// L1 tick 相对于 L0 tick 的比率
    l1_tick_ratio: u64,
    
    /// Task index for fast lookup and cancellation
    /// 
    /// 任务索引，用于快速查找和取消
    task_index: FxHashMap<TaskId, TaskLocation>,
    
    /// Batch processing configuration
    /// 
    /// 批处理配置
    batch_config: BatchConfig,
    
    /// Cache: L0 layer capacity in milliseconds - avoid repeated calculation
    /// 
    /// 缓存：L0 层容量（毫秒）- 避免重复计算
    l0_capacity_ms: u64,
    
    /// Cache: L1 layer capacity in ticks - avoid repeated calculation
    /// 
    /// 缓存：L1 层容量（tick 数）- 避免重复计算
    l1_capacity_ticks: u64,
}

impl Wheel {
    /// Create new timing wheel
    ///
    /// # Parameters
    /// - `config`: Timing wheel configuration (already validated)
    /// - `batch_config`: Batch processing configuration
    ///
    /// # Notes
    /// Configuration parameters have been validated in WheelConfig::builder().build(), so this method will not fail.
    /// 
    /// 创建新的时间轮
    ///
    /// # 参数
    /// - `config`: 时间轮配置（已验证）
    /// - `batch_config`: 批处理配置
    ///
    /// # 注意
    /// 配置参数已在 WheelConfig::builder().build() 中验证，因此此方法不会失败。
    pub fn new(config: WheelConfig, batch_config: BatchConfig) -> Self {
        let l0 = WheelLayer::new(config.l0_slot_count, config.l0_tick_duration);
        let l1 = WheelLayer::new(config.l1_slot_count, config.l1_tick_duration);
        
        // Calculate L1 tick ratio relative to L0 tick
        // 计算 L1 tick 相对于 L0 tick 的比率
        let l1_tick_ratio = l1.tick_duration_ms / l0.tick_duration_ms;
        
        // Pre-calculate capacity to avoid repeated calculation in insert
        // 预计算容量，避免在 insert 中重复计算
        let l0_capacity_ms = (l0.slot_count as u64) * l0.tick_duration_ms;
        let l1_capacity_ticks = l1.slot_count as u64;
        
        Self {
            l0,
            l1,
            l1_tick_ratio,
            task_index: FxHashMap::default(),
            batch_config,
            l0_capacity_ms,
            l1_capacity_ticks,
        }
    }

    /// Get current tick (L0 layer tick)
    /// 
    /// 获取当前 tick（L0 层 tick）
    #[allow(dead_code)]
    pub fn current_tick(&self) -> u64 {
        self.l0.current_tick
    }

    /// Get tick duration (L0 layer tick duration)
    /// 
    /// 获取 tick 持续时间（L0 层 tick 持续时间）
    #[allow(dead_code)]
    pub fn tick_duration(&self) -> Duration {
        self.l0.tick_duration
    }

    /// Get slot count (L0 layer slot count)
    /// 
    /// 获取槽数量（L0 层槽数量）
    #[allow(dead_code)]
    pub fn slot_count(&self) -> usize {
        self.l0.slot_count
    }

    /// Calculate the number of ticks corresponding to the delay (based on L0 layer)
    /// 
    /// 计算延迟对应的 tick 数量（基于 L0 层）
    #[allow(dead_code)]
    pub fn delay_to_ticks(&self, delay: Duration) -> u64 {
        self.l0.delay_to_ticks(delay)
    }
    
    /// Determine which layer the delay should be inserted into
    ///
    /// # Returns
    /// Returns: (layer, ticks, rounds)
    /// - Layer: 0 = L0, 1 = L1
    /// - Ticks: number of ticks calculated from current tick
    /// - Rounds: number of rounds (only used for very long delays in L1 layer)
    /// 
    /// 确定延迟应该插入到哪一层
    ///
    /// # 返回值
    /// 返回：(层级, ticks, 轮数)
    /// - 层级：0 = L0, 1 = L1
    /// - Ticks：从当前 tick 计算的 tick 数量
    /// - 轮数：轮数（仅用于 L1 层的超长延迟）
    #[inline(always)]
    fn determine_layer(&self, delay: Duration) -> (u8, u64, u32) {
        let delay_ms = delay.as_millis() as u64;
        
        // Fast path: most tasks are within L0 range (using cached capacity)
        // 快速路径：大多数任务在 L0 范围内（使用缓存的容量）
        if delay_ms < self.l0_capacity_ms {
            let l0_ticks = (delay_ms / self.l0.tick_duration_ms).max(1);
            return (0, l0_ticks, 0);
        }
        
        // Slow path: L1 layer tasks (using cached values)
        // 慢速路径：L1 层任务（使用缓存的值）
        let l1_ticks = (delay_ms / self.l1.tick_duration_ms).max(1);
        
        if l1_ticks < self.l1_capacity_ticks {
            (1, l1_ticks, 0)
        } else {
            let rounds = (l1_ticks / self.l1_capacity_ticks) as u32;
            (1, l1_ticks, rounds)
        }
    }

    /// Insert timer task
    ///
    /// # Parameters
    /// - `task`: Timer task
    /// - `notifier`: Completion notifier (used to send notifications when tasks expire or are cancelled)
    ///
    /// # Returns
    /// Unique identifier of the task (TaskId)
    ///
    /// # Implementation Details
    /// - Automatically calculate the layer and slot where the task should be inserted
    /// - Hierarchical mode: short delay tasks are inserted into L0, long delay tasks are inserted into L1
    /// - Use bit operations to optimize slot index calculation
    /// - Maintain task index to support O(1) lookup and cancellation
    /// 
    /// 插入定时器任务
    ///
    /// # 参数
    /// - `task`: 定时器任务
    /// - `notifier`: 完成通知器（用于在任务过期或取消时发送通知）
    ///
    /// # 返回值
    /// 任务的唯一标识符（TaskId）
    ///
    /// # 实现细节
    /// - 自动计算任务应该插入的层级和槽位
    /// - 分层模式：短延迟任务插入 L0，长延迟任务插入 L1
    /// - 使用位运算优化槽索引计算
    /// - 维护任务索引以支持 O(1) 查找和取消
    #[inline]
    pub fn insert(&mut self, mut task: TimerTaskWithCompletionNotifier) -> TaskId {
        let (level, ticks, rounds) = self.determine_layer(task.delay);
        
        // Use match to reduce branches, and use cached slot mask
        // 使用 match 减少分支，并使用缓存的槽掩码
        let (current_tick, slot_mask, slots) = match level {
            0 => (self.l0.current_tick, self.l0.slot_mask, &mut self.l0.slots),
            _ => (self.l1.current_tick, self.l1.slot_mask, &mut self.l1.slots),
        };
        
        let total_ticks = current_tick + ticks;
        let slot_index = (total_ticks as usize) & slot_mask;

        // Prepare task registration (set notifier and timing wheel parameters)
        // 准备任务注册（设置通知器和时间轮参数）
        task.prepare_for_registration(total_ticks, rounds);

        let task_id = task.id;
        
        // Get the index position of the task in Vec (the length before insertion is the index of the new task)
        // 获取任务在 Vec 中的索引位置（插入前的长度就是新任务的索引）
        let vec_index = slots[slot_index].len();
        let location = TaskLocation::new(level, slot_index, vec_index);

        // Insert task into slot
        // 将任务插入槽中
        slots[slot_index].push(task);
        
        // Record task location
        // 记录任务位置
        self.task_index.insert(task_id, location);

        task_id
    }

    /// Batch insert timer tasks
    ///
    /// # Parameters
    /// - `tasks`: List of tuples of (task, completion notifier)
    ///
    /// # Returns
    /// List of task IDs
    ///
    /// # Performance Advantages
    /// - Reduce repeated boundary checks and capacity adjustments
    /// - For tasks with the same delay, calculation results can be reused
    /// 
    /// 批量插入定时器任务
    ///
    /// # 参数
    /// - `tasks`: (任务, 完成通知器) 元组列表
    ///
    /// # 返回值
    /// 任务 ID 列表
    ///
    /// # 性能优势
    /// - 减少重复的边界检查和容量调整
    /// - 对于相同延迟的任务，可以重用计算结果
    #[inline]
    pub fn insert_batch(&mut self, tasks: Vec<TimerTaskWithCompletionNotifier>) -> Vec<TaskId> {
        let task_count = tasks.len();
        
        // Optimize: pre-allocate HashMap capacity to avoid reallocation
        // 优化：预分配 HashMap 容量以避免重新分配
        self.task_index.reserve(task_count);
        
        let mut task_ids = Vec::with_capacity(task_count);
        
        for mut task in tasks {
            let (level, ticks, rounds) = self.determine_layer(task.delay);
            
            // Use match to reduce branches, and use cached slot mask
            // 使用 match 减少分支，并使用缓存的槽掩码
            let (current_tick, slot_mask, slots) = match level {
                0 => (self.l0.current_tick, self.l0.slot_mask, &mut self.l0.slots),
                _ => (self.l1.current_tick, self.l1.slot_mask, &mut self.l1.slots),
            };
            
            let total_ticks = current_tick + ticks;
            let slot_index = (total_ticks as usize) & slot_mask;

            // Prepare task registration (set notifier and timing wheel parameters)
            // 准备任务注册（设置通知器和时间轮参数）
            task.prepare_for_registration(total_ticks, rounds);

            let task_id = task.id;
            
            // Get the index position of the task in Vec
            // 获取任务在 Vec 中的索引位置
            let vec_index = slots[slot_index].len();
            let location = TaskLocation::new(level, slot_index, vec_index);

            // Insert task into slot
            // 将任务插入槽中
            slots[slot_index].push(task);
            
            // Record task location
            // 记录任务位置
            self.task_index.insert(task_id, location);
            
            task_ids.push(task_id);
        }
        
        task_ids
    }

    /// Cancel timer task
    ///
    /// # Parameters
    /// - `task_id`: Task ID
    ///
    /// # Returns
    /// Returns true if the task exists and is successfully cancelled, otherwise returns false
    /// 
    /// 取消定时器任务
    ///
    /// # 参数
    /// - `task_id`: 任务 ID
    ///
    /// # 返回值
    /// 如果任务存在且成功取消则返回 true，否则返回 false
    #[inline]
    pub fn cancel(&mut self, task_id: TaskId) -> bool {
        // Remove task location from index
        // 从索引中移除任务位置
        let location = match self.task_index.remove(&task_id) {
            Some(loc) => loc,
            None => return false,
        };
        
        // Use match to get slot reference, reduce branches
        // 使用 match 获取槽引用，减少分支
        let slot = match location.level {
            0 => &mut self.l0.slots[location.slot_index],
            _ => &mut self.l1.slots[location.slot_index],
        };
        
        // Boundary check and ID verification
        // 边界检查和 ID 验证
        if location.vec_index >= slot.len() || slot[location.vec_index].id != task_id {
            // Index inconsistent, re-insert location to maintain data consistency
            // 索引不一致，重新插入位置以保持数据一致性
            self.task_index.insert(task_id, location);
            return false;
        }
        // Use swap_remove to remove task, record swapped task ID
        // 使用 swap_remove 移除任务，记录被交换的任务 ID
        let removed_task = slot.swap_remove(location.vec_index);
        

        match removed_task.task_type {
            TaskTypeWithCompletionNotifier::OneShot { completion_notifier } => {
                let _ = completion_notifier.0.send(TaskCompletionReasonOneShot::Cancelled);
            }
            TaskTypeWithCompletionNotifier::Periodic { completion_notifier, .. } => {
                let _ = completion_notifier.0.try_send(TaskCompletionReasonPeriodic::Cancelled);
            }
        }
        
        // If a swap occurred (vec_index is not the last element)
        // 如果发生了交换（vec_index 不是最后一个元素）
        if location.vec_index < slot.len() {
            let swapped_task_id = slot[location.vec_index].id;
            // Update swapped element's index in one go, avoid another HashMap query
            // 一次性更新被交换元素的索引，避免再次查询 HashMap
            if let Some(swapped_location) = self.task_index.get_mut(&swapped_task_id) {
                swapped_location.vec_index = location.vec_index;
            }
        }
        
        // Ensure the correct task was removed
        // 确保移除了正确的任务
        debug_assert_eq!(removed_task.id, task_id);
        true
    }

    /// Batch cancel timer tasks
    ///
    /// # Parameters
    /// - `task_ids`: List of task IDs to cancel
    ///
    /// # Returns
    /// Number of successfully cancelled tasks
    ///
    /// # Performance Advantages
    /// - Reduce repeated HashMap lookup overhead
    /// - Multiple cancellation operations on the same slot can be batch processed
    /// - Use unstable sort to improve performance
    /// - Small batch optimization: skip sorting based on configuration threshold, process directly
    /// 
    /// 批量取消定时器任务
    ///
    /// # 参数
    /// - `task_ids`: 要取消的任务 ID 列表
    ///
    /// # 返回值
    /// 成功取消的任务数量
    ///
    /// # 性能优势
    /// - 减少重复的 HashMap 查找开销
    /// - 同一槽位的多个取消操作可以批量处理
    /// - 使用不稳定排序提高性能
    /// - 小批量优化：根据配置阈值跳过排序，直接处理
    #[inline]
    pub fn cancel_batch(&mut self, task_ids: &[TaskId]) -> usize {
        let mut cancelled_count = 0;
        
        // Small batch optimization: cancel one by one to avoid grouping and sorting overhead
        // 小批量优化：逐个取消以避免分组和排序开销
        if task_ids.len() <= self.batch_config.small_batch_threshold {
            for &task_id in task_ids {
                if self.cancel(task_id) {
                    cancelled_count += 1;
                }
            }
            return cancelled_count;
        }
        
        // Group by layer and slot to optimize batch cancellation
        // Use SmallVec to avoid heap allocation in most cases
        // 按层级和槽位分组以优化批量取消
        // 使用 SmallVec 避免在大多数情况下进行堆分配
        let l0_slot_count = self.l0.slot_count;
        let l1_slot_count = self.l1.slot_count;
        
        let mut l0_tasks_by_slot: Vec<Vec<(TaskId, usize)>> = vec![Vec::new(); l0_slot_count];
        let mut l1_tasks_by_slot: Vec<Vec<(TaskId, usize)>> = vec![Vec::new(); l1_slot_count];
        
        // Collect information of tasks to be cancelled
        // 收集要取消的任务信息
        for &task_id in task_ids {
            if let Some(location) = self.task_index.get(&task_id) {
                if location.level == 0 {
                    l0_tasks_by_slot[location.slot_index].push((task_id, location.vec_index));
                } else {
                    l1_tasks_by_slot[location.slot_index].push((task_id, location.vec_index));
                }
            }
        }
        
        // Process L0 layer cancellation
        // 处理 L0 层取消
        for (slot_index, tasks) in l0_tasks_by_slot.iter_mut().enumerate() {
            if tasks.is_empty() {
                continue;
            }
            
            // Sort by vec_index in descending order, delete from back to front to avoid index invalidation
            // 按 vec_index 降序排序，从后向前删除以避免索引失效
            tasks.sort_unstable_by(|a, b| b.1.cmp(&a.1));
            
            let slot = &mut self.l0.slots[slot_index];
            
            for &(task_id, vec_index) in tasks.iter() {
                if vec_index < slot.len() && slot[vec_index].id == task_id {
                    let removed_task = slot.swap_remove(vec_index);
                    match removed_task.task_type {
                        TaskTypeWithCompletionNotifier::OneShot { completion_notifier } => {
                            let _ = completion_notifier.0.send(TaskCompletionReasonOneShot::Cancelled);
                        }
                        TaskTypeWithCompletionNotifier::Periodic { completion_notifier, .. } => {
                            let _ = completion_notifier.0.try_send(TaskCompletionReasonPeriodic::Cancelled);
                        }
                    }
                    
                    
                    if vec_index < slot.len() {
                        let swapped_task_id = slot[vec_index].id;
                        if let Some(swapped_location) = self.task_index.get_mut(&swapped_task_id) {
                            swapped_location.vec_index = vec_index;
                        }
                    }
                    
                    self.task_index.remove(&task_id);
                    cancelled_count += 1;
                }
            }
        }
        
        // Process L1 layer cancellation
        // 处理 L1 层取消
        for (slot_index, tasks) in l1_tasks_by_slot.iter_mut().enumerate() {
            if tasks.is_empty() {
                continue;
            }
            
            tasks.sort_unstable_by(|a, b| b.1.cmp(&a.1));
            
            let slot = &mut self.l1.slots[slot_index];
            
            for &(task_id, vec_index) in tasks.iter() {
                if vec_index < slot.len() && slot[vec_index].id == task_id {
            
                    let removed_task = slot.swap_remove(vec_index);
                    
                    match removed_task.task_type {
                        TaskTypeWithCompletionNotifier::OneShot { completion_notifier } => {
                            let _ = completion_notifier.0.send(TaskCompletionReasonOneShot::Cancelled);
                        }
                        TaskTypeWithCompletionNotifier::Periodic { completion_notifier, .. } => {
                            let _ = completion_notifier.0.try_send(TaskCompletionReasonPeriodic::Cancelled);
                        }
                    }
                    
                    if vec_index < slot.len() {
                        let swapped_task_id = slot[vec_index].id;
                        if let Some(swapped_location) = self.task_index.get_mut(&swapped_task_id) {
                            swapped_location.vec_index = vec_index;
                        }
                    }
                    
                    self.task_index.remove(&task_id);
                    cancelled_count += 1;
                }
            }
        }
        
        cancelled_count
    }

    /// Reinsert periodic task with the same TaskId
    /// 
    /// Used to automatically reschedule periodic tasks after they expire
    /// 
    /// 重新插入周期性任务（保持相同的 TaskId）
    /// 
    /// 用于在周期性任务过期后自动重新调度
    fn reinsert_periodic_task(
        &mut self,
        task_id: TaskId,
        interval: Duration,
        callback: Option<crate::task::CallbackWrapper>,
        notifier: crate::task::PeriodicCompletionNotifier,
    ) {
        // Determine which layer the interval should be inserted into
        // 确定间隔应该插入到哪一层
        let (level, ticks, rounds) = self.determine_layer(interval);
        
        // Use match to reduce branches, and use cached slot mask
        // 使用 match 减少分支，并使用缓存的槽掩码
        let (current_tick, slot_mask, slots) = match level {
            0 => (self.l0.current_tick, self.l0.slot_mask, &mut self.l0.slots),
            _ => (self.l1.current_tick, self.l1.slot_mask, &mut self.l1.slots),
        };
        
        let total_ticks = current_tick + ticks;
        let slot_index = (total_ticks as usize) & slot_mask;
        
        // Create new task with the same TaskId
        // 创建新任务，使用相同的 TaskId
        let task = TimerTaskWithCompletionNotifier {
            id: task_id,
            task_type: TaskTypeWithCompletionNotifier::Periodic { interval, completion_notifier: notifier },
            delay: interval,
            deadline_tick: total_ticks,
            rounds,
            callback,
        };
        
        // Get the index position of the task in Vec
        // 获取任务在 Vec 中的索引位置
        let vec_index = slots[slot_index].len();
        let location = TaskLocation::new(level, slot_index, vec_index);
        
        // Insert task into slot
        // 将任务插入槽中
        slots[slot_index].push(task);
        
        // Record task location
        // 记录任务位置
        self.task_index.insert(task_id, location);
    }

    /// Advance the timing wheel by one tick, return all expired tasks
    ///
    /// # Returns
    /// List of expired tasks
    ///
    /// # Implementation Details
    /// - L0 layer advances 1 tick each time (no rounds check)
    /// - L1 layer advances once every (L1_tick / L0_tick) times
    /// - L1 expired tasks are batch demoted to L0
    /// 
    /// 推进时间轮一个 tick，返回所有过期的任务
    ///
    /// # 返回值
    /// 过期任务列表
    ///
    /// # 实现细节
    /// - L0 层每次推进 1 个 tick（无轮数检查）
    /// - L1 层每 (L1_tick / L0_tick) 次推进一次
    /// - L1 层过期任务批量降级到 L0
    pub fn advance(&mut self) -> Vec<TimerTask> {
        // Advance L0 layer
        // 推进 L0 层
        self.l0.current_tick += 1;
        
        let mut expired_tasks = Vec::new();
        
        // Process L0 layer expired tasks
        // Distinguish between one-shot and periodic tasks
        // 处理 L0 层过期任务
        // 区分一次性任务和周期性任务
        let l0_slot_index = (self.l0.current_tick as usize) & self.l0.slot_mask;
        
        // Collect periodic tasks to reinsert
        // 收集需要重新插入的周期性任务
        let mut periodic_tasks_to_reinsert = Vec::new();
        
        {
            let l0_slot = &mut self.l0.slots[l0_slot_index];
            
            let i = 0;
            while i < l0_slot.len() {
                // Check if this is a one-shot or periodic task
                // 检查这是一次性任务还是周期性任务
                let is_periodic = matches!(l0_slot[i].task_type, TaskTypeWithCompletionNotifier::Periodic { .. });
                
                // Remove from index
                // 从索引中移除
                let task_id = l0_slot[i].id;
                self.task_index.remove(&task_id);
                
                // Use swap_remove to remove task
                // 使用 swap_remove 移除任务
                let task_with_notifier = l0_slot.swap_remove(i);
                
                // Update swapped element's index
                // 更新被交换元素的索引
                if i < l0_slot.len() {
                    let swapped_task_id = l0_slot[i].id;
                    if let Some(swapped_location) = self.task_index.get_mut(&swapped_task_id) {
                        swapped_location.vec_index = i;
                    }
                }
                
                // Split task to get TimerTask and CompletionNotifier
                // 拆分任务以获取 TimerTask 和 CompletionNotifier
                let (timer_task, completion_notifier) = task_with_notifier.split();
                
                if is_periodic {
                    // Periodic task: send notification and collect for reinsertion
                    // 周期性任务：发送通知并收集用于重新插入
                    
                    if let CompletionNotifier::Periodic(notifier) = completion_notifier {
                        // Send periodic notification
                        // 发送周期性通知
                        let _ = notifier.0.try_send(TaskCompletionReasonPeriodic::Called);
                        
                        // Extract necessary information for reinsertion
                        // 提取重新插入所需的信息
                        let interval = timer_task.get_interval().unwrap();
                        let callback = timer_task.callback.clone();
                        
                        // Collect for reinsertion
                        // 收集用于重新插入
                        periodic_tasks_to_reinsert.push((task_id, interval, callback, crate::task::PeriodicCompletionNotifier(notifier.0)));
                    }
                    
                    // Add to expired tasks for callback execution
                    // 添加到过期任务以执行回调
                    expired_tasks.push(timer_task);
                } else {
                    // One-shot task: send notification and add to expired list
                    // 一次性任务：发送通知并添加到过期列表
                    
                    if let CompletionNotifier::OneShot(notifier) = completion_notifier {
                        // Send completion notification
                        // 发送完成通知
                        let _ = notifier.0.send(TaskCompletionReasonOneShot::Expired);
                    }
                    
                    // Add to expired tasks
                    // 添加到过期任务列表
                    expired_tasks.push(timer_task);
                }
                
                // Don't increment i, as we've removed the current element
                // 不增加 i，因为我们已经移除了当前元素
            }
        }
        
        // Reinsert periodic tasks for next interval
        // 重新插入周期性任务到下一个周期
        for (task_id, interval, callback, notifier) in periodic_tasks_to_reinsert {
            self.reinsert_periodic_task(task_id, interval, callback, notifier);
        }
        
        // Process L1 layer
        // Check if L1 layer needs to be advanced
        // 处理 L1 层
        // 检查是否需要推进 L1 层
        if self.l0.current_tick % self.l1_tick_ratio == 0 {
            self.l1.current_tick += 1;
            let l1_slot_index = (self.l1.current_tick as usize) & self.l1.slot_mask;
            let l1_slot = &mut self.l1.slots[l1_slot_index];
            
            // Collect L1 layer expired tasks
            // 收集 L1 层过期任务
            let mut tasks_to_demote = Vec::new();
            let mut i = 0;
            while i < l1_slot.len() {
                let task = &mut l1_slot[i];
                
                if task.rounds > 0 {
                    // Still has rounds, decrease rounds and keep
                    // 还有轮数，减少轮数并保留
                    task.rounds -= 1;
                    if let Some(location) = self.task_index.get_mut(&task.id) {
                        location.vec_index = i;
                    }
                    i += 1;
                } else {
                    // rounds = 0, need to demote to L0
                    // rounds = 0，需要降级到 L0
                    self.task_index.remove(&task.id);
                    let task_to_demote = l1_slot.swap_remove(i);
                    
                    if i < l1_slot.len() {
                        let swapped_task_id = l1_slot[i].id;
                        if let Some(swapped_location) = self.task_index.get_mut(&swapped_task_id) {
                            swapped_location.vec_index = i;
                        }
                    }
                    
                    tasks_to_demote.push(task_to_demote);
                }
            }
            
            // Demote tasks to L0
            // 将任务降级到 L0
            self.demote_tasks(tasks_to_demote);
        }
        
        expired_tasks
    }
    
    /// Demote tasks from L1 to L0
    /// 
    /// Recalculate and insert L1 expired tasks into L0 layer
    /// 
    /// 将任务从 L1 降级到 L0
    /// 
    /// 重新计算并将 L1 过期任务插入到 L0 层
    fn demote_tasks(&mut self, tasks: Vec<TimerTaskWithCompletionNotifier>) {
        for task in tasks {
            // Calculate the remaining delay of the task in L0 layer
            // The task's deadline_tick is based on L1 tick, needs to be converted to L0 tick
            // 计算任务在 L0 层的剩余延迟
            // 任务的 deadline_tick 基于 L1 tick，需要转换为 L0 tick
            let l1_tick_ratio = self.l1_tick_ratio;
            
            // Calculate the original expiration time of the task (L1 tick)
            // 计算任务的原始过期时间（L1 tick）
            let l1_deadline = task.deadline_tick;
            
            // Convert to L0 tick expiration time
            // 转换为 L0 tick 过期时间
            let l0_deadline_tick = l1_deadline * l1_tick_ratio;
            let l0_current_tick = self.l0.current_tick;
            
            // Calculate remaining L0 ticks
            // 计算剩余 L0 ticks
            let remaining_l0_ticks = if l0_deadline_tick > l0_current_tick {
                l0_deadline_tick - l0_current_tick
            } else {
                1 // At least trigger in next tick (至少在下一个 tick 触发)
            };
            
            // Calculate L0 slot index
            // 计算 L0 槽索引
            let target_l0_tick = l0_current_tick + remaining_l0_ticks;
            let l0_slot_index = (target_l0_tick as usize) & self.l0.slot_mask;
            
            let task_id = task.id;
            let vec_index = self.l0.slots[l0_slot_index].len();
            let location = TaskLocation::new(0, l0_slot_index, vec_index);
            
            // Insert into L0 layer
            // 插入到 L0 层
            self.l0.slots[l0_slot_index].push(task);
            self.task_index.insert(task_id, location);
        }
    }

    /// Check if the timing wheel is empty
    /// 
    /// 检查时间轮是否为空
    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.task_index.is_empty()
    }

    /// Postpone timer task (keep original TaskId)
    ///
    /// # Parameters
    /// - `task_id`: Task ID to postpone
    /// - `new_delay`: New delay time (recalculated from current tick, not continuing from original delay time)
    /// - `new_callback`: New callback function (if None, keep original callback)
    ///
    /// # Returns
    /// Returns true if the task exists and is successfully postponed, otherwise returns false
    ///
    /// # Implementation Details
    /// - Remove task from original layer/slot, keep its completion_notifier (will not trigger cancellation notification)
    /// - Update delay time and callback function (if provided)
    /// - Recalculate target layer, slot, and rounds based on new_delay
    /// - Cross-layer migration may occur (L0 <-> L1)
    /// - Re-insert to new position using original TaskId
    /// - Keep consistent with external held TaskId reference
    /// 
    /// 延期定时器任务（保留原始 TaskId）
    ///
    /// # 参数
    /// - `task_id`: 要延期的任务 ID
    /// - `new_delay`: 新延迟时间（从当前 tick 重新计算，而非从原延迟时间继续）
    /// - `new_callback`: 新回调函数（如果为 None，则保留原回调）
    ///
    /// # 返回值
    /// 如果任务存在且成功延期则返回 true，否则返回 false
    ///
    /// # 实现细节
    /// - 从原层级/槽位移除任务，保留其 completion_notifier（不会触发取消通知）
    /// - 更新延迟时间和回调函数（如果提供）
    /// - 根据 new_delay 重新计算目标层级、槽位和轮数
    /// - 可能发生跨层迁移（L0 <-> L1）
    /// - 使用原始 TaskId 重新插入到新位置
    /// - 与外部持有的 TaskId 引用保持一致
    #[inline]
    pub fn postpone(
        &mut self,
        task_id: TaskId,
        new_delay: Duration,
        new_callback: Option<crate::task::CallbackWrapper>,
    ) -> bool {
        // Step 1: Find and remove original task
        // 步骤 1: 查找并移除原任务
        let old_location = match self.task_index.remove(&task_id) {
            Some(loc) => loc,
            None => return false,
        };
        
        // Use match to get slot reference
        // 使用 match 获取槽引用
        let slot = match old_location.level {
            0 => &mut self.l0.slots[old_location.slot_index],
            _ => &mut self.l1.slots[old_location.slot_index],
        };
        
        // Verify task is still at expected position
        // 验证任务仍在预期位置
        if old_location.vec_index >= slot.len() || slot[old_location.vec_index].id != task_id {
            // Index inconsistent, re-insert and return failure
            // 索引不一致，重新插入并返回失败
            self.task_index.insert(task_id, old_location);
            return false;
        }
        
        // Use swap_remove to remove task
        // 使用 swap_remove 移除任务
        let mut task = slot.swap_remove(old_location.vec_index);
        
        // Update swapped element's index (if a swap occurred)
        // 更新被交换元素的索引（如果发生了交换）
        if old_location.vec_index < slot.len() {
            let swapped_task_id = slot[old_location.vec_index].id;
            if let Some(swapped_location) = self.task_index.get_mut(&swapped_task_id) {
                swapped_location.vec_index = old_location.vec_index;
            }
        }
        
        // Step 2: Update task's delay and callback
        // 步骤 2: 更新任务的延迟和回调
        task.delay = new_delay;
        if let Some(callback) = new_callback {
            task.callback = Some(callback);
        }
        
        // Step 3: Recalculate layer, slot, and rounds based on new delay
        // 步骤 3: 根据新延迟重新计算层级、槽位和轮数
        let (new_level, ticks, new_rounds) = self.determine_layer(new_delay);
        
        // Use match to reduce branches, and use cached slot mask
        // 使用 match 减少分支，并使用缓存的槽掩码
        let (current_tick, slot_mask, slots) = match new_level {
            0 => (self.l0.current_tick, self.l0.slot_mask, &mut self.l0.slots),
            _ => (self.l1.current_tick, self.l1.slot_mask, &mut self.l1.slots),
        };
        
        let total_ticks = current_tick + ticks;
        let new_slot_index = (total_ticks as usize) & slot_mask;
        
        // Update task's timing wheel parameters
        // 更新任务的时间轮参数
        task.deadline_tick = total_ticks;
        task.rounds = new_rounds;
        
        // Step 4: Re-insert task to new layer/slot
        // 步骤 4: 将任务重新插入到新层级/槽位
        let new_vec_index = slots[new_slot_index].len();
        let new_location = TaskLocation::new(new_level, new_slot_index, new_vec_index);
        
        slots[new_slot_index].push(task);
        self.task_index.insert(task_id, new_location);
        
        true
    }

    /// Batch postpone timer tasks
    ///
    /// # Parameters
    /// - `updates`: List of tuples of (task ID, new delay)
    ///
    /// # Returns
    /// Number of successfully postponed tasks
    ///
    /// # Performance Advantages
    /// - Batch processing reduces function call overhead
    /// - All delays are recalculated from current_tick at call time
    ///
    /// # Notes
    /// - If a task ID does not exist, that task will be skipped without affecting other tasks' postponement
    /// 
    /// 批量延期定时器任务
    ///
    /// # 参数
    /// - `updates`: (任务 ID, 新延迟) 元组列表
    ///
    /// # 返回值
    /// 成功延期的任务数量
    ///
    /// # 性能优势
    /// - 批处理减少函数调用开销
    /// - 所有延迟在调用时从 current_tick 重新计算
    ///
    /// # 注意
    /// - 如果任务 ID 不存在，该任务将被跳过，不影响其他任务的延期
    #[inline]
    pub fn postpone_batch(
        &mut self,
        updates: Vec<(TaskId, Duration)>,
    ) -> usize {
        let mut postponed_count = 0;
        
        for (task_id, new_delay) in updates {
            if self.postpone(task_id, new_delay, None) {
                postponed_count += 1;
            }
        }
        
        postponed_count
    }

    /// Batch postpone timer tasks (replace callbacks)
    ///
    /// # Parameters
    /// - `updates`: List of tuples of (task ID, new delay, new callback)
    ///
    /// # Returns
    /// Number of successfully postponed tasks
    /// 
    /// 批量延期定时器任务（替换回调）
    ///
    /// # 参数
    /// - `updates`: (任务 ID, 新延迟, 新回调) 元组列表
    ///
    /// # 返回值
    /// 成功延期的任务数量
    pub fn postpone_batch_with_callbacks(
        &mut self,
        updates: Vec<(TaskId, Duration, Option<crate::task::CallbackWrapper>)>,
    ) -> usize {
        let mut postponed_count = 0;
        
        for (task_id, new_delay, new_callback) in updates {
            if self.postpone(task_id, new_delay, new_callback) {
                postponed_count += 1;
            }
        }
        
        postponed_count
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::task::{CallbackWrapper, TimerTask, TimerTaskWithCompletionNotifier};

    #[test]
    fn test_wheel_creation() {
        let wheel = Wheel::new(WheelConfig::default(), BatchConfig::default());
        assert_eq!(wheel.slot_count(), 512);
        assert_eq!(wheel.current_tick(), 0);
        assert!(wheel.is_empty());
    }

    #[test]
    fn test_hierarchical_wheel_creation() {
        let config = WheelConfig::default();
        
        let wheel = Wheel::new(config, BatchConfig::default());
        assert_eq!(wheel.slot_count(), 512); // L0 slot count (L0 层槽数量)
        assert_eq!(wheel.current_tick(), 0);
        assert!(wheel.is_empty());
        // L1 layer always exists in hierarchical mode (L1 层在分层模式下始终存在)
        assert_eq!(wheel.l1.slot_count, 64);
        assert_eq!(wheel.l1_tick_ratio, 100); // 1000ms / 10ms = 100 ticks (1000毫秒 / 10毫秒 = 100个tick)
    }

    #[test]
    fn test_hierarchical_config_validation() {
        // L1 tick must be an integer multiple of L0 tick (L1 tick 必须是 L0 tick 的整数倍)
        let result = WheelConfig::builder()
            .l0_tick_duration(Duration::from_millis(10))
            .l0_slot_count(512)
            .l1_tick_duration(Duration::from_millis(15)) // Not an integer multiple (不是整数倍)
            .l1_slot_count(64)
            .build();
        
        assert!(result.is_err());
        
        // Correct configuration (正确的配置)
        let result = WheelConfig::builder()
            .l0_tick_duration(Duration::from_millis(10))
            .l0_slot_count(512)
            .l1_tick_duration(Duration::from_secs(1)) // 1000ms / 10ms = 100 ticks (1000毫秒 / 10毫秒 = 100个tick)
            .l1_slot_count(64)
            .build();
        
        assert!(result.is_ok());
    }

    #[test]
    fn test_layer_determination() {
        let config = WheelConfig::default();
        
        let wheel = Wheel::new(config, BatchConfig::default());
        
        // Short delay should enter L0 layer (短延迟应该进入 L0 层)
        // L0: 512 slots * 10ms = 5120ms
        let (level, _, rounds) = wheel.determine_layer(Duration::from_millis(100));
        assert_eq!(level, 0);
        assert_eq!(rounds, 0);
        
        // Long delay should enter L1 layer
        // 超过 L0 范围（>5120ms） (超过 L0 范围 (>5120毫秒))
        let (level, _, rounds) = wheel.determine_layer(Duration::from_secs(10));
        assert_eq!(level, 1);
        assert_eq!(rounds, 0);
        
        // Long delay should enter L1 layer and have rounds
        // L1: 64 slots * 1000ms = 64000ms (L1: 64个槽 * 1000毫秒 = 64000毫秒)
        let (level, _, rounds) = wheel.determine_layer(Duration::from_secs(120));
        assert_eq!(level, 1);
        assert!(rounds > 0);
    }

    #[test]
    fn test_hierarchical_insert_and_advance() {
        let config = WheelConfig::default();
        
        let mut wheel = Wheel::new(config, BatchConfig::default());
        
        // Insert short delay task into L0 (插入短延迟任务到 L0)
        let callback = CallbackWrapper::new(|| async {});
        let task = TimerTask::new_oneshot(Duration::from_millis(100), Some(callback));
        let (task_with_notifier, _completion_rx) = TimerTaskWithCompletionNotifier::from_timer_task(task, 32);
        let task_id = wheel.insert(task_with_notifier);
        
        // Verify task is in L0 layer (验证任务在 L0 层)
        let location = wheel.task_index.get(&task_id).unwrap();
        assert_eq!(location.level, 0);
        
        // Advance 10 ticks (100ms) (前进 10 个 tick (100毫秒))
        for _ in 0..10 {
            let expired = wheel.advance();
            if !expired.is_empty() {
                assert_eq!(expired.len(), 1);
                assert_eq!(expired[0].id, task_id);
                return;
            }
        }
        panic!("Task should have expired");
    }

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
        let (task_with_notifier, _completion_rx) = TimerTaskWithCompletionNotifier::from_timer_task(task, 32);
        let task_id = wheel.insert(task_with_notifier);
        
        // Verify task is in L1 layer (验证任务在 L1 层)
        let location = wheel.task_index.get(&task_id).unwrap();
        assert_eq!(location.level, 1);
        
        // Advance to L1 slot expiration (6000ms / 100ms = 60 L1 ticks) (前进到 L1 槽过期 (6000毫秒 / 100毫秒 = 60个L1 tick))
        // 60 L1 ticks = 600 L0 ticks (60个L1 tick = 600个L0 tick)
        let mut demoted = false;
        for i in 0..610 {
            wheel.advance();
            
            // Check if task is demoted to L0 (检查任务是否降级到 L0)
            if let Some(location) = wheel.task_index.get(&task_id) {
                if location.level == 0 && !demoted {
                    demoted = true;
                    println!("Task demoted to L0 at L0 tick {}", i); // 任务降级到 L0 在 L0 tick {i}
                }
            }
        }
        
        assert!(demoted, "Task should have been demoted from L1 to L0"); // 任务应该从 L1 降级到 L0
    }

    #[test]
    fn test_cross_layer_cancel() {
        let config = WheelConfig::default();
        
        let mut wheel = Wheel::new(config, BatchConfig::default());
        
        // Insert L0 task (插入 L0 任务)
        let callback1 = CallbackWrapper::new(|| async {});
        let task1 = TimerTask::new_oneshot(Duration::from_millis(100), Some(callback1));
        let (task_with_notifier1, _rx1) = TimerTaskWithCompletionNotifier::from_timer_task(task1, 32);
        let task_id1 = wheel.insert(task_with_notifier1);
        
        // Insert L1 task (插入 L1 任务)
        let callback2 = CallbackWrapper::new(|| async {});
        let task2 = TimerTask::new_oneshot(Duration::from_secs(10), Some(callback2));
        let (task_with_notifier2, _rx2) = TimerTaskWithCompletionNotifier::from_timer_task(task2, 32);
        let task_id2 = wheel.insert(task_with_notifier2);
        
        // Verify levels (验证层级)
        assert_eq!(wheel.task_index.get(&task_id1).unwrap().level, 0);
        assert_eq!(wheel.task_index.get(&task_id2).unwrap().level, 1);
        
        // Cancel L0 task (取消 L0 任务)
        assert!(wheel.cancel(task_id1));
        assert!(wheel.task_index.get(&task_id1).is_none());
        
        // Cancel L1 task (取消 L1 任务)
        assert!(wheel.cancel(task_id2));
        assert!(wheel.task_index.get(&task_id2).is_none());
        
        assert!(wheel.is_empty()); // 时间轮应该为空
    }

    #[test]
    fn test_cross_layer_postpone() {
        let config = WheelConfig::default();
        
        let mut wheel = Wheel::new(config, BatchConfig::default());
        
        // Insert L0 task (100ms) (插入 L0 任务 (100毫秒))
        let callback = CallbackWrapper::new(|| async {});
        let task = TimerTask::new_oneshot(Duration::from_millis(100), Some(callback));
        let (task_with_notifier, _rx) = TimerTaskWithCompletionNotifier::from_timer_task(task, 32);
        let task_id = wheel.insert(task_with_notifier);
        
        // Verify in L0 layer (验证在 L0 层)
        assert_eq!(wheel.task_index.get(&task_id).unwrap().level, 0);
        
        // Postpone to 10 seconds (should migrate to L1) (延期到 10 秒 (应该迁移到 L1))
        assert!(wheel.postpone(task_id, Duration::from_secs(10), None));
        
        // Verify migrated to L1 layer (验证迁移到 L1 层)
        assert_eq!(wheel.task_index.get(&task_id).unwrap().level, 1);
        
        // Postpone back to 200ms (should migrate back to L0) (延期回 200毫秒 (应该迁移回 L0))
        assert!(wheel.postpone(task_id, Duration::from_millis(200), None));
        
        // Verify migrated back to L0 layer (验证迁移回 L0 层)
        assert_eq!(wheel.task_index.get(&task_id).unwrap().level, 0);
    }

    #[test]
    fn test_delay_to_ticks() {
        let wheel = Wheel::new(WheelConfig::default(), BatchConfig::default());
        assert_eq!(wheel.delay_to_ticks(Duration::from_millis(100)), 10);
        assert_eq!(wheel.delay_to_ticks(Duration::from_millis(50)), 5);
        assert_eq!(wheel.delay_to_ticks(Duration::from_millis(1)), 1); // Minimum 1 tick (最小 1 个 tick)
    }

    #[test]
    fn test_wheel_invalid_slot_count() {
        let result = WheelConfig::builder()
            .l0_slot_count(100)
            .build();
        assert!(result.is_err());
        if let Err(crate::error::TimerError::InvalidSlotCount { slot_count, reason }) = result {
            assert_eq!(slot_count, 100);
            assert_eq!(reason, "L0 layer slot count must be power of 2"); // L0 层槽数量必须是 2 的幂
        } else {
            panic!("Expected InvalidSlotCount error"); // 期望 InvalidSlotCount 错误
        }
    }

    #[test]
    fn test_insert_batch() {
        let mut wheel = Wheel::new(WheelConfig::default(), BatchConfig::default());
        
        // Create batch tasks (创建批量任务)
        let tasks: Vec<TimerTaskWithCompletionNotifier> = (0..10)
            .map(|i| {
                let callback = CallbackWrapper::new(|| async {});
                let task = TimerTask::new_oneshot(Duration::from_millis(100 + i * 10), Some(callback));
                let (task_with_notifier, _completion_rx) = TimerTaskWithCompletionNotifier::from_timer_task(task, 32);
                task_with_notifier
            })
            .collect();
        
        let task_ids = wheel.insert_batch(tasks);
        
        assert_eq!(task_ids.len(), 10);
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
            let (task_with_notifier, _completion_rx) = TimerTaskWithCompletionNotifier::from_timer_task(task, 32);
            let task_id = wheel.insert(task_with_notifier);
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
            let (task_with_notifier, _completion_rx) = TimerTaskWithCompletionNotifier::from_timer_task(task, 32);
            let task_id = wheel.insert(task_with_notifier);
            task_ids.push(task_id);
        }
        
        // Batch cancel all tasks (批量取消所有任务)
        let cancelled_count = wheel.cancel_batch(&task_ids);
        assert_eq!(cancelled_count, 20);
        assert!(wheel.is_empty());
    }

    #[test]
    fn test_postpone_single_task() {
        let mut wheel = Wheel::new(WheelConfig::default(), BatchConfig::default());
        
        // Insert task, delay 100ms (插入任务，延迟 100毫秒)
        let callback = CallbackWrapper::new(|| async {});
        let task = TimerTask::new_oneshot(Duration::from_millis(100), Some(callback));
        let (task_with_notifier, _completion_rx) = TimerTaskWithCompletionNotifier::from_timer_task(task, 32);
        let task_id = wheel.insert(task_with_notifier);
        
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
        let (task_with_notifier, _completion_rx) = TimerTaskWithCompletionNotifier::from_timer_task(task, 32);
        let task_id = wheel.insert(task_with_notifier);
        
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
                assert_eq!(expired.len(), 1, "On the {}th advance, there should be 1 task triggered", i + 1);
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
        let fake_task_id = TaskId::new();
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
            let (task_with_notifier, _completion_rx) = TimerTaskWithCompletionNotifier::from_timer_task(task, 32);
            let task_id = wheel.insert(task_with_notifier);
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
            assert!(expired.is_empty(), "The first 5 ticks should not have tasks triggered");
        }
        
        // Continue advancing 10 ticks (from tick 5 to tick 15), all tasks should trigger on the 15th tick (继续前进 10个tick (从tick 5到tick 15)，所有任务应该在第 15 个 tick 触发)
        let mut total_triggered = 0;
        for _ in 0..10 {
            let expired = wheel.advance();
            total_triggered += expired.len();
        }
        assert_eq!(total_triggered, 5, "There should be 5 tasks triggered on the 15th tick"); // 第 15 个 tick 应该有 5 个任务触发
    }

    #[test]
    fn test_postpone_batch_partial() {
        let mut wheel = Wheel::new(WheelConfig::default(), BatchConfig::default());
        
        // Insert 10 tasks, delay 50ms (5 ticks) (插入 10 个任务，延迟 50毫秒 (5个tick))
        let mut task_ids = Vec::new();
        for _ in 0..10 {
            let callback = CallbackWrapper::new(|| async {});
            let task = TimerTask::new_oneshot(Duration::from_millis(50), Some(callback));
            let (task_with_notifier, _completion_rx) = TimerTaskWithCompletionNotifier::from_timer_task(task, 32);
            let task_id = wheel.insert(task_with_notifier);
            task_ids.push(task_id);
        }
        
        // Only postpone the first 5 tasks to 150ms, including a nonexistent task (只延期前 5 个任务到 150毫秒，包括一个不存在任务)
        let fake_task_id = TaskId::new();
        let mut updates: Vec<_> = task_ids[0..5]
            .iter()
            .map(|&id| (id, Duration::from_millis(150)))
            .collect();
        updates.push((fake_task_id, Duration::from_millis(150)));
        
        let postponed_count = wheel.postpone_batch(updates);
        assert_eq!(postponed_count, 5, "There should be 5 tasks successfully postponed (fake_task_id failed)"); // 应该有 5 个任务成功延期 (fake_task_id 失败)
        
        // Advance 5 ticks (50ms), the last 5 tasks that were not postponed should trigger (前进 50毫秒 (5个tick)，最后一个没有延期的任务应该触发)
        let mut triggered_at_50ms = 0;
        for _ in 0..5 {
            let expired = wheel.advance();
            triggered_at_50ms += expired.len();
        }
        assert_eq!(triggered_at_50ms, 5, "There should be 5 tasks that were not postponed triggered on the 5th tick"); // 第 5 个 tick 应该有 5 个任务没有延期触发
        
        // Continue advancing 10 ticks (from tick 5 to tick 15), the first 5 tasks that were postponed should trigger (继续前进 10个tick (从tick 5到tick 15)，第一个 5 个任务应该触发)
        let mut triggered_at_150ms = 0;
        for _ in 0..10 {
            let expired = wheel.advance();
            triggered_at_150ms += expired.len();
        }
        assert_eq!(triggered_at_150ms, 5, "There should be 5 tasks that were postponed triggered on the 15th tick"); // 第 15 个 tick 应该有 5 个任务延期触发
    }

    #[test]
    fn test_multi_round_tasks() {
        let mut wheel = Wheel::new(WheelConfig::default(), BatchConfig::default());
        
        // Test multi-round tasks in L1 layer in hierarchical mode (在分层模式下测试 L1 层的多轮任务)
        // L1: 64 slots * 1000ms = 64000ms
        // Insert a task that exceeds one L1 circle, delay 120000ms (120 seconds) (插入一个超过一个 L1 圈的任务，延迟 120000毫秒 (120秒))
        let callback = CallbackWrapper::new(|| async {});
        let task = TimerTask::new_oneshot(Duration::from_secs(120), Some(callback));
        let (task_with_notifier, _completion_rx) = TimerTaskWithCompletionNotifier::from_timer_task(task, 32);
        let task_id = wheel.insert(task_with_notifier);
        
        // 120000ms / 1000ms = 120 L1 ticks (120000毫秒 / 1000毫秒 = 120个L1 tick)
        // 120 ticks / 64 slots = 1 round + 56 ticks (120个tick / 64个槽 = 1轮 + 56个tick)
        // Task should be in L1 layer, slot 56, rounds = 1 (任务应该在 L1 层，槽 56，轮数 = 1)
        
        // Verify task is in L1 layer (验证任务在 L1 层)
        let location = wheel.task_index.get(&task_id).unwrap();
        assert_eq!(location.level, 1);
        
        // L1 tick advances every 100 L0 ticks (L1 tick 每 100 个 L0 tick 前进一次)
        // Advance 64 * 100 = 6400 L0 ticks (complete the first round of L1) (前进 64 * 100 = 6400 L0 tick (完成 L1 的第一轮))
        for _ in 0..6400 {
            let _expired = wheel.advance();
            // During the first round of L1, the task should not be demoted or triggered (在 L1 的第一轮中，任务不应该被降级或触发)
        }
        
        // Task should still be in L1 layer, but rounds has decreased (任务应该仍在 L1 层，但轮数已经减少)
        let location = wheel.task_index.get(&task_id);
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

    #[test]
    fn test_minimum_delay() {
        let mut wheel = Wheel::new(WheelConfig::default(), BatchConfig::default());
        
        // Test minimum delay (delays less than 1 tick should be rounded up to 1 tick) (测试最小延迟 (延迟小于 1 个 tick 应该向上舍入到 1 个 tick))
        let callback = CallbackWrapper::new(|| async {});
        let task = TimerTask::new_oneshot(Duration::from_millis(1), Some(callback));
        let (task_with_notifier, _completion_rx) = TimerTaskWithCompletionNotifier::from_timer_task(task, 32);
        let task_id: TaskId = wheel.insert(task_with_notifier);
        
        // Advance 1 tick, task should trigger (前进 1 个 tick，任务应该触发)
        let expired = wheel.advance();
        assert_eq!(expired.len(), 1, "Minimum delay task should be triggered after 1 tick"); // 最小延迟任务应该在 1 个 tick 后触发
        assert_eq!(expired[0].id, task_id);
    }

    #[test]
    fn test_empty_batch_operations() {
        let mut wheel = Wheel::new(WheelConfig::default(), BatchConfig::default());
        
        // Test empty batch insert (测试空批量插入)
        let task_ids = wheel.insert_batch(vec![]);
        assert_eq!(task_ids.len(), 0);
        
        // Test empty batch cancel (测试空批量取消)
        let cancelled = wheel.cancel_batch(&[]);
        assert_eq!(cancelled, 0);
        
        // Test empty batch postpone (测试空批量延期)
        let postponed = wheel.postpone_batch(vec![]);
        assert_eq!(postponed, 0);
    }

    #[test]
    fn test_postpone_same_task_multiple_times() {
        let mut wheel = Wheel::new(WheelConfig::default(), BatchConfig::default());
        
        // Insert task (插入任务)
        let callback = CallbackWrapper::new(|| async {});
        let task = TimerTask::new_oneshot(Duration::from_millis(100), Some(callback));
        let (task_with_notifier, _completion_rx) = TimerTaskWithCompletionNotifier::from_timer_task(task, 32);
        let task_id = wheel.insert(task_with_notifier);
        
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
        assert!(triggered, "Task should be triggered at the last postpone time"); // 任务应该在最后一次延期时触发
    }

    #[test]
    fn test_advance_empty_slots() {
        let mut wheel = Wheel::new(WheelConfig::default(), BatchConfig::default());
        
        // Do not insert any tasks, advance multiple ticks (不插入任何任务，前进多个tick)
        for _ in 0..100 {
            let expired = wheel.advance();
            assert!(expired.is_empty(), "Empty slots should not return any tasks");
        }
        
        assert_eq!(wheel.current_tick(), 100, "current_tick should correctly increment"); // current_tick 应该正确递增
    }

    #[test]
    fn test_cancel_after_postpone() {
        let mut wheel = Wheel::new(WheelConfig::default(), BatchConfig::default());
        
        // Insert task (插入任务)
        let callback = CallbackWrapper::new(|| async {});
        let task = TimerTask::new_oneshot(Duration::from_millis(100), Some(callback));
        let (task_with_notifier, _completion_rx) = TimerTaskWithCompletionNotifier::from_timer_task(task, 32);
        let task_id = wheel.insert(task_with_notifier);
        
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
    fn test_slot_boundary() {
        let mut wheel = Wheel::new(WheelConfig::default(), BatchConfig::default());
        
        // Test slot boundary and wraparound (测试槽边界和环绕)
        // 第一个任务：延迟 10ms（1 tick），应该在 slot 1 触发
        let callback1 = CallbackWrapper::new(|| async {});
        let task1 = TimerTask::new_oneshot(Duration::from_millis(10), Some(callback1));
        let (task_with_notifier1, _rx1) = TimerTaskWithCompletionNotifier::from_timer_task(task1, 32);
        let task_id_1 = wheel.insert(task_with_notifier1);
        
        // Second task: delay 5110ms (511 ticks), should trigger on slot 511 (第二个任务：延迟 5110毫秒 (511个tick)，应该在槽 511 触发) 
        let callback2 = CallbackWrapper::new(|| async {});
        let task2 = TimerTask::new_oneshot(Duration::from_millis(5110), Some(callback2));
        let (task_with_notifier2, _rx2) = TimerTaskWithCompletionNotifier::from_timer_task(task2, 32);
        let task_id_2 = wheel.insert(task_with_notifier2);
        
        // Advance 1 tick, first task should trigger (前进 1 个 tick，第一个任务应该触发)
        let expired = wheel.advance();
        assert_eq!(expired.len(), 1, "First task should trigger on tick 1"); // 第一个任务应该在第 1 个 tick 触发
        assert_eq!(expired[0].id, task_id_1);
        
        // Continue advancing to 511 ticks (from tick 1 to tick 511), second task should trigger (继续前进 511个tick (从tick 1到tick 511)，第二个任务应该触发)
        let mut triggered = false;
        for i in 0..510 {
            let expired = wheel.advance();
            if !expired.is_empty() {
                assert_eq!(expired.len(), 1, "The {}th advance should trigger the second task", i + 2); // 第 {i + 2} 次前进应该触发第二个任务
                assert_eq!(expired[0].id, task_id_2);
                triggered = true;
                break;
            }
        }
        assert!(triggered, "Second task should trigger on tick 511"); // 第二个任务应该在第 511 个 tick 触发
        
        assert!(wheel.is_empty(), "All tasks should have been triggered"); // 所有任务都应该被触发
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
            let (task_with_notifier, _rx) = TimerTaskWithCompletionNotifier::from_timer_task(task, 32);
            let task_id = wheel.insert(task_with_notifier);
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
    fn test_task_id_uniqueness() {
        let mut wheel = Wheel::new(WheelConfig::default(), BatchConfig::default());
        
        // Insert multiple tasks, verify TaskId uniqueness (插入多个任务，验证 TaskId 唯一性)
        let mut task_ids = std::collections::HashSet::new();
        for _ in 0..100 {
            let callback = CallbackWrapper::new(|| async {});
            let task = TimerTask::new_oneshot(Duration::from_millis(100), Some(callback));
            let (task_with_notifier, _rx) = TimerTaskWithCompletionNotifier::from_timer_task(task, 32);
            let task_id = wheel.insert(task_with_notifier);
            
            assert!(task_ids.insert(task_id), "TaskId should be unique"); // TaskId 应该唯一
        }
        
        assert_eq!(task_ids.len(), 100);
    }

    #[test]
    fn test_periodic_task_basic() {
        use crate::task::CompletionReceiver;
        
        let mut wheel = Wheel::new(WheelConfig::default(), BatchConfig::default());
        
        // Insert periodic task with 50ms interval (插入 50毫秒间隔的周期性任务)
        let callback = CallbackWrapper::new(|| async {});
        let task = TimerTask::new_periodic(
            Duration::from_millis(50),  // initial delay
            Duration::from_millis(50),  // interval
            Some(callback),
        );
        let (task_with_notifier, completion_receiver) = TimerTaskWithCompletionNotifier::from_timer_task(task, 32);
        let task_id = wheel.insert(task_with_notifier);
        
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
        assert!(wheel.task_index.contains_key(&task_id), "Task should still be in index");
        
        // Advance another 5 ticks (50ms), task should trigger again
        // 再前进 5 个 tick (50毫秒)，任务应该再次触发
        for _ in 0..5 {
            wheel.advance();
        }
        
        // Check second notification
        // 检查第二次通知
        assert!(rx.try_recv().is_ok(), "Should receive second notification");
        assert!(!wheel.is_empty(), "Periodic task should still be in the wheel");
        
        // Cancel the periodic task
        // 取消周期性任务
        assert!(wheel.cancel(task_id), "Should be able to cancel periodic task");
        assert!(wheel.is_empty(), "Wheel should be empty after cancellation");
    }

    #[test]
    fn test_periodic_task_cancel() {
        use crate::task::{TaskCompletionReasonPeriodic, CompletionReceiver};
        
        let mut wheel = Wheel::new(WheelConfig::default(), BatchConfig::default());
        
        // Insert periodic task (插入周期性任务)
        let callback = CallbackWrapper::new(|| async {});
        let task = TimerTask::new_periodic(
            Duration::from_millis(100),
            Duration::from_millis(100),
            Some(callback),
        );
        let (task_with_notifier, completion_receiver) = TimerTaskWithCompletionNotifier::from_timer_task(task, 32);
        let task_id = wheel.insert(task_with_notifier);
        
        let mut rx = match completion_receiver {
            CompletionReceiver::Periodic(receiver) => receiver,
            _ => panic!("Expected periodic completion receiver"),
        };
        
        // Cancel immediately (立即取消)
        assert!(wheel.cancel(task_id), "Should successfully cancel");
        
        // Check cancellation notification
        // 检查取消通知
        if let Ok(reason) = rx.try_recv() {
            assert_eq!(reason, TaskCompletionReasonPeriodic::Cancelled, "Should receive Cancelled notification");
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
            assert!(expired.is_empty(), "No tasks should expire after cancellation");
        }
    }

    #[test]
    fn test_periodic_task_multiple_triggers() {
        use crate::task::CompletionReceiver;
        
        let mut wheel = Wheel::new(WheelConfig::default(), BatchConfig::default());
        
        // Insert periodic task with 30ms interval (插入 30毫秒间隔的周期性任务)
        let callback = CallbackWrapper::new(|| async {});
        let task = TimerTask::new_periodic(
            Duration::from_millis(30),
            Duration::from_millis(30),
            Some(callback),
        );
        let (task_with_notifier, completion_receiver) = TimerTaskWithCompletionNotifier::from_timer_task(task, 32);
        let task_id = wheel.insert(task_with_notifier);
        
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
        assert!(trigger_count >= 3, "Should trigger at least 3 times, got {}", trigger_count);
        
        // Cancel the task
        // 取消任务
        wheel.cancel(task_id);
    }

    #[test]
    fn test_periodic_task_cross_layer() {
        use crate::task::CompletionReceiver;
        
        let mut wheel = Wheel::new(WheelConfig::default(), BatchConfig::default());
        
        // Insert periodic task with long interval (exceeds L0 range)
        // 插入长间隔的周期性任务（超过 L0 范围）
        // L0 range: 512 slots * 10ms = 5120ms
        let callback = CallbackWrapper::new(|| async {});
        let task = TimerTask::new_periodic(
            Duration::from_secs(10),  // 10000ms > 5120ms
            Duration::from_secs(10),
            Some(callback),
        );
        let (task_with_notifier, completion_receiver) = TimerTaskWithCompletionNotifier::from_timer_task(task, 32);
        let task_id = wheel.insert(task_with_notifier);
        
        let mut rx = match completion_receiver {
            CompletionReceiver::Periodic(receiver) => receiver,
            _ => panic!("Expected periodic completion receiver"),
        };
        
        // Verify task is in L1 layer
        // 验证任务在 L1 层
        let location = wheel.task_index.get(&task_id).unwrap();
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
        assert!(wheel.task_index.contains_key(&task_id), "Task should be reinserted");
        let location = wheel.task_index.get(&task_id).unwrap();
        assert_eq!(location.level, 1, "Reinserted task should still be in L1");
        
        // Cancel the task
        // 取消任务
        wheel.cancel(task_id);
    }

    #[test]
    fn test_periodic_task_batch_cancel() {
        use crate::task::{TaskCompletionReasonPeriodic, CompletionReceiver};
        
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
            );
            let (task_with_notifier, completion_receiver) = TimerTaskWithCompletionNotifier::from_timer_task(task, 32);
            let task_id = wheel.insert(task_with_notifier);
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
                assert_eq!(reason, TaskCompletionReasonPeriodic::Cancelled);
            } else {
                panic!("Should receive cancellation notification");
            }
        }
        
        assert!(wheel.is_empty(), "Wheel should be empty");
    }

    #[test]
    fn test_periodic_task_with_initial_delay() {
        use crate::task::CompletionReceiver;
        
        let mut wheel = Wheel::new(WheelConfig::default(), BatchConfig::default());
        
        // Insert periodic task with different initial delay and interval
        // 插入具有不同初始延迟和间隔的周期性任务
        let callback = CallbackWrapper::new(|| async {});
        let task = TimerTask::new_periodic(
            Duration::from_millis(100),  // initial delay 100ms
            Duration::from_millis(50),   // interval 50ms
            Some(callback),
        );
        let (task_with_notifier, completion_receiver) = TimerTaskWithCompletionNotifier::from_timer_task(task, 32);
        let task_id = wheel.insert(task_with_notifier);
        
        let mut rx = match completion_receiver {
            CompletionReceiver::Periodic(receiver) => receiver,
            _ => panic!("Expected periodic completion receiver"),
        };
        
        // Advance 10 ticks (100ms), task should trigger for the first time
        // 前进 10 个 tick (100毫秒)，任务应该第一次触发
        for _ in 0..10 {
            wheel.advance();
        }
        
        assert!(rx.try_recv().is_ok(), "Should receive first notification after initial delay");
        
        // Advance another 5 ticks (50ms), task should trigger again (using interval)
        // 再前进 5 个 tick (50毫秒)，任务应该再次触发（使用间隔）
        for _ in 0..5 {
            wheel.advance();
        }
        
        assert!(rx.try_recv().is_ok(), "Should receive second notification after interval");
        
        // Cancel the task
        // 取消任务
        wheel.cancel(task_id);
    }
}

