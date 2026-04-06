use std::sync::Arc;
use serde::Deserialize;
use crate::llm::client::LlmClient;

// ---------------------------------------------------------------------------
// Output types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct SynthesizedResponse {
    pub summary: String,
    pub files_changed: Vec<String>,
    pub key_actions: Vec<String>,
    pub warnings: Vec<String>,
    pub raw_output: String,
}

impl SynthesizedResponse {
    /// Fallback constructor -- puts raw output as summary with empty structured fields.
    pub fn passthrough(raw: &str) -> Self {
        Self {
            summary: raw.to_string(),
            files_changed: Vec::new(),
            key_actions: Vec::new(),
            warnings: Vec::new(),
            raw_output: raw.to_string(),
        }
    }

    /// Format the structured fields into a human-readable string for display.
    pub fn format_for_display(&self) -> String {
        let mut out = String::new();

        out.push_str("Summary\n");
        out.push_str("-------\n");
        out.push_str(&self.summary);
        out.push('\n');

        if !self.files_changed.is_empty() {
            out.push('\n');
            out.push_str("Files Changed\n");
            out.push_str("-------------\n");
            for f in &self.files_changed {
                out.push_str("  - ");
                out.push_str(f);
                out.push('\n');
            }
        }

        if !self.key_actions.is_empty() {
            out.push('\n');
            out.push_str("Key Actions\n");
            out.push_str("-----------\n");
            for a in &self.key_actions {
                out.push_str("  - ");
                out.push_str(a);
                out.push('\n');
            }
        }

        if !self.warnings.is_empty() {
            out.push('\n');
            out.push_str("Warnings\n");
            out.push_str("--------\n");
            for w in &self.warnings {
                out.push_str("  ! ");
                out.push_str(w);
                out.push('\n');
            }
        }

        out
    }
}

// ---------------------------------------------------------------------------
// LLM response shape (JSON from grammar-constrained output)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
struct LlmSynthesis {
    summary: String,
    files_changed: Vec<String>,
    key_actions: Vec<String>,
    warnings: Vec<String>,
}

// ---------------------------------------------------------------------------
// ANSI stripping
// ---------------------------------------------------------------------------

/// Remove ANSI escape sequences from a string.
/// Handles CSI sequences of the form ESC [ ... <final byte> and
/// bare ESC sequences (ESC followed by a non-[ character).
fn strip_ansi(input: &str) -> String {
    let bytes = input.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] == 0x1b {
            // ESC detected
            i += 1;
            if i < bytes.len() && bytes[i] == b'[' {
                // CSI sequence -- skip until final byte (0x40..=0x7e)
                i += 1;
                while i < bytes.len() && !(0x40..=0x7eu8).contains(&bytes[i]) {
                    i += 1;
                }
                i += 1; // skip final byte
            } else {
                // Single-char escape or other Fe sequence -- skip one more byte
                i += 1;
            }
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }

    // The input was valid UTF-8, and we only kept non-escape bytes which are
    // also valid UTF-8 code units, so this is safe.
    String::from_utf8_lossy(&out).into_owned()
}

// ---------------------------------------------------------------------------
// Synthesizer
// ---------------------------------------------------------------------------

pub struct Synthesizer {
    client: Arc<LlmClient>,
    model_name: String,
}

impl Synthesizer {
    pub fn new(client: Arc<LlmClient>, model_name: &str) -> Self {
        Self {
            client,
            model_name: model_name.to_string(),
        }
    }

    pub async fn synthesize(&self, raw_output: &str) -> SynthesizedResponse {
        if raw_output.is_empty() {
            return SynthesizedResponse::passthrough(raw_output);
        }

        let clean = strip_ansi(raw_output);

        let system_prompt =
            "You are a concise technical summarizer. \
             Given raw agent output, extract structured information. \
             Respond with valid JSON only -- no prose outside the JSON object.";

        let user_prompt = format!(
            "Analyze the following agent output and extract:\n\
             1. A 2-3 sentence summary of what was accomplished.\n\
             2. A list of files that were created or modified (paths only).\n\
             3. A list of key actions taken (short phrases).\n\
             4. Any warnings, errors, or issues encountered.\n\
             \n\
             Respond with JSON matching this shape:\n\
             {{\n\
               \"summary\": \"...\",\n\
               \"files_changed\": [\"...\"],\n\
               \"key_actions\": [\"...\"],\n\
               \"warnings\": [\"...\"]\n\
             }}\n\
             \n\
             Agent output:\n\
             {clean}"
        );

        let messages: &[(&str, &str)] = &[
            ("system", system_prompt),
            ("user", &user_prompt),
        ];

        let request = LlmClient::build_request_with_model(
            &self.model_name,
            messages,
            0.1,
            None,
        );

        let response = match self.client.complete(&request).await {
            Ok(r) => r,
            Err(_) => return SynthesizedResponse::passthrough(raw_output),
        };

        let content = match response.choices.first() {
            Some(choice) => &choice.message.content,
            None => return SynthesizedResponse::passthrough(raw_output),
        };

        match serde_json::from_str::<LlmSynthesis>(content) {
            Ok(parsed) => SynthesizedResponse {
                summary: parsed.summary,
                files_changed: parsed.files_changed,
                key_actions: parsed.key_actions,
                warnings: parsed.warnings,
                raw_output: raw_output.to_string(),
            },
            Err(_) => SynthesizedResponse::passthrough(raw_output),
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_ansi_removes_color_codes() {
        let input = "\x1b[32mHello\x1b[0m world";
        assert_eq!(strip_ansi(input), "Hello world");
    }

    #[test]
    fn strip_ansi_handles_bare_escape() {
        let input = "before\x1bXafter";
        assert_eq!(strip_ansi(input), "beforeafter");
    }

    #[test]
    fn strip_ansi_plain_text_unchanged() {
        let input = "no escapes here";
        assert_eq!(strip_ansi(input), input);
    }

    #[test]
    fn passthrough_populates_summary_and_raw() {
        let s = SynthesizedResponse::passthrough("some output");
        assert_eq!(s.summary, "some output");
        assert_eq!(s.raw_output, "some output");
        assert!(s.files_changed.is_empty());
        assert!(s.key_actions.is_empty());
        assert!(s.warnings.is_empty());
    }

    #[test]
    fn format_for_display_summary_only() {
        let s = SynthesizedResponse::passthrough("Did a thing.");
        let display = s.format_for_display();
        assert!(display.contains("Summary"));
        assert!(display.contains("Did a thing."));
        assert!(!display.contains("Files Changed"));
    }

    #[test]
    fn format_for_display_all_sections() {
        let s = SynthesizedResponse {
            summary: "Compiled successfully.".to_string(),
            files_changed: vec!["src/main.rs".to_string()],
            key_actions: vec!["ran cargo build".to_string()],
            warnings: vec!["unused variable x".to_string()],
            raw_output: String::new(),
        };
        let display = s.format_for_display();
        assert!(display.contains("Files Changed"));
        assert!(display.contains("src/main.rs"));
        assert!(display.contains("Key Actions"));
        assert!(display.contains("ran cargo build"));
        assert!(display.contains("Warnings"));
        assert!(display.contains("unused variable x"));
    }
}
