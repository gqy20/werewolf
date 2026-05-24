//! BridgeState 单元测试
//!
//! 测试运行时初始化、懒连接、block_on 调度。

#[test]
fn test_init_returns_state() {
    let _s = BridgeState::init();
    // rmux 未连接（需要真实 daemon）
}

#[test]
fn test_state_is_singleton() {
    let a = BridgeState::init();
    let b = BridgeState::init();
    assert_eq!(a as *const _, b as *const _);
}

#[test]
fn test_block_on_resolves_future() {
    let s = BridgeState::init();
    let result: u32 = s.block_on(async { 42 });
    assert_eq!(result, 42);
}

#[test]
fn test_block_on_handles_async_block() {
    use std::sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    };
    let s = BridgeState::init();
    let counter = Arc::new(AtomicUsize::new(0));
    let c = counter.clone();
    s.block_on(async move {
        c.fetch_add(1, Ordering::SeqCst);
        tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        c.fetch_add(1, Ordering::SeqCst);
    });
    assert_eq!(counter.load(Ordering::SeqCst), 2);
}

#[test]
fn test_state_global_access() {
    let _s = BridgeState::init(); // init first
    let s = crate::bridge_state::state(); // global access
    let result: i32 = s.block_on(async { -7 });
    assert_eq!(result, -7);
}

#[cfg(test)]
use crate::bridge_state::BridgeState;
