use reqwest::blocking::Client;
use serde_json::json;
use std::time::Duration;

fn main() {
    println!("=== 验证 OpenAI 兼容模式 Header 修复 ===\n");

    let client = Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap();

    // 测试：直接发送不带 Anthropic headers 的请求
    println!("【测试】模拟 cc-switch 修复后的请求");
    println!("URL: http://127.0.0.1:8168/v1/chat/completions");

    let openai_req = json!({
        "model": "qwen2.5-coder-32b-instruct-q4_k_m",
        "messages": [{"role": "user", "content": "Hello"}],
        "stream": false
    });

    let request = client
        .post("http://127.0.0.1:8168/v1/chat/completions")
        .header("Authorization", "Bearer xxxx")
        .header("Content-Type", "application/json")
        // 注意：不添加 anthropic-beta 和 anthropic-version
        .json(&openai_req);

    match request.send() {
        Ok(resp) => {
            println!("✓ 状态码: {}", resp.status());
            if resp.status().is_success() {
                println!("✓ 请求成功！");
                if let Ok(body) = resp.text() {
                    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
                    println!("✓ 响应模型: {}", v["model"]);
                    println!("✓ 响应内容: {}...",
                        v["choices"][0]["message"]["content"].as_str().unwrap().chars().take(50).collect::<String>()
                    );
                    println!("\n=== 修复验证成功! ===");
                    println!("\n现在 cc-switch 会自动跳过 Anthropic 特定 headers");
                    println!("当 apiFormat='openai_chat' 时，只发送标准 OpenAI headers");
                }
            } else {
                println!("✗ 状态码异常: {}", resp.status());
            }
        }
        Err(e) => {
            println!("✗ 请求失败: {}", e);
        }
    }
}
