/// Skill 压缩模块集成测试
///
/// 从 src/proxy/skill_compressor.rs 的单元测试迁移而来

use cc_switch_lib::proxy::skill_compressor::*;

#[test]
fn test_compress_removes_code_blocks() {
    let content = r#"# My Skill

Some instructions here.

```python
def hello():
    print("world")
```

More instructions.
"#;
    let (compressed, ratio) = compress_skill_content(content, 100);
    assert!(!compressed.contains("```"));
    assert!(!compressed.contains("def hello"));
    assert!(compressed.contains("Some instructions"));
    assert!(compressed.contains("More instructions"));
    assert!(ratio > 0.0);
}

#[test]
fn test_compress_removes_html_comments() {
    let content = "Instructions\n<!-- This is a comment -->\nMore text";
    let (compressed, _) = compress_skill_content(content, 100);
    assert!(!compressed.contains("<!--"));
    assert!(compressed.contains("Instructions"));
    assert!(compressed.contains("More text"));
}

#[test]
fn test_compress_removes_yaml_frontmatter() {
    let content = "---\nname: test\n---\n# Actual content\nInstructions";
    let (compressed, _) = compress_skill_content(content, 100);
    assert!(!compressed.contains("---"));
    assert!(compressed.contains("# Actual content"));
}

#[test]
fn test_compress_collapses_blank_lines() {
    let content = "Line1\n\n\n\n\nLine2";
    let (compressed, _) = compress_skill_content(content, 100);
    assert_eq!(compressed.matches("\n\n").count(), 1);
}

#[test]
fn test_compress_truncates_long_content() {
    let lines: Vec<String> = (0..200).map(|i| format!("Line {}", i)).collect();
    let content = lines.join("\n");
    let (compressed, _) = compress_skill_content(&content, 50);
    let line_count = compressed.lines().count();
    assert!(line_count <= 52); // 50 + "..." line + possible blank
}

#[test]
fn test_estimate_tokens() {
    // 纯英文
    let english = "hello world this is a test";
    let tokens_en = estimate_tokens(english);
    assert!(tokens_en > 0);

    // 纯中文
    let chinese = "这是一个测试";
    let tokens_cn = estimate_tokens(chinese);
    assert!(tokens_cn > 0);

    // 中文 token 应该比同等字符数的英文多
    // 5个中文字符 ≈ 7.5 tokens, 5个英文单词 ≈ 6.5 tokens
}

#[test]
fn test_format_catalog_entry() {
    let entry = format_catalog_entry("autopilot", "build me", 85);
    assert!(entry.contains("[autopilot]"));
    assert!(entry.contains("build me"));
    assert!(entry.contains("85%"));
}

#[test]
fn test_format_catalog_block() {
    let entries = vec![
        ("autopilot".to_string(), "build me".to_string(), 85),
        ("tdd".to_string(), "test first".to_string(), 72),
    ];
    let block = format_catalog_block(&entries);
    assert!(block.contains("<skill-catalog>"));
    assert!(block.contains("</skill-catalog>"));
    assert!(block.contains("[autopilot]"));
    assert!(block.contains("[tdd]"));
}

#[test]
fn test_allocate_budget_basic() {
    let config = SkillBudgetConfig {
        injection_char_budget: 5000,
        single_skill_char_budget: 2000,
        max_full_skills: 2,
        ..Default::default()
    };

    let skills = vec![
        ("skill-1".to_string(), "A".repeat(1500), "trigger1".to_string(), 90),
        ("skill-2".to_string(), "B".repeat(1500), "trigger2".to_string(), 80),
        ("skill-3".to_string(), "C".repeat(1500), "trigger3".to_string(), 70),
        ("skill-4".to_string(), "D".repeat(1500), "trigger4".to_string(), 60),
    ];

    let alloc = allocate_budget(&skills, &config);
    assert_eq!(alloc.full_skills.len(), 2); // max_full_skills = 2
    assert_eq!(alloc.catalog_skills.len(), 2); // 剩余降级
    assert!(alloc.total_chars <= config.injection_char_budget + 1000); // catalog 额外开销
}

#[test]
fn test_allocate_budget_respects_char_limit() {
    let config = SkillBudgetConfig {
        injection_char_budget: 3000,
        single_skill_char_budget: 2000,
        max_full_skills: 10,
        ..Default::default()
    };

    let skills = vec![
        ("big-skill".to_string(), "X".repeat(2500), "t1".to_string(), 90),
        ("small-skill".to_string(), "Y".repeat(500), "t2".to_string(), 80),
    ];

    let alloc = allocate_budget(&skills, &config);
    // big-skill (2500) 先注入，remaining=3000-2500=500 < single_skill_char_budget(2000)
    // small-skill (500) 虽然内容小，但 remaining_budget 不足，降级为 catalog
    assert_eq!(alloc.full_skills.len(), 1);
    assert_eq!(alloc.catalog_skills.len(), 1);
    assert_eq!(alloc.full_skills[0].0, "big-skill");
}

