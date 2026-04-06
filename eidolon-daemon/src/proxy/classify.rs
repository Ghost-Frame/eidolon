use regex::Regex;
use std::sync::LazyLock;
use super::Category;

/// High-confidence rule-based pattern triggers.
/// Returns Some(category) for strong matches, None for ambiguous cases that need LLM.
pub fn classify_rule(content: &str) -> Option<Category> {
    let lower = content.to_lowercase();

    // Auto-skip: tool calls, code output, error traces, very short content
    if content.len() < 10 {
        return Some(Category::Skip);
    }
    if looks_like_code_or_tool_output(content) {
        return Some(Category::Skip);
    }

    // Auto-capture patterns (high confidence)
    for pattern in DECISION_PATTERNS.iter() {
        if pattern.is_match(&lower) {
            return Some(Category::Decision);
        }
    }
    for pattern in PREFERENCE_PATTERNS.iter() {
        if pattern.is_match(&lower) {
            return Some(Category::Preference);
        }
    }
    for pattern in STATE_PATTERNS.iter() {
        if pattern.is_match(&lower) {
            return Some(Category::StateChange);
        }
    }
    for pattern in ISSUE_PATTERNS.iter() {
        if pattern.is_match(&lower) {
            return Some(Category::Issue);
        }
    }

    // No strong signal -- needs LLM
    None
}

/// Classify content using Ollama LLM for nuanced cases.
/// Falls back to Skip on any error.
pub async fn classify_llm(
    client: &reqwest::Client,
    ollama_url: &str,
    model: &str,
    content: &str,
) -> Category {
    // Truncate very long content for classification
    let truncated = if content.len() > 2000 {
        &content[..2000]
    } else {
        content
    };

    let prompt = format!(
        "Classify this conversation turn. Is it worth remembering long-term?\n\
         Categories: fact, decision, preference, state_change, discovery, issue, skip\n\
         Most turns should be \"skip\" -- routine code, tool output, commands.\n\
         Only flag things that would be useful weeks or months from now.\n\n\
         Turn: {}\n\n\
         Category (one word):",
        truncated
    );

    let payload = serde_json::json!({
        "model": model,
        "prompt": prompt,
        "stream": false,
        "options": {
            "temperature": 0.1,
            "num_predict": 10,
        }
    });

    let url = format!("{}/api/generate", ollama_url);

    let resp = match client
        .post(&url)
        .json(&payload)
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!("proxy classify: ollama request failed: {}", e);
            return Category::Skip;
        }
    };

    if !resp.status().is_success() {
        tracing::warn!("proxy classify: ollama returned {}", resp.status());
        return Category::Skip;
    }

    let body: serde_json::Value = match resp.json().await {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("proxy classify: ollama parse failed: {}", e);
            return Category::Skip;
        }
    };

    let response_text = body
        .get("response")
        .and_then(|v| v.as_str())
        .unwrap_or("skip");

    Category::from_str_loose(response_text)
}

/// Extract a concise memory from longer content using Ollama.
/// Returns None on failure.
pub async fn extract_memory(
    client: &reqwest::Client,
    ollama_url: &str,
    model: &str,
    content: &str,
) -> Option<String> {
    // Short content doesn't need extraction
    if content.len() < 200 {
        return Some(content.to_string());
    }

    let truncated = if content.len() > 3000 {
        &content[..3000]
    } else {
        content
    };

    let prompt = format!(
        "Extract the key fact, decision, or preference from this text.\n\
         Write a single clear sentence that would be useful as a standalone memory.\n\n\
         Text: {}\n\n\
         Memory:",
        truncated
    );

    let payload = serde_json::json!({
        "model": model,
        "prompt": prompt,
        "stream": false,
        "options": {
            "temperature": 0.2,
            "num_predict": 100,
        }
    });

    let url = format!("{}/api/generate", ollama_url);

    let resp = client
        .post(&url)
        .json(&payload)
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await
        .ok()?;

    if !resp.status().is_success() {
        return None;
    }

    let body: serde_json::Value = resp.json().await.ok()?;
    let text = body.get("response")?.as_str()?.trim().to_string();

    if text.is_empty() {
        None
    } else {
        Some(text)
    }
}

