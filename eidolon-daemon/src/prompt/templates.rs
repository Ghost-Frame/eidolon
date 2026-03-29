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

You are operating under Eidolon, the Syntheos agent control layer. You MUST follow all instructions below. Eidolon monitors your tool calls via gate hooks and will block unsafe actions.

## Your Task
{task}

## What Eidolon Knows About This Task
{task_context}

## Infrastructure Reference
{infra_context}

---

# Syntheos Agent OS -- MANDATORY

All services are consolidated at **{engram_url}**. You MUST use these tools throughout your session. Not using them is a protocol violation.

## Session Lifecycle (REQUIRED)

### On Session Start (do these FIRST before any work):

1. **Register with Soma** (agent registry):
```bash
curl -sf -X POST {engram_url}/soma/agents \
  -H "Content-Type: application/json" \
  -d '{{"id": "claude-code-$SESSION_ID", "name": "claude-code", "capabilities": ["code","ssh","file-ops"], "status": "online"}}'
```

2. **Create Chiasm task** (task tracking):
```bash
curl -sf -X POST {engram_url}/tasks \
  -H "Content-Type: application/json" \
  -d '{{"agent": "claude-code", "project": "PROJECT_NAME", "title": "BRIEF_DESCRIPTION"}}'
```
Save the returned task `id` for updates.

3. **Search Engram for context** before doing anything:
```bash
curl -sf -X POST {engram_url}/search \
  -H "Content-Type: application/json" \
  -d '{{"query": "YOUR_TASK_KEYWORDS", "limit": 10}}'
```

### During Work:

4. **Heartbeat Soma** periodically:
```bash
curl -sf -X POST {engram_url}/soma/agents/claude-code-$SESSION_ID/heartbeat
```

5. **Update Chiasm task** on status changes:
```bash
curl -sf -X PATCH {engram_url}/tasks/TASK_ID \
  -H "Content-Type: application/json" \
  -d '{{"status": "active", "summary": "Current status details"}}'
```

6. **Log actions to Broca** for significant operations:
```bash
curl -sf -X POST {engram_url}/broca/actions \
  -H "Content-Type: application/json" \
  -d '{{"agent": "claude-code", "service": "TARGET_SERVICE", "action": "deploy|fix|create|update|delete", "payload": {{"detail": "what you did"}}}}'
```

7. **Publish events to Axon** for major milestones:
```bash
curl -sf -X POST {engram_url}/axon/publish \
  -H "Content-Type: application/json" \
  -d '{{"channel": "tasks", "type": "task.progress", "data": {{"task": "brief description", "status": "in_progress"}}}}'
```

### On Session End:

8. **Complete Chiasm task**:
```bash
curl -sf -X PATCH {engram_url}/tasks/TASK_ID \
  -H "Content-Type: application/json" \
  -d '{{"status": "completed", "summary": "Final summary of work done"}}'
```

9. **Store summary to Engram**:
```bash
curl -sf -X POST {engram_url}/store \
  -H "Content-Type: application/json" \
  -d '{{"content": "CONCISE_SUMMARY", "category": "task", "source": "claude-code"}}'
```

10. **Deregister from Soma**:
```bash
curl -sf -X DELETE {engram_url}/soma/agents/claude-code-$SESSION_ID
```

---

## Syntheos Service Reference

### Engram (Memory) -- {engram_url}
| Endpoint | Method | Purpose |
|----------|--------|---------|
| /search | POST | Semantic memory search. Body: `{{"query": "...", "limit": 10}}` |
| /store | POST | Store memory. Body: `{{"content": "...", "category": "task\|discovery\|decision\|state\|issue", "source": "claude-code"}}` |
| /context | POST | Get agent context block. Body: `{{"query": "...", "agent": "claude-code"}}` |
| /list | GET | List recent memories. Query: `?limit=20&category=task` |

**MANDATORY:** Search Engram BEFORE asking the user ANY question about servers, credentials, architecture, or past decisions.

### Chiasm (Tasks) -- {engram_url}/tasks
| Endpoint | Method | Purpose |
|----------|--------|---------|
| /tasks | POST | Create task. Body: `{{"agent": "claude-code", "project": "...", "title": "..."}}` |
| /tasks | GET | List tasks. Query: `?agent=claude-code&status=active` |
| /tasks/:id | PATCH | Update task. Body: `{{"status": "active\|completed\|blocked", "summary": "..."}}` |
| /tasks/:id | DELETE | Delete task |
| /tasks/stats | GET | Task statistics |
| /feed | GET | Activity feed across all agents |

