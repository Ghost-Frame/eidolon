// instincts.rs -- synthetic pre-training corpus for new Eidolon instances
//
// Generates ~200 structurally realistic ghost memories across 5 categories.
// Ghosts use negative IDs, start at strength 0.3, decay 2x faster than real memories.
// When a real memory with cosine_sim > 0.85 to a ghost is absorbed, the ghost is removed.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::Path;

use ndarray::Array1;
use serde::{Deserialize, Serialize};

use crate::graph::ConnectionGraph;
use crate::substrate::HopfieldSubstrate;
use crate::types::{BrainMemory, EdgeType, RAW_DIM};

// Ghost strength and decay constants
pub const GHOST_STRENGTH: f32 = 0.3;
pub const GHOST_REPLACE_SIM: f32 = 0.85;

// Binary file magic header
const INST_MAGIC: &[u8; 4] = b"INST";
const INST_VERSION: u32 = 1;

// ---- Types ----

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyntheticMemory {
    pub id: i64,
    pub content: String,
    pub category: String,
    pub importance: i32,
    pub created_at: String,
    pub embedding: Vec<f32>, // 1024-dim, L2-normalized, seeded from content hash
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyntheticEdge {
    pub source_id: i64,
    pub target_id: i64,
    pub weight: f32,
    pub edge_type: String, // "association" | "temporal" | "contradiction"
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstinctsCorpus {
    pub version: u32,
    pub generated_at: String,
    pub memories: Vec<SyntheticMemory>,
    pub edges: Vec<SyntheticEdge>,
}

// ---- Deterministic embedding generation ----
// Seeds from content hash, produces a sin-based pattern, then L2-normalizes.

fn hash_content(content: &str) -> u64 {
    // FNV-1a 64-bit hash
    let mut h: u64 = 14695981039346656037u64;
    for byte in content.bytes() {
        h ^= byte as u64;
        h = h.wrapping_mul(1099511628211u64);
    }
    h
}

fn make_embedding(content: &str) -> Vec<f32> {
    let seed = hash_content(content);
    let mut emb = vec![0.0f32; RAW_DIM];
    for i in 0..RAW_DIM {
        let val = seed
            .wrapping_mul(6364136223846793005u64)
            .wrapping_add((i as u64).wrapping_mul(1442695040888963407u64));
        let angle = (val as f32) * (std::f32::consts::PI / (u32::MAX as f32 * 2.0));
        emb[i] = angle.sin();
    }
    // L2 normalize
    let norm: f32 = emb.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 1e-10 {
        for v in &mut emb {
            *v /= norm;
        }
    }
    emb
}

fn is_leap(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

fn days_to_ymd(days_from_epoch: i64) -> (i32, u32, u32) {
    let mut remaining = days_from_epoch;
    let mut year = 1970i32;
    loop {
        let days_in_year = if is_leap(year) { 366 } else { 365 };
        if remaining < days_in_year {
            break;
        }
        remaining -= days_in_year;
        year += 1;
    }
    let month_days: [i64; 12] = [
        31,
        if is_leap(year) { 29 } else { 28 },
        31, 30, 31, 30, 31, 31, 30, 31, 30, 31,
    ];
    let mut month = 1u32;
    for md in &month_days {
        if remaining < *md {
            break;
        }
        remaining -= md;
        month += 1;
    }
    (year, month, remaining as u32 + 1)
}

fn format_date(offset_hours: i64) -> String {
    // Base: 2026-01-15T00:00:00Z = 1768521600 seconds since Unix epoch
    let base: i64 = 1768521600;
    let ts = base + offset_hours * 3600;
    let secs = ts;
    let mins = secs / 60;
    let hours_total = mins / 60;
    let days_total = hours_total / 24;
    let s = (secs % 60) as u32;
    let m = (mins % 60) as u32;
    let h = (hours_total % 24) as u32;
    let (year, month, day) = days_to_ymd(days_total);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year, month, day, h, m, s
    )
}

// ---- Corpus generator ----

pub fn generate_instincts() -> InstinctsCorpus {
    let mut memories: Vec<SyntheticMemory> = Vec::with_capacity(200);
    let mut edges: Vec<SyntheticEdge> = Vec::with_capacity(300);
    let mut next_id: i64 = -1;

    // Category 1: Infrastructure state transitions (10 sets x 4 memories = 40)
    let infra_sets: &[(&str, &str, &str, &str, i64)] = &[
        ("nginx v1.18", "nginx v1.24", "web-proxy", "HTTP reverse proxy", 0),
        ("PostgreSQL 13", "PostgreSQL 16", "db-primary", "primary database", 48),
        ("Redis 6.2", "Redis 7.2", "cache-layer", "session cache", 96),
        ("Node.js 18", "Node.js 22", "api-server", "REST API runtime", 144),
        ("Docker 20.10", "Podman 4.9", "container-runtime", "container orchestration", 192),
        ("Python 3.10", "Python 3.12", "worker-service", "background task runner", 240),
        ("Elasticsearch 7", "OpenSearch 2.11", "search-cluster", "full-text search index", 288),
        ("RabbitMQ 3.10", "RabbitMQ 3.12", "message-broker", "async job queue", 336),
        ("Traefik 2.x", "Traefik 3.x", "ingress-controller", "TLS termination and routing", 384),
        ("Grafana 9", "Grafana 11", "monitoring-ui", "metrics dashboard", 432),
    ];

    for (old_ver, new_ver, service, desc, base_hours) in infra_sets {
        let id1 = next_id; next_id -= 1;
        let content1 = format!(
            "{} is running {} on {} -- {}. Deployed 2026-01-15. Status: stable.",
            service, old_ver, service, desc
        );
        memories.push(SyntheticMemory {
            id: id1,
            content: content1.clone(),
            category: "state".to_string(),
            importance: 5,
            created_at: format_date(*base_hours),
            embedding: make_embedding(&content1),
        });

        let id2 = next_id; next_id -= 1;
        let content2 = format!(
            "Decision to migrate {} from {} to {} on {}. Reason: upstream EOL and security patches. Scheduled downtime: 30 minutes.",
            service, old_ver, new_ver, service
        );
        memories.push(SyntheticMemory {
            id: id2,
            content: content2.clone(),
            category: "decision".to_string(),
            importance: 7,
            created_at: format_date(base_hours + 12),
            embedding: make_embedding(&content2),
        });

        let id3 = next_id; next_id -= 1;
        let content3 = format!(
            "{} successfully migrated to {} on {}. Migration completed 2026-01. All health checks passing. Previous version {} decommissioned.",
            service, new_ver, service, old_ver
        );
        memories.push(SyntheticMemory {
            id: id3,
            content: content3.clone(),
            category: "task".to_string(),
            importance: 8,
            created_at: format_date(base_hours + 24),
            embedding: make_embedding(&content3),
        });

        let id4 = next_id; next_id -= 1;
        let content4 = format!(
            "{} is NOW running {} on {} -- {}. Upgraded from {}. Status: stable, verified.",
            service, new_ver, service, desc, old_ver
        );
        memories.push(SyntheticMemory {
            id: id4,
            content: content4.clone(),
            category: "state".to_string(),
            importance: 9,
            created_at: format_date(base_hours + 25),
            embedding: make_embedding(&content4),
        });

        edges.push(SyntheticEdge { source_id: id1, target_id: id2, weight: 0.8, edge_type: "temporal".to_string() });
        edges.push(SyntheticEdge { source_id: id2, target_id: id3, weight: 0.8, edge_type: "temporal".to_string() });
        edges.push(SyntheticEdge { source_id: id3, target_id: id4, weight: 0.8, edge_type: "temporal".to_string() });
        edges.push(SyntheticEdge { source_id: id1, target_id: id4, weight: 0.7, edge_type: "contradiction".to_string() });
    }

    // Category 2: Decision records (10 sets x 2 memories = 20)
    let decisions: &[(&str, &str, &str, &str, i64)] = &[
        ("database", "Use PostgreSQL over MySQL for the primary store",
         "ACID compliance, better JSON support, and superior indexing. MySQL replication lag unacceptable for consistency requirements.",
         "PostgreSQL selected. MySQL evaluated and rejected.", 500),
        ("caching", "Use Redis over Memcached for session caching",
         "Redis supports persistence, pub/sub, and sorted sets needed for leaderboards. Memcached is volatile-only.",
         "Redis deployed for sessions and pub/sub.", 520),
        ("container", "Switch from Docker to Podman for production workloads",
         "Rootless Podman reduces attack surface. Docker daemon single point of failure eliminated. OCI compatible.",
         "Podman adopted. Docker daemon removed from production nodes.", 540),
        ("monitoring", "Adopt Prometheus and Grafana over Datadog",
         "Cost: Datadog $3k/month vs self-hosted $50/month infra cost. Prometheus retention and alerting fully customizable.",
         "Prometheus stack deployed. Datadog subscription cancelled.", 560),
        ("proxy", "Use Traefik over HAProxy for ingress",
         "Traefik integrates with Docker service discovery. HAProxy requires manual config for each backend.",
         "Traefik deployed as ingress. HAProxy configs archived.", 580),
        ("logging", "Use Loki over Elasticsearch for log aggregation",
         "Loki label-based indexing 10x cheaper at scale. Elasticsearch full-text not needed for structured logs.",
         "Loki deployed. Elasticsearch retained only for search features.", 600),
        ("queue", "Use RabbitMQ over Kafka for job processing",
         "Kafka overhead unjustified for sub-10k msg/sec. RabbitMQ simpler ops and sufficient throughput.",
         "RabbitMQ in production. Kafka evaluated for future data pipeline.", 620),
        ("cdn", "Self-host Nginx for static assets over CloudFront",
         "Data transfer costs $800/month on CloudFront. Nginx on dedicated node costs $40/month at current traffic.",
         "Nginx static asset server deployed. CloudFront distribution disabled.", 640),
        ("auth", "Implement JWT with refresh tokens over session cookies",
         "Stateless JWT enables horizontal scaling without session store. Refresh token rotation provides security equivalent.",
         "JWT auth implemented. Session store removed from architecture.", 660),
        ("backup", "Use restic over duplicati for backup strategy",
         "restic deduplication more efficient. CLI-first design fits automation. duplicati GUI dependency removed from headless servers.",
         "restic deployed on all nodes. Automated daily snapshots verified.", 680),
    ];

    for (domain, title, rationale, outcome, base_hours) in decisions {
        let id1 = next_id; next_id -= 1;
        let content1 = format!(
            "Architecture decision [{}]: {}. Rationale: {}",
            domain, title, rationale
        );
        memories.push(SyntheticMemory {
            id: id1,
            content: content1.clone(),
            category: "decision".to_string(),
            importance: 8,
            created_at: format_date(*base_hours),
            embedding: make_embedding(&content1),
        });

        let id2 = next_id; next_id -= 1;
        let content2 = format!(
            "Decision outcome [{}]: {} Implemented and verified in production.",
            domain, outcome
        );
        memories.push(SyntheticMemory {
            id: id2,
            content: content2.clone(),
            category: "task".to_string(),
            importance: 7,
            created_at: format_date(base_hours + 24),
            embedding: make_embedding(&content2),
        });

        edges.push(SyntheticEdge { source_id: id1, target_id: id2, weight: 0.75, edge_type: "association".to_string() });
        edges.push(SyntheticEdge { source_id: id2, target_id: id1, weight: 0.75, edge_type: "association".to_string() });
    }

    // Category 3: Discovery / reference (20 memories)
    let references: &[(&str, &str, i64, i32)] = &[
        ("server-specs", "app-server-1: 8 vCPU, 32GB RAM, 500GB NVMe SSD, Ubuntu 22.04. Role: application services backend.", 700, 8),
        ("server-specs", "edge-server-1: 4 vCPU, 16GB RAM, 200GB SSD, Rocky Linux 9. Role: CDN edge node and static assets.", 702, 7),
        ("server-specs", "dev-workstation: Xeon W-2125 4.0GHz 8-core, 30GB RAM, 2TB HDD. Role: primary development and build machine.", 704, 9),
        ("endpoint", "Engram memory API: POST /store to persist memories, POST /search to query, GET /recall for recent. Auth via Bearer token.", 710, 8),
        ("endpoint", "Eidolon brain API: JSON over stdio. Commands: init, query, absorb, decay_tick, dream_cycle, get_stats, shutdown.", 712, 9),
        ("filepath", "Brain database location: /brain.db -- SQLite, contains memories, edges, pca_state tables.", 714, 8),
        ("filepath", "Instincts binary: /instincts.bin -- gzip-compressed JSON corpus, applied on first init when brain.db is empty.", 716, 7),
        ("filepath", "Eidolon Rust source: Eidolon source: src/ directory of eidolon-lib crate -- substrate.rs, graph.rs, dreaming.rs, instincts.rs, main.rs.", 718, 6),
        ("credential", "SSH key for all servers: operator-configured SSH key. Custom ports may apply for specific servers.", 720, 9),
        ("network", "VPN mesh subnet: configured in operator's mesh network. All nodes reachable by mesh IP.. Use internal IPs for inter-service traffic.", 722, 8),
        ("config", "Nginx config directory: /etc/nginx/sites-enabled/. Reload: nginx -t && systemctl reload nginx. Never restart without testing.", 730, 7),
        ("config", "PostgreSQL data directory: /var/lib/postgresql/16/main/. Config: /etc/postgresql/16/main/postgresql.conf.", 732, 7),
        ("config", "Redis config: /etc/redis/redis.conf. Bind 127.0.0.1 only. requirepass enabled. maxmemory-policy allkeys-lru.", 734, 6),
        ("pattern", "Service restart pattern: check state -> back up config -> stop service -> apply change -> start service -> verify health -> monitor logs.", 740, 9),
        ("pattern", "File deployment pattern: write locally -> SCP to /tmp/ -> SSH mv to destination -> set permissions -> verify.", 742, 8),
        ("pattern", "Never use heredoc over SSH for file content -- truncates to 0 bytes. Always use SCP for file transfers to remote hosts.", 744, 9),
        ("pattern", "CrowdSec is the intrusion detection system on all nodes. Never install fail2ban. CrowdSec bouncer handles blocking.", 746, 8),
        ("error", "podman cp truncates heredoc content -- root cause: shell expansion in subprocess. Fix: scp local file then podman cp from host.", 750, 8),
        ("error", "Unix socket stale fd: when upstream restarts, downstream holds old fd. Both must restart in order: upstream first, then downstream.", 752, 7),
        ("error", "SELinux blocks unexpected service access -- check ausearch -m avc -ts recent. Fix: restorecon -Rv /path or semanage.", 754, 6),
    ];

    for (category, content, base_hours, importance) in references {
        let id = next_id; next_id -= 1;
        memories.push(SyntheticMemory {
            id,
            content: content.to_string(),
            category: category.to_string(),
            importance: *importance,
            created_at: format_date(*base_hours),
            embedding: make_embedding(content),
        });
    }

    // Category 4: Task completions (20 memories)
    let tasks: &[(&str, i64, i32)] = &[
        ("Deploy Eidolon brain substrate Phase 1 to the development server. Result: binary at /target/release/eidolon. brain.db initialized with 847 memories from Engram export.", 800, 9),
        ("Migrate Engram database from SQLite to PostgreSQL. Result: 12,847 memories migrated, 0 data loss, query time improved from 45ms to 8ms at p99.", 810, 8),
        ("Fix memory leak in Eidolon decay module. Root cause: Vec not cleared after prune. Fix: add memory.retain() after dead_set removal. Leak eliminated.", 820, 9),
        ("Set up Traefik TLS termination for all subdomains. Let Encrypt wildcard cert via DNS challenge. All subdomains now HTTPS.", 830, 8),
        ("Configure Prometheus scrape targets for all nodes. Added: node_exporter, postgres_exporter, redis_exporter, nginx_exporter.", 840, 7),
        ("Deploy CrowdSec on application and edge servers. Installed bouncer for nginx. Community blocklist active. First 24h: 1,247 IPs blocked.", 850, 8),
        ("Upgrade PostgreSQL 13 to 16 on db-primary. pg_upgrade used for in-place upgrade. Backup taken before: pg_dump 18GB. Zero data loss.", 860, 9),
        ("Implement JWT refresh token rotation in API server. Old refresh tokens invalidated on use. 7-day token expiry. Redis TTL set.", 870, 7),
        ("Set up automated restic backups on all nodes. Daily snapshots at 02:00 UTC. Retention: 7 daily, 4 weekly, 12 monthly.", 880, 8),
        ("Debug and fix Nginx upstream 502 errors. Root cause: backend pool exhausted due to connection leak in Python worker.", 890, 9),
        ("Enable TCP BBR on all Linux servers via sysctl. net.ipv4.tcp_congestion_control=bbr. p99 latency -23%, throughput +18%.", 900, 7),
        ("Consolidate application and edge server nginx configs into shared template. Reduced config duplication from 4 files to 1 template.", 910, 6),
        ("Add memory decay monitoring to Engram dashboard. Alert when avg_decay_factor < 0.3. Grafana panel showing health_distribution over time.", 920, 7),
        ("Write Eidolon dreaming module. Implements: replay_recent, merge_redundant, prune_dead, discover_connections, resolve_lingering.", 930, 9),
        ("Write Eidolon instincts module. Generates 200 synthetic ghost patterns across 5 categories. Ghost replacement on cosine_sim > 0.85.", 940, 9),
        ("Implement Hopfield substrate in Rust with ndarray. PCA projection 1024->512 dims. Retrieval via softmax activation.", 950, 9),
        ("Add graph spread to Eidolon query pipeline. BFS from Hopfield seeds, 3 hops, decay 0.5/hop. Contradiction resolution.", 960, 8),
        ("Deploy Eidolon C++ backend as alternative to Rust. Same JSON protocol. Eigen3 for linear algebra.", 970, 7),
        ("Fix race condition in dream cycle pruning. Root cause: iterating memories while removing. Fix: collect dead_ids first.", 980, 8),
        ("Optimize PCA projection in Eidolon. Moved from per-query projection to cached patterns. Query time improved from 8ms to 2ms p99.", 990, 8),
    ];

    for (content, base_hours, importance) in tasks {
        let id = next_id; next_id -= 1;
        memories.push(SyntheticMemory {
            id,
            content: content.to_string(),
            category: "task".to_string(),
            importance: *importance,
            created_at: format_date(*base_hours),
            embedding: make_embedding(content),
        });
    }

    // Category 5: Corrections (20 pairs x 2 = 40 memories)
    let corrections: &[(&str, &str, i64, i64, i32, i32)] = &[
        (
            "app-server-1 is running Ubuntu 20.04 LTS.",
            "CORRECTION: app-server-1 is running Ubuntu 22.04 LTS, not 20.04. Upgraded 2025-11. Verify with: lsb_release -a.",
            1100, 1112, 6, 8
        ),
        (
            "Redis is configured to bind to 0.0.0.0 for inter-service access.",
            "CORRECTION: Redis binds to 127.0.0.1 ONLY. Inter-service access via Unix socket or SSH tunnel. Binding 0.0.0.0 was a security incident.",
            1120, 1132, 5, 9
        ),
        (
            "PostgreSQL replication lag is acceptable at 2-3 seconds for read replicas.",
            "CORRECTION: PostgreSQL replication lag target is under 500ms, not 2-3 seconds. Alert threshold: 1000ms.",
            1140, 1152, 5, 8
        ),
        (
            "fail2ban is installed on all nodes for SSH protection.",
            "CORRECTION: CrowdSec is used, NOT fail2ban. fail2ban was removed in 2025-10. CrowdSec bouncer handles all blocking.",
            1160, 1172, 4, 9
        ),
        (
            "The SSH private key location is ~/.ssh/id_ed25519 on all machines.",
            "CORRECTION: SSH key is the operator-configured SSH key, NOT ~/.ssh/id_ed25519. All server logins use the operator-configured SSH key. Custom ports may apply for specific servers.",
            1180, 1192, 5, 9
        ),
        (
            "Eidolon brain.db is stored at /var/lib/eidolon/brain.db.",
            "CORRECTION: brain.db is at /brain.db, not /var/lib/eidolon/. The data_dir is /.",
            1200, 1212, 5, 8
        ),
        (
            "Memory patterns are stored in full 1024-dimensional space in the Hopfield substrate.",
            "CORRECTION: Patterns are PCA-projected to 512 dimensions before Hopfield storage. Raw 1024-dim embeddings stored in brain.db.",
            1220, 1232, 6, 9
        ),
        (
            "Dream cycles run during active query processing to consolidate recent memories.",
            "CORRECTION: Dream cycles run ONLY during idle periods. TypeScript coordinator pauses dreaming when query activity detected.",
            1240, 1252, 5, 8
        ),
        (
            "Ghost patterns from instincts have the same decay rate as real memories.",
            "CORRECTION: Ghost patterns decay at 2x the rate of real memories. Ghost strength starts at 0.3 vs 0.5 for real.",
            1260, 1272, 6, 9
        ),
        (
            "Association edges are created between all memory pairs with cosine similarity above 0.3.",
            "CORRECTION: Association threshold is 0.4, not 0.3. Contradiction threshold is 0.75. Max 15 edges per memory.",
            1280, 1292, 5, 7
        ),
        (
            "The Eidolon binary accepts HTTP REST API requests on port 7433.",
            "CORRECTION: Eidolon binary uses JSON over stdio, NOT HTTP. TypeScript manager wraps the binary in a subprocess.",
            1300, 1312, 5, 9
        ),
        (
            "Engram stores memories in a custom binary format for performance.",
            "CORRECTION: Engram stores memories in SQLite or PostgreSQL. The instincts.bin file is separate and not the main Engram storage.",
            1320, 1332, 5, 7
        ),
        (
            "The PCA transform is recomputed from scratch on every Eidolon startup.",
            "CORRECTION: PCA state is saved to brain.db after first fit and loaded on subsequent startups.",
            1340, 1352, 6, 8
        ),
        (
            "Edge weights in the ConnectionGraph start at 1.0 for all new edges.",
            "CORRECTION: Association edges start at cosine similarity value (0.4-1.0). Temporal edges start at max(cosine_sim, 0.1).",
            1360, 1372, 5, 7
        ),
        (
            "The memory importance field controls retrieval priority directly.",
            "CORRECTION: Importance affects decay rate and tie-breaking. Retrieval priority is determined by activation score.",
            1380, 1392, 6, 8
        ),
        (
            "Heredoc over SSH is a reliable way to write files on remote servers.",
            "CORRECTION: Heredoc over SSH truncates files to 0 bytes in practice. Always use SCP. This is a documented gotcha in AGENTS.md.",
            1400, 1412, 4, 9
        ),
        (
            "Rootless Podman containers access host files using normal Unix permissions.",
            "CORRECTION: Rootless Podman uses user namespace mapping. Files may be owned by UID 100000+. Must chown to mapped UID.",
            1420, 1432, 5, 8
        ),
        (
            "The Hopfield substrate retrieves exact matches for query embeddings.",
            "CORRECTION: Hopfield retrieval is approximate -- it finds nearest attractors via energy minimization. Not exact lookup.",
            1440, 1452, 6, 8
        ),
        (
            "All graph edges are bidirectional by default when created.",
            "CORRECTION: Edges are unidirectional in ConnectionGraph.add_edge(). absorb_memory creates bidirectional pairs explicitly.",
            1460, 1472, 5, 7
        ),
        (
            "Memory decay happens automatically in real-time as time passes.",
            "CORRECTION: Decay is applied only on explicit decay_tick commands. The TypeScript manager sends decay_tick periodically.",
            1480, 1492, 5, 8
        ),
    ];

    for (wrong, correction, h1, h2, imp1, imp2) in corrections {
        let id1 = next_id; next_id -= 1;
        let id2 = next_id; next_id -= 1;

        memories.push(SyntheticMemory {
            id: id1,
            content: wrong.to_string(),
            category: "state".to_string(),
            importance: *imp1,
            created_at: format_date(*h1),
            embedding: make_embedding(wrong),
        });

        memories.push(SyntheticMemory {
            id: id2,
            content: correction.to_string(),
            category: "correction".to_string(),
            importance: *imp2,
            created_at: format_date(*h2),
            embedding: make_embedding(correction),
        });

        edges.push(SyntheticEdge { source_id: id1, target_id: id2, weight: 0.8, edge_type: "contradiction".to_string() });
        edges.push(SyntheticEdge { source_id: id2, target_id: id1, weight: 0.8, edge_type: "contradiction".to_string() });
        edges.push(SyntheticEdge { source_id: id1, target_id: id2, weight: 0.6, edge_type: "temporal".to_string() });
    }

    InstinctsCorpus {
        version: 1,
        generated_at: "2026-01-01T00:00:00Z".to_string(),
        memories,
        edges,
    }
}

// ---- Serialize / Deserialize ----

pub fn save_instincts(corpus: &InstinctsCorpus, path: &str) -> Result<(), String> {
    use flate2::write::GzEncoder;
    use flate2::Compression;

    let json_bytes =
        serde_json::to_vec(corpus).map_err(|e| format!("serialize error: {}", e))?;

    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder
        .write_all(&json_bytes)
        .map_err(|e| format!("compress error: {}", e))?;
    let compressed = encoder
        .finish()
        .map_err(|e| format!("compress finish error: {}", e))?;

    let compressed_len = compressed.len() as u32;

    let mut out = Vec::new();
    out.extend_from_slice(INST_MAGIC);
    out.extend_from_slice(&INST_VERSION.to_le_bytes());
    out.extend_from_slice(&compressed_len.to_le_bytes());
    out.extend_from_slice(&compressed);

    std::fs::write(path, &out).map_err(|e| format!("write error: {}", e))?;
    Ok(())
}

pub fn load_instincts(path: &str) -> Option<InstinctsCorpus> {
    use flate2::read::GzDecoder;

    if !Path::new(path).exists() {
        return None;
    }

    let data = std::fs::read(path).ok()?;
    if data.len() < 12 {
        eprintln!("[instincts] file too small: {}", path);
        return None;
    }

    if &data[0..4] != INST_MAGIC {
        eprintln!("[instincts] invalid magic in: {}", path);
        return None;
    }

    let _version = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);
    let compressed_len =
        u32::from_le_bytes([data[8], data[9], data[10], data[11]]) as usize;

    if data.len() < 12 + compressed_len {
        eprintln!("[instincts] truncated data in: {}", path);
        return None;
    }

    let compressed = &data[12..12 + compressed_len];
    let mut decoder = GzDecoder::new(compressed);
    let mut json_bytes = Vec::new();
    if let Err(e) = decoder.read_to_end(&mut json_bytes) {
        eprintln!("[instincts] decompress error: {}", e);
        return None;
    }

    match serde_json::from_slice::<InstinctsCorpus>(&json_bytes) {
        Ok(corpus) => Some(corpus),
        Err(e) => {
            eprintln!("[instincts] parse error: {}", e);
            None
        }
    }
}

// ---- Apply instincts to brain ----

pub fn apply_instincts(
    memories: &mut Vec<BrainMemory>,
    memory_index: &mut HashMap<i64, usize>,
    substrate: &mut HopfieldSubstrate,
    graph: &mut ConnectionGraph,
    pca: &crate::pca::PcaTransform,
    corpus: &InstinctsCorpus,
) {
    eprintln!(
        "[instincts] applying {} ghost patterns",
        corpus.memories.len()
    );

    for syn_mem in &corpus.memories {
        if memory_index.contains_key(&syn_mem.id) {
            continue;
        }

        let raw = Array1::from(syn_mem.embedding.clone());
        let pattern = pca.project(&raw);

        let brain_mem = BrainMemory {
            id: syn_mem.id,
            content: syn_mem.content.clone(),
            category: syn_mem.category.clone(),
            source: "instinct".to_string(),
            importance: syn_mem.importance,
            created_at: syn_mem.created_at.clone(),
            embedding: syn_mem.embedding.clone(),
            pattern: pattern.clone(),
            activation: GHOST_STRENGTH,
            last_activated: 0.0,
            access_count: 0,
            decay_factor: GHOST_STRENGTH,
            tags: vec!["ghost".to_string()],
        };

        substrate.store(syn_mem.id, &pattern, GHOST_STRENGTH);
        graph.add_node(syn_mem.id);

        let idx = memories.len();
        memory_index.insert(syn_mem.id, idx);
        memories.push(brain_mem);
    }

    for edge in &corpus.edges {
        let etype = EdgeType::from_str(&edge.edge_type);
        graph.add_edge(edge.source_id, edge.target_id, edge.weight, etype);
    }

    eprintln!(
        "[instincts] applied {} ghosts, {} edges",
        corpus.memories.len(),
        corpus.edges.len()
    );
}

// ---- Ghost replacement check ----
// Called during absorb when a real memory is added.
// Removes any ghost with cosine_sim > GHOST_REPLACE_SIM to the new real memory.

pub fn check_ghost_replacement(
    new_pattern: &Array1<f32>,
    memories: &mut Vec<BrainMemory>,
    memory_index: &mut HashMap<i64, usize>,
    substrate: &mut HopfieldSubstrate,
    graph: &mut ConnectionGraph,
) -> usize {
    let mut to_remove: Vec<i64> = Vec::new();

    for mem in memories.iter() {
        if mem.id >= 0 {
            continue;
        }
        if mem.pattern.len() == 0 {
            continue;
        }
        let sim = cosine_sim_arrays(new_pattern, &mem.pattern);
        if sim > GHOST_REPLACE_SIM {
            to_remove.push(mem.id);
        }
    }

    let count = to_remove.len();

    for ghost_id in &to_remove {
        substrate.remove(*ghost_id);

        graph.adjacency.remove(ghost_id);
        for adj in graph.adjacency.values_mut() {
            adj.retain(|(tid, _, _)| *tid != *ghost_id);
        }
        graph.nodes.remove(ghost_id);

        if let Some(&idx) = memory_index.get(ghost_id) {
            let last_idx = memories.len() - 1;
            if idx < last_idx {
                memories.swap(idx, last_idx);
                let swapped_id = memories[idx].id;
                memory_index.insert(swapped_id, idx);
            }
            memories.pop();
            memory_index.remove(ghost_id);
        }
    }

    count
}

fn cosine_sim_arrays(a: &Array1<f32>, b: &Array1<f32>) -> f32 {
    if a.len() != b.len() || a.len() == 0 {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let na: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let nb: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if na < 1e-10 || nb < 1e-10 {
        return 0.0;
    }
    dot / (na * nb)
}

// ---- Tests ----

#[cfg(test)]
mod tests {
    use super::*;
    use ndarray::Array2;
    use crate::pca::PcaTransform;
    use tempfile::NamedTempFile;

    fn build_test_pca() -> PcaTransform {
        let n = 20usize;
        let mut data = Array2::<f32>::zeros((n, RAW_DIM));
        for i in 0..n {
            for j in 0..RAW_DIM {
                data[[i, j]] = ((i as f32 * 0.3 + j as f32 * 0.07) * std::f32::consts::PI).sin();
            }
        }
        PcaTransform::fit(&data)
    }

    #[test]
    fn test_generate_corpus() {
        let corpus = generate_instincts();

        assert!(
            corpus.memories.len() >= 100,
            "expected at least 100 memories, got {}",
            corpus.memories.len()
        );

        for mem in &corpus.memories {
            assert!(mem.id < 0, "ghost ID should be negative, got {}", mem.id);
        }

        let categories: std::collections::HashSet<&str> =
            corpus.memories.iter().map(|m| m.category.as_str()).collect();
        assert!(
            categories.len() >= 4,
            "expected at least 4 categories, got {:?}",
            categories
        );

        assert!(!corpus.edges.is_empty(), "expected edges in corpus");

        for mem in &corpus.memories {
            assert_eq!(
                mem.embedding.len(),
                RAW_DIM,
                "embedding should be {} dims, got {} for memory {}",
                RAW_DIM,
                mem.embedding.len(),
                mem.id
            );
        }

        // Determinism check
        let corpus2 = generate_instincts();
        assert_eq!(corpus.memories.len(), corpus2.memories.len());
        for (a, b) in corpus.memories.iter().zip(corpus2.memories.iter()) {
            assert_eq!(a.id, b.id);
            assert_eq!(a.content, b.content);
            let diff: f32 = a.embedding.iter().zip(b.embedding.iter())
                .map(|(x, y)| (x - y).abs()).sum();
            assert!(diff < 1e-6, "embeddings should be deterministic");
        }
    }

    #[test]
    fn test_save_load_roundtrip() {
        let corpus = generate_instincts();
        let tmp = NamedTempFile::new().expect("temp file");
        let path = tmp.path().to_str().expect("path");

        save_instincts(&corpus, path).expect("save should succeed");

        let metadata = std::fs::metadata(path).expect("file should exist");
        assert!(metadata.len() > 100, "file should have content");

        let loaded = load_instincts(path).expect("load should succeed");

        assert_eq!(corpus.version, loaded.version);
        assert_eq!(corpus.memories.len(), loaded.memories.len());
        assert_eq!(corpus.edges.len(), loaded.edges.len());
        assert_eq!(corpus.memories[0].content, loaded.memories[0].content);

        let orig_emb = &corpus.memories[0].embedding;
        let load_emb = &loaded.memories[0].embedding;
        let diff: f32 = orig_emb.iter().zip(load_emb.iter()).map(|(a, b)| (a - b).abs()).sum();
        assert!(diff < 1e-4, "embedding should survive roundtrip, diff={}", diff);
    }

    #[test]
    fn test_ghost_patterns() {
        let corpus = generate_instincts();
        let pca = build_test_pca();

        let mut memories: Vec<BrainMemory> = Vec::new();
        let mut memory_index: HashMap<i64, usize> = HashMap::new();
        let mut substrate = HopfieldSubstrate::new();
        let mut graph = ConnectionGraph::new();

        apply_instincts(
            &mut memories,
            &mut memory_index,
            &mut substrate,
            &mut graph,
            &pca,
            &corpus,
        );

        for mem in &memories {
            assert!(mem.id < 0, "ghost ID should be negative, got {}", mem.id);
        }

        for mem in &memories {
            assert!(
                (mem.decay_factor - GHOST_STRENGTH).abs() < 0.01,
                "ghost decay_factor should be {}, got {} for id {}",
                GHOST_STRENGTH, mem.decay_factor, mem.id
            );
        }

        assert!(substrate.n_patterns() > 0, "substrate should have ghost patterns");
        assert_eq!(memories.len(), memory_index.len());
        for (id, &idx) in &memory_index {
            assert_eq!(memories[idx].id, *id);
        }
    }

    #[test]
    fn test_ghost_replacement() {
        let corpus = generate_instincts();
        let pca = build_test_pca();

        let mut memories: Vec<BrainMemory> = Vec::new();
        let mut memory_index: HashMap<i64, usize> = HashMap::new();
        let mut substrate = HopfieldSubstrate::new();
        let mut graph = ConnectionGraph::new();

        apply_instincts(
            &mut memories,
            &mut memory_index,
            &mut substrate,
            &mut graph,
            &pca,
            &corpus,
        );

        let ghost_count_before = memories.iter().filter(|m| m.id < 0).count();
        let first_ghost = memories.iter().find(|m| m.id < 0).cloned().unwrap();
        let near_pattern = first_ghost.pattern.clone();
        let first_ghost_id = first_ghost.id;

        let removed = check_ghost_replacement(
            &near_pattern,
            &mut memories,
            &mut memory_index,
            &mut substrate,
            &mut graph,
        );

        assert!(removed > 0, "at least one ghost should be replaced");

        let ghost_count_after = memories.iter().filter(|m| m.id < 0).count();
        assert!(
            ghost_count_after < ghost_count_before,
            "ghost count should decrease: before={}, after={}",
            ghost_count_before, ghost_count_after
        );

        assert!(
            !memory_index.contains_key(&first_ghost_id),
            "replaced ghost should not be in memory_index"
        );

        assert_eq!(memories.len(), memory_index.len());
        for (id, &idx) in &memory_index {
            assert_eq!(memories[idx].id, *id, "memory_index consistency failed");
        }
    }
}
