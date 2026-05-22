//! Bridge 运行时状态 — 持有 tokio runtime 和 rmux 连接
//!
//! 每个 bridge 进程生命周期内共享一个 Rmux 连接，
//! 所有 JSON-RPC handler 通过 block_on 调用异步 SDK。

use std::sync::{OnceLock, Mutex};
use std::time::Duration;

use rmux_sdk::Rmux;

/// 全局共享的 bridge 状态（进程级单例）
static BRIDGE_STATE: OnceLock<BridgeState> = OnceLock::new();

/// Bridge 进程状态，持有 tokio runtime 和 rmux 连接（Arc 包装以支持 Clone）
pub struct BridgeState {
    runtime: tokio::runtime::Runtime,
    rmux: Mutex<Option<std::sync::Arc<Rmux>>>,
}

impl BridgeState {
    /// 初始化全局 bridge 状态（main.rs 入口调用一次）
    pub fn init() -> &'static Self {
        BRIDGE_STATE.get_or_init(|| {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("failed to create tokio runtime");
            BridgeState { runtime, rmux: Mutex::new(None) }
        })
    }

    /// 获取或初始化 Rmux 连接（懒连接，返回 Arc 克隆）
    pub fn get_rmux(&self) -> Result<std::sync::Arc<Rmux>, String> {
        let mut guard = self.rmux.lock().map_err(|e| format!("mutex poisoned: {e}"))?;
        if guard.is_none() {
            let rmux = self
                .runtime
                .block_on(Rmux::builder()
                    .default_timeout(Duration::from_secs(10))
                    .connect_or_start())
                .map_err(|e| format!("rmux connect failed: {e}"))?;
            *guard = Some(std::sync::Arc::new(rmux));
        }
        Ok(guard.as_ref().unwrap().clone())
    }

    /// 在 tokio runtime 上阻塞执行异步操作
    pub fn block_on<F, T>(&self, future: F) -> T
    where
        F: std::future::Future<Output = T>,
    {
        self.runtime.block_on(future)
    }
}

/// 获取全局 bridge 状态引用
pub fn state() -> &'static BridgeState {
    BRIDGE_STATE
        .get()
        .expect("BridgeState not initialized — call BridgeState::init() first")
}
