//! werewolf-bridge: Python ↔ rmux SDK JSON-RPC bridge
//!
//! TDD 开发顺序:
//! 1. protocol — JSON-RPC 类型定义 ✅
//! 2. session — Session 管理（new/list/exists/kill）
//! 3. pane — Pane 操作（send_text/capture/wait_for）
//! 4. capture — 结构化输出提取
//! 5. server — stdin/stdout RPC 服务端

pub mod bridge_state;
pub mod capture;
pub mod pane;
pub mod protocol;
pub mod server;
pub mod session;

pub use bridge_state::BridgeState;
pub use protocol::{BridgeError, BridgeRequest, BridgeResponse};

#[cfg(test)]
mod bridge_state_tests;
#[cfg(test)]
mod capture_tests;
#[cfg(test)]
mod pane_tests;
#[cfg(test)]
mod protocol_tests;
#[cfg(test)]
mod server_tests;
#[cfg(test)]
mod session_tests;