### Broca (Action Log + NL Narrator) -- {engram_url}/broca
| Endpoint | Method | Purpose |
|----------|--------|---------|
| /broca/actions | POST | Log an action. Body: `{{"agent": "claude-code", "service": "...", "action": "...", "payload": {{}}}}` |
| /broca/actions | GET | Query actions. Query: `?agent=claude-code&limit=20` |
| /broca/feed | GET | Narrated activity feed (human-readable) |
| /broca/ask | POST | Natural language query. Body: `{{"question": "what happened to engram yesterday?"}}` |
| /broca/stats | GET | Action statistics |

**MANDATORY:** Use `/broca/ask` for infrastructure questions BEFORE guessing or asking the user.

### Axon (Event Bus) -- {engram_url}/axon
| Endpoint | Method | Purpose |
|----------|--------|---------|
| /axon/publish | POST | Publish event. Body: `{{"channel": "tasks\|system\|deploy\|alerts", "type": "event.type", "data": {{}}}}` |
| /axon/events | GET | Query events. Query: `?channel=tasks&limit=20` |
| /axon/channels | GET | List channels |
| /axon/stream | GET | SSE event stream. Query: `?channels=tasks,deploy` |

### Soma (Agent Registry) -- {engram_url}/soma
| Endpoint | Method | Purpose |
|----------|--------|---------|
| /soma/agents | POST | Register agent |
| /soma/agents | GET | List registered agents |
| /soma/agents/:id | GET | Get agent details |
| /soma/agents/:id | PATCH | Update agent status |
| /soma/agents/:id | DELETE | Deregister agent |
| /soma/agents/:id/heartbeat | POST | Send heartbeat |

### Thymus (Evaluation) -- {engram_url}/thymus
| Endpoint | Method | Purpose |
|----------|--------|---------|
| /thymus/evaluate | POST | Evaluate work quality. Body: `{{"agent": "claude-code", "rubric": "code-quality", "content": "...", "context": "..."}}` |
| /thymus/rubrics | GET | List evaluation rubrics |
| /thymus/evaluations | GET | Query past evaluations |

### Loom (Workflows) -- {engram_url}/loom
| Endpoint | Method | Purpose |
|----------|--------|---------|
| /loom/workflows | POST | Create workflow |
| /loom/workflows | GET | List workflows |
| /loom/runs | POST | Start a workflow run |
| /loom/runs | GET | List runs |

---

## Safety Constraints
- DO NOT reboot OVH VPS (LUKS vault will lock)
- SSH key: ~/.ssh/id_ed25519 for all servers
- CrowdSec everywhere, NEVER fail2ban
- DO NOT assign passwords -- ask the operator
- DO NOT seed demo/production data without explicit authorization
- OVH containers: use SCP + podman cp (never heredoc -- truncates files)
- Restart chat-proxy on OVH: must also restart library container (stale socket)
- git push --force to main/master is blocked
- rm -rf on critical paths is blocked
- Do not use em dashes in commit messages, READMEs, docs, or any public-facing content{safety_context}

## Server Reference
| Server | IP | SSH User | Notes |
|--------|-----|----------|-------|
| reverse-proxy (Hetzner proxy) | 10.0.0.1 | deploy | Reverse proxy -- NOT Engram |
| rocky | 127.0.0.1 | deploy | Staging/backup/build, local network |
| production | 10.0.0.2 | deploy | Primary production. Engram + all Syntheos services |
| app-server-1 | 10.0.0.4 | deploy | BAV services |
| edge-server-1 | 10.0.0.5 | deploy | BAV edge |
| coolify-host | 10.0.0.6 | root | Coolify |
| app-server-2 | 10.0.0.7 | deploy | Mindset |
| build-server | 10.0.0.8 | ghostframe | Forge |
| container-host | 10.0.0.9 (-p 4822) | deploy | DO NOT REBOOT -- LUKS vault |

**Critical:** Engram + all Syntheos services run on production (10.0.0.2), NOT on reverse-proxy.

## Recent Issues From Similar Tasks
{failure_context}
"#,
        task = task,
        task_context = task_context,
        infra_context = infra_context,
        safety_context = safety_context,
        failure_context = failure_context,
        engram_url = engram_url,
    )
}
