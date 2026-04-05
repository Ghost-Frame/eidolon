use base64::Engine;
use std::collections::HashMap;

pub struct ScrubRegistry {
    tracked: HashMap<String, Vec<String>>,
}

/// Minimum secret length to generate encoded variants.
/// Shorter secrets produce base64/percent-encoded strings that are too
/// generic and would cause false-positive scrubbing.
const MIN_ENCODED_SCRUB_LEN: usize = 8;

impl ScrubRegistry {
    pub fn new() -> Self {
        ScrubRegistry {
            tracked: HashMap::new(),
        }
    }

    pub fn track(&mut self, session_id: &str, value: String) {
        self.tracked
            .entry(session_id.to_string())
            .or_default()
            .push(value);
    }

    pub fn scrub(&self, session_id: &str, text: &str) -> String {
        let Some(values) = self.tracked.get(session_id) else {
            return text.to_string();
        };
        let mut result = text.to_string();
        for secret in values {
            if secret.is_empty() {
                continue;
            }
            // Raw string match (always)
            result = result.replace(secret, "[REDACTED]");

            // Encoded variant scrubbing (only for secrets long enough to avoid false positives)
            if secret.len() >= MIN_ENCODED_SCRUB_LEN {
                // Base64 standard encoding
                let b64_std = base64::engine::general_purpose::STANDARD.encode(secret.as_bytes());
                if result.contains(&b64_std) {
                    result = result.replace(&b64_std, "[REDACTED:b64]");
                }

                // Base64 URL-safe encoding
                let b64_url = base64::engine::general_purpose::URL_SAFE.encode(secret.as_bytes());
                if b64_url != b64_std && result.contains(&b64_url) {
                    result = result.replace(&b64_url, "[REDACTED:b64]");
                }

                // Base64 without padding (common in JWTs and URLs)
                let b64_nopad = base64::engine::general_purpose::STANDARD_NO_PAD.encode(secret.as_bytes());
                if b64_nopad != b64_std && result.contains(&b64_nopad) {
                    result = result.replace(&b64_nopad, "[REDACTED:b64]");
                }

                // Percent-encoding (URL encoding)
                let pct = percent_encode(secret);
                if pct != *secret && result.contains(&pct) {
                    result = result.replace(&pct, "[REDACTED:pct]");
                }
            }
        }
        result
    }

    pub fn remove_session(&mut self, session_id: &str) {
        self.tracked.remove(session_id);
    }
}

/// Percent-encode a string (RFC 3986 unreserved characters pass through).
fn percent_encode(input: &str) -> String {
    let mut encoded = String::with_capacity(input.len() * 3);
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                encoded.push(byte as char);
            }
            _ => {
                encoded.push('%');
                encoded.push(hex_char(byte >> 4));
                encoded.push(hex_char(byte & 0x0F));
            }
        }
    }
    encoded
}

fn hex_char(nibble: u8) -> char {
    match nibble {
        0..=9 => (b'0' + nibble) as char,
        10..=15 => (b'A' + nibble - 10) as char,
        _ => unreachable!(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrub_raw_value() {
        let mut reg = ScrubRegistry::new();
        reg.track("s1", "my-secret-key-12345".to_string());
        let result = reg.scrub("s1", "the key is my-secret-key-12345 here");
        assert!(result.contains("[REDACTED]"));
        assert!(!result.contains("my-secret-key-12345"));
    }

    #[test]
    fn scrub_base64_encoded() {
        let mut reg = ScrubRegistry::new();
        let secret = "SuperSecretAPIKey123";
        reg.track("s1", secret.to_string());
        let b64 = base64::engine::general_purpose::STANDARD.encode(secret.as_bytes());
        let text = format!("encoded: {}", b64);
        let result = reg.scrub("s1", &text);
        assert!(result.contains("[REDACTED:b64]"));
        assert!(!result.contains(&b64));
    }

    #[test]
    fn scrub_percent_encoded() {
        let mut reg = ScrubRegistry::new();
        let secret = "key=value&secret+data";
        reg.track("s1", secret.to_string());
        let pct = percent_encode(secret);
        let text = format!("url param: {}", pct);
        let result = reg.scrub("s1", &text);
        assert!(result.contains("[REDACTED:pct]"));
        assert!(!result.contains(&pct));
    }

    #[test]
    fn short_secrets_skip_encoding() {
        let mut reg = ScrubRegistry::new();
        reg.track("s1", "short".to_string());
        let b64 = base64::engine::general_purpose::STANDARD.encode(b"short");
        let text = format!("has {} in it", b64);
        let result = reg.scrub("s1", &text);
        // Short secret should NOT trigger b64 scrubbing (false positive risk)
        assert!(!result.contains("[REDACTED:b64]"));
    }

    #[test]
    fn no_session_returns_unchanged() {
        let reg = ScrubRegistry::new();
        let result = reg.scrub("nonexistent", "some text");
        assert_eq!(result, "some text");
    }
}
