//! werewolf-bridge: stdin/stdout JSON-RPC server 二进制入口
//!
//! 用法:
//!   echo '{"id":1,"method":"list_sessions","params":{}}' | werewolf-bridge
//!   echo '{"id":2,"method":"send_text","params":{"session":"s1","text":"hi"}}' | werewolf-bridge

use std::io::BufReader;

fn main() {
    // 初始化全局 bridge 状态（tokio runtime + rmux 连接池）
    werewolf_bridge::BridgeState::init();

    let mut reader = BufReader::new(std::io::stdin());
    loop {
        let Some(line) = werewolf_bridge::server::read_request_line(&mut reader) else {
            break;
        };
        let response = werewolf_bridge::server::handle_request(&line);
        println!("{response}");
    }
}