fn looks_like_code_or_tool_output(content: &str) -> bool {
    // Lines starting with common code/tool patterns
    let lines: Vec<&str> = content.lines().collect();
    if lines.is_empty() {
        return false;
    }

    // If most lines look like code/output, skip
    let code_indicators = lines.iter().filter(|line| {
        let trimmed = line.trim();
        trimmed.starts_with("```")
            || trimmed.starts_with("$ ")
            || trimmed.starts_with("> ")
            || trimmed.starts_with("//")
            || trimmed.starts_with('#')
            || trimmed.starts_with("import ")
            || trimmed.starts_with("use ")
            || trimmed.starts_with("fn ")
            || trimmed.starts_with("pub ")
            || trimmed.starts_with("const ")
            || trimmed.starts_with("let ")
            || trimmed.starts_with("async ")
            || trimmed.starts_with("error[")
            || trimmed.starts_with("warning[")
            || trimmed.starts_with("Compiling ")
            || trimmed.starts_with("Running ")
            || trimmed.contains(" -> ")
            || trimmed.contains("::")
            || trimmed.contains("();")
    }).count();

    // If more than 60% of lines look like code, it's code
    code_indicators as f32 / lines.len() as f32 > 0.6
}

// Pattern sets for rule-based classification
static DECISION_PATTERNS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    vec![
        Regex::new(r"from now on").unwrap(),
        Regex::new(r"i('ve| have) decided").unwrap(),
        Regex::new(r"we('re| are) going (with|to use)").unwrap(),
        Regex::new(r"let('s| us) (go with|use|switch to)").unwrap(),
        Regex::new(r"the decision is").unwrap(),
        Regex::new(r"we('ll| will) (use|go with|switch)").unwrap(),
        Regex::new(r"don't ever|never (again|do)").unwrap(),
        Regex::new(r"always use|always do").unwrap(),
        Regex::new(r"rule:\s").unwrap(),
    ]
});

static PREFERENCE_PATTERNS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    vec![
        Regex::new(r"i (prefer|like|want|hate|dislike|love)").unwrap(),
        Regex::new(r"i('d| would) rather").unwrap(),
        Regex::new(r"remember that i").unwrap(),
        Regex::new(r"keep in mind").unwrap(),
    ]
});

static STATE_PATTERNS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    vec![
        Regex::new(r"(deployed|migrated|installed|configured|moved) (to|on|at)").unwrap(),
        Regex::new(r"(is|are) now (running|live|deployed|active)").unwrap(),
        Regex::new(r"switched (to|from)").unwrap(),
        Regex::new(r"(server|service|container) .{0,30} (is|running|stopped|crashed)").unwrap(),
    ]
});

static ISSUE_PATTERNS: LazyLock<Vec<Regex>> = LazyLock::new(|| {
    vec![
        Regex::new(r"known (bug|issue|problem)").unwrap(),
        Regex::new(r"this (broke|breaks|is broken|crashed|fails)").unwrap(),
        Regex::new(r"workaround:").unwrap(),
        Regex::new(r"do not .{0,20} (or|because) .{0,30} (break|crash|fail|lock)").unwrap(),
    ]
});

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decision_patterns() {
        assert_eq!(classify_rule("from now on we use rust"), Some(Category::Decision));
        assert_eq!(classify_rule("i've decided to switch to postgres"), Some(Category::Decision));
        assert_eq!(classify_rule("we're going with approach A"), Some(Category::Decision));
    }

    #[test]
    fn test_preference_patterns() {
        assert_eq!(classify_rule("i prefer dark mode for everything"), Some(Category::Preference));
        assert_eq!(classify_rule("i hate small talk; get to the point"), Some(Category::Preference));
        assert_eq!(classify_rule("remember that i like concise answers"), Some(Category::Preference));
    }

    #[test]
    fn test_skip_patterns() {
        assert_eq!(classify_rule("ok"), Some(Category::Skip)); // too short
        // Code detection is tested separately in test_code_detection
    }

    #[test]
    fn test_ambiguous() {
        // Should return None for ambiguous content needing LLM
        assert_eq!(classify_rule("yeah let's not do that again, it was painful"), None);
        assert_eq!(classify_rule("the migration went well overall"), None);
    }

    #[test]
    fn test_code_detection() {
        // 5 lines, 4 match code patterns (use, fn, let, contains ::) = 80%
        let code = "use std::collections::HashMap;\nfn main() {\n    let x = HashMap::new();\n    let y = x.len();\n    println!(\"{}\", y);\n}";
        assert!(looks_like_code_or_tool_output(code));

        let prose = "I think we should switch to using a different database for this project because the current one is too slow.";
        assert!(!looks_like_code_or_tool_output(prose));
    }
}
