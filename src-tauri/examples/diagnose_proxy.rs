use reqwest::blocking::Client;
use serde_json::json;
use std::time::Duration;

fn main() {
    println!("=== cc-switch 本地模型诊断工具 ===\n");

    let client = Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .unwrap();

    // 测试 1: 直接测试本地模型
    println!("【测试 1】直接测试本地模型服务");
    println!("URL: http://127.0.0.1:8168/v1/chat/completions");

    let openai_req = json!({
        "model": "qwen2.5-coder-32b-instruct-q4_k_m",
        "messages": [{"role": "user", "content": "Hello"}],
        "stream": false
    });

    match client
        .post("http://127.0.0.1:8168/v1/chat/completions")
        .header("Authorization", "Bearer xxxx")
        .header("Content-Type", "application/json")
        .json(&openai_req)
        .send()
    {
        Ok(resp) => {
            println!("✓ 状态码: {}", resp.status());
            if resp.status().is_success() {
                println!("✓ 本地模型服务正常\n");
            } else {
                println!("✗ 本地模型服务异常\n");
            }
        }
        Err(e) => {
            println!("✗ 无法连接到本地模型服务: {}", e);
            println!("  请确认服务正在运行: 127.0.0.1:8168\n");
            return;
        }
    }

    // 测试 2: 测试 cc-switch 代理
    println!("【测试 2】测试 cc-switch 代理");
    println!("URL: http://127.0.0.1:15721/v1/messages");

    let anthropic_req = json!({
        "model": "qwen2.5-coder-32b-instruct-q4_k_m",
        "max_tokens": 50,
        "messages": [{"role": "user", "content": "Hello"}],
        "stream": false
    });

    match client
        .post("http://127.0.0.1:15721/v1/messages")
        .header("Content-Type", "application/json")
        .header("anthropic-version", "2023-06-01")
        .json(&anthropic_req)
        .send()
    {
        Ok(resp) => {
            println!("状态码: {}", resp.status());
            let status = resp.status();

            match resp.text() {
                Ok(body) => {
                    if status.is_success() {
                        println!("✓ cc-switch 代理正常");
                        println!("响应: {}", body);
                    } else if status.as_u16() == 502 {
                        println!("✗ 502 Bad Gateway");
                        println!("错误响应: {}", body);
                        println!("\n可能原因:");
                        println!("1. Provider 配置未正确加载");
                        println!("2. apiFormat 未生效");
                        println!("3. cc-switch 无法连接到本地模型");
                    } else {
                        println!("✗ 请求失败");
                        println!("响应: {}", body);
                    }
                }
                Err(e) => {
                    println!("✗ 无法读取响应: {}", e);
                }
            }
        }
        Err(e) => {
            println!("✗ 无法连接到 cc-switch: {}", e);
            println!("  请确认 cc-switch 正在运行");
            println!("  默认端口: 15721");
        }
    }

    println!("\n【检查项】");
    println!("1. cc-switch 是否正在运行?");
    println!("   - 检查进程: tasklist | findstr cc-switch");
    println!("   - 检查端口: netstat -ano | findstr :15721");
    println!();
    println!("2. Provider 是否正确配置?");
    println!("   - 检查数据库: sqlite3 ~/.cc-switch/cc-switch.db \"SELECT id,name,meta FROM providers WHERE app_type='claude'\"");
    println!("   - 确认 meta.apiFormat = 'openai_chat'");
    println!();
    println!("3. cc-switch 日志在哪里?");
    println!("   - 开发模式: 查看终端输出");
    println!("   - 生产模式: 查看应用日志文件");
    println!("   - 关键字: '[Claude] >>> 请求 URL'");
    println!("   - 关键字: '连接失败' 或 '转发失败'");
}
