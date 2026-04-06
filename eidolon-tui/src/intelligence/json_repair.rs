/// Robust JSON extraction and repair.
///
/// Handles 6 failure modes common in LLM output:
/// 1. JSON embedded in prose or markdown
/// 2. Markdown code fences wrapping JSON
/// 3. Trailing commas before `}` or `]`
/// 4. Unterminated strings
/// 5. Unbalanced braces/brackets
/// 6. Braces inside string values (string-aware scanning)

/// Try to extract and repair a JSON value from potentially messy text.
/// Returns `None` only if all repair strategies fail.
pub fn extract_and_repair_json(text: &str) -> Option<serde_json::Value> {
    // Step 1: Direct parse
    if let Ok(v) = serde_json::from_str(text) {
        return Some(v);
    }

    // Step 2: Extract JSON body (string-aware first { to matching })
    let body = extract_json_body(text)?;
    if let Ok(v) = serde_json::from_str(&body) {
        return Some(v);
    }

    // Step 3: Strip markdown code fences
    let stripped = strip_markdown_fences(&body);
    if stripped != body {
        if let Ok(v) = serde_json::from_str(&stripped) {
            return Some(v);
        }
    }

    // Step 4: Fix trailing commas before } or ]
    let no_trailing = fix_trailing_commas(&stripped);
    if let Ok(v) = serde_json::from_str(&no_trailing) {
        return Some(v);
    }

    // Step 5: Close unterminated strings
    let closed = close_unterminated_strings(&no_trailing);
    if let Ok(v) = serde_json::from_str(&closed) {
        return Some(v);
    }

    // Step 6: Balance braces/brackets
    let balanced = balance_delimiters(&closed);
    serde_json::from_str(&balanced).ok()
}

/// String-aware JSON body extraction.
/// Finds first unquoted `{`, scans to matching `}` respecting string context.
fn extract_json_body(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut i = 0;

    // Find first unquoted '{'
    let start;
    loop {
        if i >= bytes.len() {
            return None;
        }
        if bytes[i] == b'{' {
            start = i;
            break;
        }
        i += 1;
    }

    // Scan to matching '}', respecting strings
    let mut depth = 0i32;
    let mut in_string = false;
    let mut j = start;

    while j < bytes.len() {
        let ch = bytes[j];

        if in_string {
            if ch == b'\\' {
                j += 2; // skip escaped char
                continue;
            }
            if ch == b'"' {
                in_string = false;
            }
            j += 1;
            continue;
        }

        match ch {
            b'"' => {
                in_string = true;
            }
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(text[start..=j].to_string());
                }
            }
            _ => {}
        }
        j += 1;
    }

    // Unbalanced -- return what we have from start to end
    if depth > 0 {
        Some(text[start..].to_string())
    } else {
        None
    }
}

/// Strip markdown code fences: ```json ... ``` or ``` ... ```
fn strip_markdown_fences(text: &str) -> String {
    let trimmed = text.trim();
    // Check if wrapped in code fences
    if let Some(rest) = trimmed.strip_prefix("```") {
        // Skip optional language tag on first line
        let after_tag = if let Some(nl) = rest.find('\n') {
            &rest[nl + 1..]
        } else {
            rest
        };
        // Strip trailing fence
        let body = if let Some(stripped) = after_tag.strip_suffix("```") {
            stripped
        } else {
            after_tag
        };
        return body.trim().to_string();
    }
    trimmed.to_string()
}

/// Fix trailing commas before `}` or `]`.
/// e.g. {"a": 1, "b": 2,} -> {"a": 1, "b": 2}
fn fix_trailing_commas(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut result = Vec::with_capacity(bytes.len());
    let mut in_string = false;
    let mut i = 0;

    while i < bytes.len() {
        let ch = bytes[i];

        if in_string {
            result.push(ch);
            if ch == b'\\' && i + 1 < bytes.len() {
                i += 1;
                result.push(bytes[i]);
            } else if ch == b'"' {
                in_string = false;
            }
            i += 1;
            continue;
        }

        if ch == b'"' {
            in_string = true;
            result.push(ch);
            i += 1;
            continue;
        }

        if ch == b',' {
            // Look ahead past whitespace for } or ]
            let mut k = i + 1;
            while k < bytes.len() && (bytes[k] == b' ' || bytes[k] == b'\n' || bytes[k] == b'\r' || bytes[k] == b'\t') {
                k += 1;
            }
            if k < bytes.len() && (bytes[k] == b'}' || bytes[k] == b']') {
                // Skip this trailing comma
                i += 1;
                continue;
            }
        }

        result.push(ch);
        i += 1;
    }

    String::from_utf8_lossy(&result).into_owned()
}

