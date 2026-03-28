pub const SECTION_TASK: &str = "## TASK";
pub const SECTION_STATE: &str = "## CURRENT STATE";
pub const SECTION_CONSTRAINTS: &str = "## CONSTRAINTS";
pub const SECTION_TOOLS: &str = "## TOOLS";
pub const SECTION_ISSUES: &str = "## KNOWN ISSUES";
pub const SECTION_CONTEXT: &str = "## RELEVANT CONTEXT";

pub fn static_constraints() -> String {
    r#"- SSH key is at ~/.ssh/id_ed25519 (not ~/.ssh/id_rsa)
- SSH connection pattern: ssh -i ~/.ssh/id_ed25519 user@host
- OVH VPS: ssh -i ~/.ssh/id_ed25519 -p 4822 deploy@10.0.0.9
- DO NOT reboot OVH VPS (10.0.0.9, port 4822) -- LUKS vault will lock
- Use CrowdSec, never fail2ban
- Do not use em dashes in any files, commit messages, or output
- Register with Chiasm at session start
- Query Engram before asking questions about infrastructure
- SSH key must be verified before locking down SSH config
- Never assign passwords -- ask the operator what he wants
- OVH containers: use SCP + podman cp (not heredoc -- truncates files)
- Restart chat-proxy on OVH: must also restart library container (stale socket)"#.to_string()
}

pub fn tools_section(engram_url: &str, chiasm_url: &str) -> String {
    format!(
        r#"- Engram search: POST {engram_url}/search {{\"query\": \"...\", \"limit\": 10}}
- Engram store: POST {engram_url}/store {{\"content\": \"...\", \"category\": \"task\", \"source\": \"claude-code\"}}
- Chiasm register: POST {chiasm_url}/tasks {{\"agent\": \"claude-code\", \"project\": \"...\", \"title\": \"...\"}}
- Chiasm feed: GET {chiasm_url}/feed"#,
        engram_url = engram_url,
        chiasm_url = chiasm_url,
    )
}

pub fn format_fallback_briefing(memories: &[crate::prompt::generator::MemorySummary]) -> String {
    if memories.is_empty() {
        return "No relevant context found in brain substrate.".to_string();
    }
    let lines: Vec<String> = memories.iter().take(10).map(|m| {
        format!("- [{}] {}", m.category, m.content)
    }).collect();
    lines.join("\n")
}
