use crate::{TimerWheel, TimerTask};
use crate::config::ServiceConfig;
use crate::task::CallbackWrapper;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

#[tokio::test]
async fn test_schedule_once_batch_direct() {
    let timer = TimerWheel::with_defaults();
    let mut service = timer.create_service(ServiceConfig::default());
    let counter = Arc::new(AtomicU32::new(0));

    // Schedule timers directly through service
    // 直接通过服务调度定时器
    let handles = service.allocate_handles(3);
    let tasks: Vec<_> = (0..3)
        .map(|_| {
            let counter = Arc::clone(&counter);
            TimerTask::new_oneshot(Duration::from_millis(50), Some(CallbackWrapper::new(move || {
                let counter = Arc::clone(&counter);
                async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                }
            })))
        })
        .collect();

    service.register_batch(handles, tasks).unwrap();

    // Receive all timeout notifications
    // 接收所有超时通知
    let mut received_count = 0;
    let rx = service.take_receiver().unwrap();
    
    while received_count < 3 {
        match tokio::time::timeout(Duration::from_millis(200), rx.recv()).await {
            Ok(Some(_task_id)) => {
                received_count += 1;
            }
            Ok(None) => break,
            Err(_) => break,
        }
    }

    assert_eq!(received_count, 3);
    
    // Wait for callback to execute
    // 等待回调执行
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert_eq!(counter.load(Ordering::SeqCst), 3);
}