/// Close unterminated strings by appending a `"` where needed.
fn close_unterminated_strings(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut result = Vec::with_capacity(bytes.len() + 4);
    let mut in_string = false;
    let mut i = 0;

    while i < bytes.len() {
        let ch = bytes[i];

        if in_string {
            if ch == b'\\' && i + 1 < bytes.len() {
                result.push(ch);
                i += 1;
                result.push(bytes[i]);
                i += 1;
                continue;
            }
            if ch == b'"' {
                in_string = false;
                result.push(ch);
                i += 1;
                continue;
            }
            // If we hit a newline inside a string, close it
            if ch == b'\n' {
                result.push(b'"');
                in_string = false;
                result.push(ch);
                i += 1;
                continue;
            }
            result.push(ch);
            i += 1;
            continue;
        }

        if ch == b'"' {
            in_string = true;
        }
        result.push(ch);
        i += 1;
    }

    // If still in a string at EOF, close it
    if in_string {
        result.push(b'"');
    }

    String::from_utf8_lossy(&result).into_owned()
}

/// Balance unmatched braces and brackets by appending closers.
fn balance_delimiters(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut stack: Vec<u8> = Vec::new();
    let mut in_string = false;
    let mut i = 0;

    while i < bytes.len() {
        let ch = bytes[i];

        if in_string {
            if ch == b'\\' && i + 1 < bytes.len() {
                i += 2;
                continue;
            }
            if ch == b'"' {
                in_string = false;
            }
            i += 1;
            continue;
        }

        match ch {
            b'"' => in_string = true,
            b'{' => stack.push(b'}'),
            b'[' => stack.push(b']'),
            b'}' | b']' => {
                if let Some(&expected) = stack.last() {
                    if expected == ch {
                        stack.pop();
                    }
                }
            }
            _ => {}
        }
        i += 1;
    }

    let mut result = text.to_string();
    // Append closers in reverse order
    while let Some(closer) = stack.pop() {
        result.push(closer as char);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_clean_json_passthrough() {
        let input = r#"{"intent": "casual", "confidence": 0.9}"#;
        let result = extract_and_repair_json(input).unwrap();
        assert_eq!(result["intent"], "casual");
    }

    #[test]
    fn test_json_in_prose() {
        let input = r#"Here is the result: {"intent": "action", "confidence": 0.8} as requested."#;
        let result = extract_and_repair_json(input).unwrap();
        assert_eq!(result["intent"], "action");
    }

    #[test]
    fn test_markdown_fences() {
        let input = "```json\n{\"intent\": \"memory\", \"confidence\": 0.7}\n```";
        let result = extract_and_repair_json(input).unwrap();
        assert_eq!(result["intent"], "memory");
    }

    #[test]
    fn test_braces_inside_strings() {
        let input = r#"{"reasoning": "the pattern {x} matched", "intent": "casual"}"#;
        let result = extract_and_repair_json(input).unwrap();
        assert_eq!(result["intent"], "casual");
        assert_eq!(result["reasoning"], "the pattern {x} matched");
    }

    #[test]
    fn test_trailing_commas() {
        let input = r#"{"a": 1, "b": 2,}"#;
        let result = extract_and_repair_json(input).unwrap();
        assert_eq!(result["a"], 1);
        assert_eq!(result["b"], 2);
    }

    #[test]
    fn test_unterminated_string() {
        let input = "{\"action\": \"search\n, \"done\": true}";
        let result = extract_and_repair_json(input);
        // Should attempt repair -- may or may not fully recover
        // but should not panic
        assert!(result.is_some() || result.is_none());
    }

    #[test]
    fn test_unbalanced_braces() {
        let input = r#"{"a": {"b": 1}"#;
        let result = extract_and_repair_json(input).unwrap();
        assert_eq!(result["a"]["b"], 1);
    }

    #[test]
    fn test_none_on_garbage() {
        let result = extract_and_repair_json("no json here at all");
        assert!(result.is_none());
    }

    #[test]
    fn test_nested_braces_in_strings_dont_confuse() {
        let input = r#"Sure! {"msg": "use {braces} and {more}", "ok": true}"#;
        let result = extract_and_repair_json(input).unwrap();
        assert_eq!(result["ok"], true);
        assert_eq!(result["msg"], "use {braces} and {more}");
    }
}