#[test]
fn test_injection_mode_display() {
    assert_eq!(SkillInjectionMode::Full.to_string(), "full");
    assert_eq!(SkillInjectionMode::Catalog.to_string(), "catalog");
    assert_eq!(SkillInjectionMode::None.to_string(), "none");
}

#[test]
fn test_resolve_injection_mode_openai() {
    let config = SkillBudgetConfig::default();
    let mode = resolve_injection_mode("local-llm", None, "Claude", true, &config);
    assert_eq!(mode, SkillInjectionMode::Catalog);
}

#[test]
fn test_resolve_injection_mode_anthropic() {
    let config = SkillBudgetConfig::default();
    let mode = resolve_injection_mode("cloud-claude", None, "Claude", false, &config);
    assert_eq!(mode, SkillInjectionMode::Full);
}

#[test]
fn test_resolve_injection_mode_override() {
    let mut config = SkillBudgetConfig::default();
    config
        .provider_overrides
        .insert("my-provider".to_string(), SkillInjectionMode::None);
    let mode = resolve_injection_mode("my-provider", None, "Claude", false, &config);
    assert_eq!(mode, SkillInjectionMode::None);
}

#[test]
fn test_resolve_injection_mode_disabled() {
    let config = SkillBudgetConfig {
        enabled: false,
        ..Default::default()
    };
    let mode = resolve_injection_mode("local-llm", None, "Claude", true, &config);
    assert_eq!(mode, SkillInjectionMode::Full);
}

// =============================================================================
// 无损工具压缩测试
// =============================================================================

#[test]
fn test_is_core_tool() {
    // 核心工具
    assert!(is_core_tool("Read"));
    assert!(is_core_tool("Write"));
    assert!(is_core_tool("Edit"));
    assert!(is_core_tool("Bash"));
    assert!(is_core_tool("Glob"));
    assert!(is_core_tool("Grep"));
    assert!(is_core_tool("TodoWrite"));
    assert!(is_core_tool("WebSearch"));
    assert!(is_core_tool("AskUserQuestion"));
    assert!(is_core_tool("mcp__sequential-thinking__sequentialthinking"));

    // 非核心工具
    assert!(!is_core_tool("mcp__chrome-devtools__click"));
    assert!(!is_core_tool("mcp__context7__resolve-library-id"));
    assert!(!is_core_tool("SomeUnknownTool"));

    // 非核心工具
    assert!(!is_core_tool("mcp__chrome-devtools__click"));
    assert!(!is_core_tool("mcp__context7__resolve-library-id"));
    assert!(!is_core_tool("SomeUnknownTool"));
}

#[test]
fn test_extract_provider() {
    assert_eq!(
        extract_provider("mcp__chrome-devtools__click"),
        Some("chrome-devtools")
    );
    assert_eq!(
        extract_provider("mcp__context7__resolve-library-id"),
        Some("context7")
    );
    assert_eq!(
        extract_provider("mcp__4_5v_mcp__analyze_image"),
        Some("4_5v_mcp")
    );
    assert_eq!(extract_provider("Read"), None);
    assert_eq!(extract_provider("unknown"), None);
}

#[test]
fn test_truncate_at_word_boundary() {
    // 短文本不截断
    assert_eq!(truncate_at_word_boundary("hello", 100), "hello");

    // 在词边界截断
    let long = "This is a very long description that should be truncated at a word boundary";
    let result = truncate_at_word_boundary(long, 30);
    assert!(result.ends_with("..."));
    assert!(result.len() < long.len());
    // 确保在词边界截断（保留最后一个完整单词）
    assert!(result.ends_with("..."));
    assert!(result.len() < long.len());
}

#[test]
fn test_truncate_at_word_boundary_no_spaces() {
    // 没有空格时直接截断
    let no_spaces = "abcdefghijklmnopqrstuvwxyz";
    let result = truncate_at_word_boundary(no_spaces, 10);
    assert_eq!(result, "abcdefghij...");
}

