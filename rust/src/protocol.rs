//! JSON-RPC 2.0 协议类型定义
//!
//! Bridge 使用 stdin/stdout JSON-RPC 与 Python 端通信。
//! 每条消息一行 JSON（newline-delimited JSON）。

use serde::{Deserialize, Serialize};

/// 来自 Python 的请求
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BridgeRequest {
    pub id: u64,
    pub method: String,
    pub params: serde_json::Value,
}

/// 返回给 Python 的响应
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BridgeResponse {
    pub id: u64,
    pub result: Option<serde_json::Value>,
    pub error: Option<BridgeError>,
}

impl BridgeResponse {
    pub fn ok(id: u64, result: serde_json::Value) -> Self {
        Self { id, result: Some(result), error: None }
    }

    pub fn error(err: BridgeError) -> Self {
        Self { id: err.id, result: None, error: Some(err) }
    }
}

/// RPC 错误
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BridgeError {
    #[serde(default)]
    pub id: u64,
    pub code: i32,
    pub message: String,
}

impl BridgeError {
    pub fn new(code: i32, message: impl Into<String>) -> Self {
        Self { id: 0, code, message: message.into() }
    }

    pub fn id(mut self, id: u64) -> Self {
        self.id = id;
        self
    }

    /// 标准错误码
    pub const INVALID_PARAMS: i32 = -32602;
    pub const INTERNAL_ERROR: i32 = -32603;
    pub const METHOD_NOT_FOUND: i32 = -32601;
    pub const PARSE_ERROR: i32 = -32700;
}
