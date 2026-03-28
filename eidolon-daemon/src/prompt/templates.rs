use crate::prompt::generator::MemorySummary;

fn format_memories_as_bullets(memories: &[MemorySummary]) -> String {
    if memories.is_empty() {
        return "No relevant memories found.".to_string();
    }
    memories.iter().take(8).map(|m| {
        format!("- [{}] {}", m.category, m.content.trim())
    }).collect::<Vec<_>>().join("\n")
}

pub fn build_living_prompt(
    task: &str,
    task_memories: &[MemorySummary],
    infra_memories: &[MemorySummary],
    safety_memories: &[MemorySummary],
    failure_memories: &[MemorySummary],
    engram_url: &str,
    chiasm_url: &str,
) -> String {
    let task_context = format_memories_as_bullets(task_memories);
    let infra_context = format_memories_as_bullets(infra_memories);
    let failure_context = format_memories_as_bullets(failure_memories);
    let safety_context = if safety_memories.is_empty() {
        String::new()
    } else {
        format!("\n\n## Additional Safety Rules From Engram\n{}", format_memories_as_bullets(safety_memories))
    };

    format!(
        r#"# Eidolon Session Context

## Your Task
{task}

## What Eidolon Knows About This Task
{task_context}

## Infrastructure Reference
{infra_context}

## Safety Constraints
- DO NOT reboot OVH VPS (LUKS vault will lock)
- SSH key: ~/.ssh/id_ed25519 for all servers
- CrowdSec everywhere, NEVER fail2ban
- DO NOT assign passwords, ask the operator
- DO NOT touch demo data or seed real data into public-facing instances
- Register with Chiasm on session start
- Store discoveries to Engram
- Query Engram BEFORE guessing at anything
- OVH containers: use SCP + podman cp (never heredoc -- truncates files)
- Restart chat-proxy on OVH: must also restart library container (stale socket)
- git push --force to main/master is blocked
- rm -rf on critical paths is blocked{safety_context}

## Server Reference
| Server | IP | SSH User | Notes |
|--------|-----|----------|-------|
| reverse-proxy (Hetzner proxy) | 10.0.0.1 | deploy | Reverse-proxy reverse proxy -- NOT Engram |
| rocky | 127.0.0.1 / 10.0.0.3 | deploy | Staging/backup, local network |
| production | 10.0.0.2 | deploy | Engram production server |
| app-server-1 | 10.0.0.4 / 10.0.0.4 | deploy | BAV services |
| edge-server-1 | 10.0.0.5 / 10.0.0.5 | deploy | BAV edge |
| coolify-host | 10.0.0.6 / 10.0.0.6 | root | Coolify |
| app-server-2 | 10.0.0.7 / 10.0.0.7 | deploy | Mindset |
| build-server | 10.0.0.8 / 10.0.0.8 | ghostframe | Forge |
| container-host | 10.0.0.9 / 10.0.0.9 | deploy | Port 4822, DO NOT REBOOT |

**Critical:** Engram runs on production (10.0.0.2), NOT on reverse-proxy (10.0.0.1).
Engram URL: {engram_url}
Chiasm URL: {chiasm_url}

## Tools
- Engram search: POST {engram_url}/search {{"query": "...", "limit": 10}}
- Engram store: POST {engram_url}/store {{"content": "...", "category": "task", "source": "claude-code"}}
- Chiasm register: POST {chiasm_url}/tasks {{"agent": "claude-code", "project": "...", "title": "..."}}
- Chiasm feed: GET {chiasm_url}/feed

## Recent Issues From Similar Tasks
{failure_context}
"#,
        task = task,
        task_context = task_context,
        infra_context = infra_context,
        safety_context = safety_context,
        failure_context = failure_context,
        engram_url = engram_url,
        chiasm_url = chiasm_url,
    )
}