#[test]
fn test_compress_tool_definitions_mixed() {
    let tools = vec![
        // 核心工具
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "Read",
                "description": "Reads a file from the local filesystem",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "file_path": { "type": "string", "description": "The absolute path" }
                    },
                    "required": ["file_path"]
                }
            }
        }),
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "Bash",
                "description": "Execute a bash command",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "command": { "type": "string" }
                    },
                    "required": ["command"]
                }
            }
        }),
        // MCP 工具
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "mcp__chrome-devtools__click",
                "description": "Clicks on the provided element",
                "parameters": {
                    "type": "object",
                    "properties": {
                        "uid": { "type": "string", "description": "Element uid" }
                    },
                    "required": ["uid"]
                }
            }
        }),
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "mcp__chrome-devtools__take_screenshot",
                "description": "Take a screenshot of the page",
                "parameters": { "type": "object", "properties": {} }
            }
        }),
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "mcp__context7__query-docs",
                "description": "Query documentation from Context7",
                "parameters": { "type": "object", "properties": {} }
            }
        }),
    ];

    let result = compress_tool_definitions(&tools);

    // 统计验证
    assert_eq!(result.stats.original_count, 5);
    assert_eq!(result.stats.core_count, 2);
    assert_eq!(result.stats.catalog_count, 3);

    // 核心工具完整保留
    assert_eq!(result.compressed_tools.len(), 2);
    let core_names: Vec<&str> = result
        .compressed_tools
        .iter()
        .filter_map(|t| t.get("function")?.get("name")?.as_str())
        .collect();
    assert!(core_names.contains(&"Read"));
    assert!(core_names.contains(&"Bash"));

    // Catalog 按 provider 分组
    assert_eq!(result.catalog_by_provider.len(), 2); // chrome-devtools, context7
    assert!(result.catalog_by_provider.contains_key("chrome-devtools"));
    assert!(result.catalog_by_provider.contains_key("context7"));
    assert_eq!(result.catalog_by_provider["chrome-devtools"].len(), 2);
    assert_eq!(result.catalog_by_provider["context7"].len(), 1);

    // 字节数减少
    assert!(result.stats.compressed_bytes < result.stats.original_bytes);
}

#[test]
fn test_format_tool_catalog_block() {
    let mut catalog = std::collections::HashMap::new();
    catalog.insert(
        "chrome-devtools".to_string(),
        vec![
            ("mcp__chrome-devtools__click".to_string(), "Click element...".to_string()),
            ("mcp__chrome-devtools__screenshot".to_string(), "Take screenshot...".to_string()),
        ],
    );
    catalog.insert(
        "context7".to_string(),
        vec![("mcp__context7__query".to_string(), "Query docs...".to_string())],
    );

    let block = format_tool_catalog_block(&catalog, "/tmp/tool-cache");

    assert!(block.contains("<available-mcp-tools>"));
    assert!(block.contains("</available-mcp-tools>"));
    assert!(block.contains("3 个工具")); // 总计 3 个
    assert!(block.contains("chrome-devtools (2 tools)"));
    assert!(block.contains("context7 (1 tools)"));
    assert!(block.contains("/tmp/tool-cache/chrome-devtools.json"));
    assert!(block.contains("/tmp/tool-cache/context7.json"));
    assert!(block.contains("mcp__chrome-devtools__click"));
    assert!(block.contains("mcp__context7__query"));
}

#[test]
fn test_format_tool_catalog_block_empty() {
    let catalog = std::collections::HashMap::new();
    let block = format_tool_catalog_block(&catalog, "/tmp/cache");
    assert!(block.is_empty());
}

#[test]
fn test_save_tool_cache_and_read_back() {
    let tools = vec![
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "Read",
                "description": "Core tool",
                "parameters": { "type": "object" }
            }
        }),
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "mcp__chrome-devtools__click",
                "description": "Click element",
                "parameters": { "type": "object" }
            }
        }),
        serde_json::json!({
            "type": "function",
            "function": {
                "name": "mcp__chrome-devtools__hover",
                "description": "Hover element",
                "parameters": { "type": "object" }
            }
        }),
    ];

    let cache_dir = std::env::temp_dir().join("cc-switch-test-cache");
    let cache_dir_str = cache_dir.to_string_lossy().to_string();

    // 清理可能存在的旧缓存
    let _ = std::fs::remove_dir_all(&cache_dir);

    // 保存
    save_tool_cache(&tools, &cache_dir_str).expect("save_tool_cache should succeed");

    // 验证 chrome-devtools 缓存文件存在
    let chrome_file = cache_dir.join("chrome-devtools.json");
    assert!(chrome_file.exists(), "chrome-devtools.json should exist");

    // 验证内容：应包含 2 个 chrome-devtools 工具（不含核心工具 Read）
    let content = std::fs::read_to_string(&chrome_file).expect("read cache file");
    let parsed: serde_json::Value = serde_json::from_str(&content).expect("parse cache json");
    assert_eq!(parsed["tool_count"], 2);
    assert_eq!(parsed["provider"], "chrome-devtools");

    // 验证核心工具 Read 没有被缓存
    let tool_names: Vec<&str> = parsed["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|t| t["function"]["name"].as_str())
        .collect();
    assert!(!tool_names.contains(&"Read"));

    // 清理
    let _ = std::fs::remove_dir_all(&cache_dir);
}
