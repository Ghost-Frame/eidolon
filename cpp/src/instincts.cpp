// instincts.cpp -- synthetic pre-training corpus implementation

#include "brain/instincts.hpp"
#include "brain/absorb.hpp"

#include <nlohmann/json.hpp>
#include <zlib.h>

#include <cstdio>
#include <cstring>
#include <cmath>
#include <fstream>
#include <sstream>
#include <algorithm>
#include <numeric>
#include <unordered_set>

using json = nlohmann::json;

namespace brain {

// ---- Binary file format ----
// [4B magic "INST"][4B version LE][4B compressed_len LE][compressed gzip JSON]

static const char INST_MAGIC[4] = {'I','N','S','T'};
static const uint32_t INST_VERSION = 1;

// ---- Deterministic embedding generation ----

static uint64_t hash_content(const std::string& content) {
    // FNV-1a 64-bit
    uint64_t h = 14695981039346656037ULL;
    for (unsigned char c : content) {
        h ^= (uint64_t)c;
        h *= 1099511628211ULL;
    }
    return h;
}

static std::vector<float> make_embedding(const std::string& content) {
    uint64_t seed = hash_content(content);
    std::vector<float> emb(RAW_DIM);
    for (int i = 0; i < RAW_DIM; ++i) {
        uint64_t val = seed * 6364136223846793005ULL
                     + (uint64_t)i * 1442695040888963407ULL;
        float angle = (float)val * (float)(M_PI / (double)(0xFFFFFFFFu) / 2.0);
        emb[i] = std::sin(angle);
    }
    // L2 normalize
    float sq_sum = 0.0f;
    for (float v : emb) sq_sum += v * v;
    float norm = std::sqrt(sq_sum);
    if (norm > 1e-10f) {
        for (float& v : emb) v /= norm;
    }
    return emb;
}

// ---- Date helpers ----

static bool is_leap(int year) {
    return (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0);
}

static std::tuple<int,int,int> days_to_ymd(int64_t days_from_epoch) {
    int64_t remaining = days_from_epoch;
    int year = 1970;
    while (true) {
        int diy = is_leap(year) ? 366 : 365;
        if (remaining < diy) break;
        remaining -= diy;
        ++year;
    }
    static const int month_days_normal[12] = {31,28,31,30,31,30,31,31,30,31,30,31};
    static const int month_days_leap[12]   = {31,29,31,30,31,30,31,31,30,31,30,31};
    const int* md = is_leap(year) ? month_days_leap : month_days_normal;
    int month = 1;
    for (int i = 0; i < 12; ++i) {
        if (remaining < md[i]) break;
        remaining -= md[i];
        ++month;
    }
    return {year, month, (int)(remaining + 1)};
}

static std::string format_date(int64_t offset_hours) {
    // Base: 2026-01-15T00:00:00Z = 1768521600 seconds
    int64_t base = 1768521600LL;
    int64_t ts   = base + offset_hours * 3600LL;
    int64_t secs = ts;
    int64_t mins = secs / 60;
    int64_t hrs  = mins / 60;
    int64_t days = hrs / 24;
    int s = (int)(secs % 60);
    int m = (int)(mins % 60);
    int h = (int)(hrs  % 24);
    auto [year, month, day] = days_to_ymd(days);
    char buf[32];
    snprintf(buf, sizeof(buf), "%04d-%02d-%02dT%02d:%02d:%02dZ",
             year, month, day, h, m, s);
    return std::string(buf);
}

// ---- Corpus generator ----

InstinctsCorpus generate_instincts() {
    InstinctsCorpus corpus;
    corpus.version = 1;
    corpus.generated_at = "2026-01-01T00:00:00Z";
    corpus.memories.reserve(200);
    corpus.edges.reserve(300);

    int64_t next_id = -1;

    auto push_mem = [&](int64_t id, const std::string& content,
                        const std::string& category, int importance,
                        int64_t offset_hours) {
        SyntheticMemory m;
        m.id = id;
        m.content = content;
        m.category = category;
        m.importance = importance;
        m.created_at = format_date(offset_hours);
        m.embedding = make_embedding(content);
        corpus.memories.push_back(std::move(m));
    };

    auto push_edge = [&](int64_t src, int64_t tgt, float w, const std::string& type) {
        corpus.edges.push_back({src, tgt, w, type});
    };

    // ---- Category 1: Infrastructure state transitions (10 sets x 4 = 40) ----
    struct InfraSet { const char* old_ver; const char* new_ver; const char* service; const char* desc; int64_t base; };
    InfraSet infra_sets[] = {
        {"nginx v1.18", "nginx v1.24", "web-proxy", "HTTP reverse proxy", 0},
        {"PostgreSQL 13", "PostgreSQL 16", "db-primary", "primary database", 48},
        {"Redis 6.2", "Redis 7.2", "cache-layer", "session cache", 96},
        {"Node.js 18", "Node.js 22", "api-server", "REST API runtime", 144},
        {"Docker 20.10", "Podman 4.9", "container-runtime", "container orchestration", 192},
        {"Python 3.10", "Python 3.12", "worker-service", "background task runner", 240},
        {"Elasticsearch 7", "OpenSearch 2.11", "search-cluster", "full-text search index", 288},
        {"RabbitMQ 3.10", "RabbitMQ 3.12", "message-broker", "async job queue", 336},
        {"Traefik 2.x", "Traefik 3.x", "ingress-controller", "TLS termination and routing", 384},
        {"Grafana 9", "Grafana 11", "monitoring-ui", "metrics dashboard", 432},
    };

    for (auto& s : infra_sets) {
        int64_t id1 = next_id--;
        std::string c1 = std::string(s.service) + " is running " + s.old_ver + " on " + s.service +
                         " -- " + s.desc + ". Deployed 2026-01-15. Status: stable.";
        push_mem(id1, c1, "state", 5, s.base);

        int64_t id2 = next_id--;
        std::string c2 = std::string("Decision to migrate ") + s.service + " from " + s.old_ver +
                         " to " + s.new_ver + " on " + s.service +
                         ". Reason: upstream EOL and security patches. Scheduled downtime: 30 minutes.";
        push_mem(id2, c2, "decision", 7, s.base + 12);

        int64_t id3 = next_id--;
        std::string c3 = std::string(s.service) + " successfully migrated to " + s.new_ver +
                         " on " + s.service + ". Migration completed 2026-01. All health checks passing. Previous version " +
                         s.old_ver + " decommissioned.";
        push_mem(id3, c3, "task", 8, s.base + 24);

        int64_t id4 = next_id--;
        std::string c4 = std::string(s.service) + " is NOW running " + s.new_ver + " on " + s.service +
                         " -- " + s.desc + ". Upgraded from " + s.old_ver + ". Status: stable, verified.";
        push_mem(id4, c4, "state", 9, s.base + 25);

        push_edge(id1, id2, 0.8f, "temporal");
        push_edge(id2, id3, 0.8f, "temporal");
        push_edge(id3, id4, 0.8f, "temporal");
        push_edge(id1, id4, 0.7f, "contradiction");
    }

    // ---- Category 2: Decision records (10 sets x 2 = 20) ----
    struct DecisionSet { const char* domain; const char* title; const char* rationale; const char* outcome; int64_t base; };
    DecisionSet decisions[] = {
        {"database", "Use PostgreSQL over MySQL for the primary store",
         "ACID compliance, better JSON support, and superior indexing. MySQL replication lag unacceptable for consistency requirements.",
         "PostgreSQL selected. MySQL evaluated and rejected.", 500},
        {"caching", "Use Redis over Memcached for session caching",
         "Redis supports persistence, pub/sub, and sorted sets needed for leaderboards. Memcached is volatile-only.",
         "Redis deployed for sessions and pub/sub.", 520},
        {"container", "Switch from Docker to Podman for production workloads",
         "Rootless Podman reduces attack surface. Docker daemon single point of failure eliminated. OCI compatible.",
         "Podman adopted. Docker daemon removed from production nodes.", 540},
        {"monitoring", "Adopt Prometheus and Grafana over Datadog",
         "Cost: Datadog $3k/month vs self-hosted $50/month infra cost. Prometheus retention and alerting fully customizable.",
         "Prometheus stack deployed. Datadog subscription cancelled.", 560},
        {"proxy", "Use Traefik over HAProxy for ingress",
         "Traefik integrates with Docker service discovery. HAProxy requires manual config for each backend.",
         "Traefik deployed as ingress. HAProxy configs archived.", 580},
        {"logging", "Use Loki over Elasticsearch for log aggregation",
         "Loki label-based indexing 10x cheaper at scale. Elasticsearch full-text not needed for structured logs.",
         "Loki deployed. Elasticsearch retained only for search features.", 600},
        {"queue", "Use RabbitMQ over Kafka for job processing",
         "Kafka overhead unjustified for sub-10k msg/sec. RabbitMQ simpler ops and sufficient throughput.",
         "RabbitMQ in production. Kafka evaluated for future data pipeline.", 620},
        {"cdn", "Self-host Nginx for static assets over CloudFront",
         "Data transfer costs $800/month on CloudFront. Nginx on dedicated node costs $40/month.",
         "Nginx static asset server deployed. CloudFront distribution disabled.", 640},
        {"auth", "Implement JWT with refresh tokens over session cookies",
         "Stateless JWT enables horizontal scaling without session store. Refresh token rotation provides security equivalent.",
         "JWT auth implemented. Session store removed from architecture.", 660},
        {"backup", "Use restic over duplicati for backup strategy",
         "restic deduplication more efficient. CLI-first design fits automation.",
         "restic deployed on all nodes. Automated daily snapshots verified.", 680},
    };

    for (auto& d : decisions) {
        int64_t id1 = next_id--;
        std::string c1 = std::string("Architecture decision [") + d.domain + "]: " + d.title + ". Rationale: " + d.rationale;
        push_mem(id1, c1, "decision", 8, d.base);

        int64_t id2 = next_id--;
        std::string c2 = std::string("Decision outcome [") + d.domain + "]: " + d.outcome + " Implemented and verified in production.";
        push_mem(id2, c2, "task", 7, d.base + 24);

        push_edge(id1, id2, 0.75f, "association");
        push_edge(id2, id1, 0.75f, "association");
    }

    // ---- Category 3: Discovery / reference (20 memories) ----
    struct RefEntry { const char* category; const char* content; int64_t base; int importance; };
    RefEntry references[] = {
        {"server-specs", "app-server-1: 8 vCPU, 32GB RAM, 500GB NVMe SSD, Ubuntu 22.04. Role: application services backend.", 700, 8},
        {"server-specs", "edge-server-1: 4 vCPU, 16GB RAM, 200GB SSD, Rocky Linux 9. Role: CDN edge node and static assets.", 702, 7},
        {"server-specs", "dev-workstation: Xeon W-2125 4.0GHz 8-core, 30GB RAM, 2TB HDD. Role: primary development and build machine.", 704, 9},
        {"endpoint", "Engram memory API: POST /store to persist memories, POST /search to query, GET /recall for recent. Auth via Bearer token.", 710, 8},
        {"endpoint", "Eidolon brain API: JSON over stdio. Commands: init, query, absorb, decay_tick, dream_cycle, get_stats, shutdown.", 712, 9},
        {"filepath", "Brain database location: \/brain.db -- SQLite, contains memories, edges, pca_state tables.", 714, 8},
        {"filepath", "Instincts binary: \/instincts.bin -- gzip-compressed JSON corpus, applied on first init when brain.db is empty.", 716, 7},
        {"filepath", "Eidolon Rust source: src/ directory of eidolon-lib crate -- substrate.rs, graph.rs, dreaming.rs, instincts.rs, main.rs.", 718, 6},
        {"credential", "SSH key for all servers: operator-configured SSH key. Custom ports may apply for specific servers.", 720, 9},
        {"network", "VPN mesh subnet: configured in operator mesh network. All nodes reachable by mesh IP. Use internal IPs for inter-service traffic.", 722, 8},
        {"config", "Nginx config directory: /etc/nginx/sites-enabled/. Reload: nginx -t && systemctl reload nginx. Never restart without testing.", 730, 7},
        {"config", "PostgreSQL data directory: /var/lib/postgresql/16/main/. Config: /etc/postgresql/16/main/postgresql.conf.", 732, 7},
        {"config", "Redis config: /etc/redis/redis.conf. Bind 127.0.0.1 only. requirepass enabled. maxmemory-policy allkeys-lru.", 734, 6},
        {"pattern", "Service restart pattern: check state -> back up config -> stop service -> apply change -> start service -> verify health -> monitor logs.", 740, 9},
        {"pattern", "File deployment pattern: write locally -> SCP to /tmp/ -> SSH mv to destination -> set permissions -> verify.", 742, 8},
        {"pattern", "Never use heredoc over SSH for file content -- truncates to 0 bytes. Always use SCP for file transfers to remote hosts.", 744, 9},
        {"pattern", "CrowdSec is the intrusion detection system on all nodes. Never install fail2ban. CrowdSec bouncer handles blocking.", 746, 8},
        {"error", "podman cp truncates heredoc content -- root cause: shell expansion in subprocess. Fix: scp local file then podman cp from host.", 750, 8},
        {"error", "Unix socket stale fd: when upstream restarts, downstream holds old fd. Both must restart in order: upstream first, then downstream.", 752, 7},
        {"error", "SELinux blocks unexpected service access -- check ausearch -m avc -ts recent. Fix: restorecon -Rv /path or semanage.", 754, 6},
    };

    for (auto& r : references) {
        int64_t id = next_id--;
        push_mem(id, r.content, r.category, r.importance, r.base);
    }

    // ---- Category 4: Task completions (20 memories) ----
    struct TaskEntry { const char* content; int64_t base; int importance; };
    TaskEntry tasks[] = {
        {"Deploy Eidolon brain substrate Phase 1 to Rocky Linux. Result: binary at /opt/eidolon/eidolon/rust/target/release/eidolon. brain.db initialized with 847 memories from Engram export.", 800, 9},
        {"Migrate Engram database from SQLite to PostgreSQL. Result: 12,847 memories migrated, 0 data loss, query time improved from 45ms to 8ms at p99.", 810, 8},
        {"Fix memory leak in Eidolon decay module. Root cause: Vec not cleared after prune. Fix: add memory.retain() after dead_set removal. Leak eliminated.", 820, 9},
        {"Set up Traefik TLS termination for all subdomains. Let Encrypt wildcard cert via DNS challenge. All subdomains now HTTPS.", 830, 8},
        {"Configure Prometheus scrape targets for all nodes. Added: node_exporter, postgres_exporter, redis_exporter, nginx_exporter.", 840, 7},
        {"Deploy CrowdSec on application and edge servers. Installed bouncer for nginx. Community blocklist active. First 24h: 1,247 IPs blocked.", 850, 8},
        {"Upgrade PostgreSQL 13 to 16 on db-primary. pg_upgrade used for in-place upgrade. Backup taken before: pg_dump 18GB. Zero data loss.", 860, 9},
        {"Implement JWT refresh token rotation in API server. Old refresh tokens invalidated on use. 7-day token expiry. Redis TTL set.", 870, 7},
        {"Set up automated restic backups on all nodes. Daily snapshots at 02:00 UTC. Retention: 7 daily, 4 weekly, 12 monthly.", 880, 8},
        {"Debug and fix Nginx upstream 502 errors. Root cause: backend pool exhausted due to connection leak in Python worker.", 890, 9},
        {"Enable TCP BBR on all Linux servers via sysctl. net.ipv4.tcp_congestion_control=bbr. p99 latency -23%, throughput +18%.", 900, 7},
        {"Consolidate application and edge server nginx configs into shared template. Reduced config duplication from 4 files to 1 template.", 910, 6},
        {"Add memory decay monitoring to Engram dashboard. Alert when avg_decay_factor < 0.3. Grafana panel showing health_distribution over time.", 920, 7},
        {"Write Eidolon dreaming module. Implements: replay_recent, merge_redundant, prune_dead, discover_connections, resolve_lingering.", 930, 9},
        {"Write Eidolon instincts module. Generates 200 synthetic ghost patterns across 5 categories. Ghost replacement on cosine_sim > 0.85.", 940, 9},
        {"Implement Hopfield substrate in Rust with ndarray. PCA projection 1024->512 dims. Retrieval via softmax activation.", 950, 9},
        {"Add graph spread to Eidolon query pipeline. BFS from Hopfield seeds, 3 hops, decay 0.5/hop. Contradiction resolution.", 960, 8},
        {"Deploy Eidolon C++ backend as alternative to Rust. Same JSON protocol. Eigen3 for linear algebra.", 970, 7},
        {"Fix race condition in dream cycle pruning. Root cause: iterating memories while removing. Fix: collect dead_ids first.", 980, 8},
        {"Optimize PCA projection in Eidolon. Moved from per-query projection to cached patterns. Query time improved from 8ms to 2ms p99.", 990, 8},
    };

    for (auto& t : tasks) {
        int64_t id = next_id--;
        push_mem(id, t.content, "task", t.importance, t.base);
    }

    // ---- Category 5: Corrections (20 pairs x 2 = 40) ----
    struct CorrEntry { const char* wrong; const char* correction; int64_t h1; int64_t h2; int imp1; int imp2; };
    CorrEntry corrections[] = {
        {"app-server-1 is running Ubuntu 20.04 LTS.",
         "CORRECTION: app-server-1 is running Ubuntu 22.04 LTS, not 20.04. Upgraded 2025-11. Verify with: lsb_release -a.",
         1100, 1112, 6, 8},
        {"Redis is configured to bind to 0.0.0.0 for inter-service access.",
         "CORRECTION: Redis binds to 127.0.0.1 ONLY. Inter-service access via Unix socket or SSH tunnel. Binding 0.0.0.0 was a security incident.",
         1120, 1132, 5, 9},
        {"PostgreSQL replication lag is acceptable at 2-3 seconds for read replicas.",
         "CORRECTION: PostgreSQL replication lag target is under 500ms, not 2-3 seconds. Alert threshold: 1000ms.",
         1140, 1152, 5, 8},
        {"fail2ban is installed on all nodes for SSH protection.",
         "CORRECTION: CrowdSec is used, NOT fail2ban. fail2ban was removed in 2025-10. CrowdSec bouncer handles all blocking.",
         1160, 1172, 4, 9},
        {"The SSH private key location is ~/.ssh/id_ed25519 on all machines.",
         "CORRECTION: Use the operator-configured SSH key for all server logins. Some servers may require custom ports.",
         1180, 1192, 5, 9},
        {"Eidolon brain.db is stored at /var/lib/eidolon/brain.db.",
         "CORRECTION: brain.db is at \/brain.db, not /var/lib/eidolon/. The data_dir is \/.",
         1200, 1212, 5, 8},
        {"Memory patterns are stored in full 1024-dimensional space in the Hopfield substrate.",
         "CORRECTION: Patterns are PCA-projected to 512 dimensions before Hopfield storage. Raw 1024-dim embeddings stored in brain.db.",
         1220, 1232, 6, 9},
        {"Dream cycles run during active query processing to consolidate recent memories.",
         "CORRECTION: Dream cycles run ONLY during idle periods. TypeScript coordinator pauses dreaming when query activity detected.",
         1240, 1252, 5, 8},
        {"Ghost patterns from instincts have the same decay rate as real memories.",
         "CORRECTION: Ghost patterns decay at 2x the rate of real memories. Ghost strength starts at 0.3 vs 0.5 for real.",
         1260, 1272, 6, 9},
        {"Association edges are created between all memory pairs with cosine similarity above 0.3.",
         "CORRECTION: Association threshold is 0.4, not 0.3. Contradiction threshold is 0.75. Max 15 edges per memory.",
         1280, 1292, 5, 7},
        {"The Eidolon binary accepts HTTP REST API requests on port 7433.",
         "CORRECTION: Eidolon binary uses JSON over stdio, NOT HTTP. TypeScript manager wraps the binary in a subprocess.",
         1300, 1312, 5, 9},
        {"Engram stores memories in a custom binary format for performance.",
         "CORRECTION: Engram stores memories in SQLite or PostgreSQL. The instincts.bin file is separate and not the main Engram storage.",
         1320, 1332, 5, 7},
        {"The PCA transform is recomputed from scratch on every Eidolon startup.",
         "CORRECTION: PCA state is saved to brain.db after first fit and loaded on subsequent startups.",
         1340, 1352, 6, 8},
        {"Edge weights in the ConnectionGraph start at 1.0 for all new edges.",
         "CORRECTION: Association edges start at cosine similarity value (0.4-1.0). Temporal edges start at max(cosine_sim, 0.1).",
         1360, 1372, 5, 7},
        {"The memory importance field controls retrieval priority directly.",
         "CORRECTION: Importance affects decay rate and tie-breaking. Retrieval priority is determined by activation score.",
         1380, 1392, 6, 8},
        {"Heredoc over SSH is a reliable way to write files on remote servers.",
         "CORRECTION: Heredoc over SSH truncates files to 0 bytes in practice. Always use SCP. This is a documented gotcha in AGENTS.md.",
         1400, 1412, 4, 9},
        {"Rootless Podman containers access host files using normal Unix permissions.",
         "CORRECTION: Rootless Podman uses user namespace mapping. Files may be owned by UID 100000+. Must chown to mapped UID.",
         1420, 1432, 5, 8},
        {"The Hopfield substrate retrieves exact matches for query embeddings.",
         "CORRECTION: Hopfield retrieval is approximate -- it finds nearest attractors via energy minimization. Not exact lookup.",
         1440, 1452, 6, 8},
        {"All graph edges are bidirectional by default when created.",
         "CORRECTION: Edges are unidirectional in ConnectionGraph.add_edge(). absorb_memory creates bidirectional pairs explicitly.",
         1460, 1472, 5, 7},
        {"Memory decay happens automatically in real-time as time passes.",
         "CORRECTION: Decay is applied only on explicit decay_tick commands. The TypeScript manager sends decay_tick periodically.",
         1480, 1492, 5, 8},
    };

    for (auto& c : corrections) {
        int64_t id1 = next_id--;
        int64_t id2 = next_id--;

        push_mem(id1, c.wrong,      "state",      c.imp1, c.h1);
        push_mem(id2, c.correction, "correction", c.imp2, c.h2);

        push_edge(id1, id2, 0.8f, "contradiction");
        push_edge(id2, id1, 0.8f, "contradiction");
        push_edge(id1, id2, 0.6f, "temporal");
    }

    return corpus;
}

// ---- JSON serialization helpers ----

static json corpus_to_json(const InstinctsCorpus& corpus) {
    json j;
    j["version"] = corpus.version;
    j["generated_at"] = corpus.generated_at;

    json mems = json::array();
    for (auto& m : corpus.memories) {
        json jm;
        jm["id"] = m.id;
        jm["content"] = m.content;
        jm["category"] = m.category;
        jm["importance"] = m.importance;
        jm["created_at"] = m.created_at;
        jm["embedding"] = m.embedding;
        mems.push_back(std::move(jm));
    }
    j["memories"] = std::move(mems);

    json edgs = json::array();
    for (auto& e : corpus.edges) {
        json je;
        je["source_id"] = e.source_id;
        je["target_id"] = e.target_id;
        je["weight"] = e.weight;
        je["edge_type"] = e.edge_type;
        edgs.push_back(std::move(je));
    }
    j["edges"] = std::move(edgs);

    return j;
}

static InstinctsCorpus json_to_corpus(const json& j) {
    InstinctsCorpus corpus;
    corpus.version = j.value("version", 1u);
    corpus.generated_at = j.value("generated_at", std::string(""));

    if (j.contains("memories") && j["memories"].is_array()) {
        for (auto& jm : j["memories"]) {
            SyntheticMemory m;
            m.id = jm.value("id", (int64_t)0);
            m.content = jm.value("content", std::string(""));
            m.category = jm.value("category", std::string(""));
            m.importance = jm.value("importance", 5);
            m.created_at = jm.value("created_at", std::string(""));
            if (jm.contains("embedding") && jm["embedding"].is_array()) {
                for (auto& v : jm["embedding"]) m.embedding.push_back(v.get<float>());
            }
            corpus.memories.push_back(std::move(m));
        }
    }

    if (j.contains("edges") && j["edges"].is_array()) {
        for (auto& je : j["edges"]) {
            SyntheticEdge e;
            e.source_id = je.value("source_id", (int64_t)0);
            e.target_id = je.value("target_id", (int64_t)0);
            e.weight = je.value("weight", 0.5f);
            e.edge_type = je.value("edge_type", std::string("association"));
            corpus.edges.push_back(std::move(e));
        }
    }

    return corpus;
}

// ---- Save / Load ----

bool save_instincts(const InstinctsCorpus& corpus, const std::string& path) {
    // Serialize to JSON string
    json j = corpus_to_json(corpus);
    std::string json_str = j.dump();

    // Compress with zlib/gzip (deflate with gzip header via compressBound)
    uLongf src_len = (uLongf)json_str.size();
    uLongf dest_len = compressBound(src_len) + 32;
    std::vector<uint8_t> compressed(dest_len);

    // Use gzip format (windowBits = 15 + 16)
    z_stream zs{};
    zs.zalloc = Z_NULL;
    zs.zfree  = Z_NULL;
    zs.opaque = Z_NULL;
    int rc = deflateInit2(&zs, Z_DEFAULT_COMPRESSION, Z_DEFLATED,
                          15 + 16, // gzip header
                          8, Z_DEFAULT_STRATEGY);
    if (rc != Z_OK) {
        fprintf(stderr, "[instincts] deflateInit2 failed: %d\n", rc);
        return false;
    }

    zs.next_in   = (Bytef*)json_str.data();
    zs.avail_in  = (uInt)json_str.size();
    zs.next_out  = compressed.data();
    zs.avail_out = (uInt)dest_len;

    rc = deflate(&zs, Z_FINISH);
    deflateEnd(&zs);

    if (rc != Z_STREAM_END) {
        fprintf(stderr, "[instincts] deflate failed: %d\n", rc);
        return false;
    }

    size_t compressed_len = dest_len - zs.avail_out;

    // Write file: [INST][version u32 LE][len u32 LE][compressed bytes]
    std::ofstream ofs(path, std::ios::binary);
    if (!ofs) {
        fprintf(stderr, "[instincts] failed to open %s for writing\n", path.c_str());
        return false;
    }

    ofs.write(INST_MAGIC, 4);

    uint32_t ver = INST_VERSION;
    ofs.write(reinterpret_cast<char*>(&ver), 4);

    uint32_t clen = (uint32_t)compressed_len;
    ofs.write(reinterpret_cast<char*>(&clen), 4);

    ofs.write(reinterpret_cast<char*>(compressed.data()), (std::streamsize)compressed_len);

    return ofs.good();
}

std::optional<InstinctsCorpus> load_instincts(const std::string& path) {
    std::ifstream ifs(path, std::ios::binary);
    if (!ifs) return std::nullopt;

    // Read entire file
    std::vector<uint8_t> data((std::istreambuf_iterator<char>(ifs)),
                               std::istreambuf_iterator<char>());

    if (data.size() < 12) {
        fprintf(stderr, "[instincts] file too small: %s\n", path.c_str());
        return std::nullopt;
    }

    // Check magic
    if (memcmp(data.data(), INST_MAGIC, 4) != 0) {
        fprintf(stderr, "[instincts] invalid magic in: %s\n", path.c_str());
        return std::nullopt;
    }

    // Read compressed length
    uint32_t compressed_len;
    memcpy(&compressed_len, data.data() + 8, 4);

    if (data.size() < (size_t)(12 + compressed_len)) {
        fprintf(stderr, "[instincts] truncated data in: %s\n", path.c_str());
        return std::nullopt;
    }

    // Decompress
    const uint8_t* src = data.data() + 12;
    // Estimate decompressed size (10x compressed or at least 1MB)
    size_t buf_size = std::max((size_t)(compressed_len * 12), (size_t)(1 << 20));
    std::vector<uint8_t> json_bytes(buf_size);

    z_stream zs{};
    zs.zalloc = Z_NULL;
    zs.zfree  = Z_NULL;
    zs.opaque = Z_NULL;
    // 15+16 = gzip mode
    int rc = inflateInit2(&zs, 15 + 16);
    if (rc != Z_OK) {
        fprintf(stderr, "[instincts] inflateInit2 failed: %d\n", rc);
        return std::nullopt;
    }

    zs.next_in   = (Bytef*)src;
    zs.avail_in  = compressed_len;
    zs.next_out  = json_bytes.data();
    zs.avail_out = (uInt)buf_size;

    rc = inflate(&zs, Z_FINISH);
    size_t json_len = buf_size - zs.avail_out;
    inflateEnd(&zs);

    if (rc != Z_STREAM_END) {
        fprintf(stderr, "[instincts] inflate failed: %d\n", rc);
        return std::nullopt;
    }

    // Parse JSON
    try {
        std::string json_str((char*)json_bytes.data(), json_len);
        json j = json::parse(json_str);
        return json_to_corpus(j);
    } catch (const std::exception& ex) {
        fprintf(stderr, "[instincts] JSON parse error: %s\n", ex.what());
        return std::nullopt;
    }
}

// ---- Apply instincts ----

void apply_instincts(
    std::vector<BrainMemory>& memories,
    std::unordered_map<int64_t, size_t>& memory_index,
    HopfieldSubstrate& substrate,
    ConnectionGraph& graph,
    PcaTransform& pca,
    const InstinctsCorpus& corpus
) {
    fprintf(stderr, "[instincts] applying %zu ghost patterns\n", corpus.memories.size());

    for (auto& syn : corpus.memories) {
        if (memory_index.count(syn.id)) continue;

        // Project embedding
        Eigen::VectorXf pattern;
        if ((int)syn.embedding.size() == RAW_DIM && pca.is_fitted()) {
            Eigen::VectorXf raw = Eigen::Map<const Eigen::VectorXf>(syn.embedding.data(), RAW_DIM);
            Eigen::VectorXf proj = pca.project(raw);
            pattern = Eigen::VectorXf::Zero(BRAIN_DIM);
            int copy_dims = std::min((int)proj.size(), BRAIN_DIM);
            pattern.head(copy_dims) = proj.head(copy_dims);
        } else {
            pattern = Eigen::VectorXf::Zero(BRAIN_DIM);
        }

        BrainMemory mem;
        mem.id = syn.id;
        mem.content = syn.content;
        mem.category = syn.category;
        mem.source = "instinct";
        mem.importance = syn.importance;
        mem.created_at = syn.created_at;
        mem.embedding = syn.embedding;
        mem.pattern = pattern;
        mem.activation = GHOST_STRENGTH;
        mem.last_activated = 0.0;
        mem.access_count = 0;
        mem.decay_factor = GHOST_STRENGTH;
        mem.tags = {"ghost"};

        substrate.store(syn.id, pattern, GHOST_STRENGTH);
        graph.add_node(syn.id);

        size_t idx = memories.size();
        memory_index[syn.id] = idx;
        memories.push_back(std::move(mem));
    }

    // Add edges
    for (auto& e : corpus.edges) {
        EdgeType et = edge_type_from_str(e.edge_type);
        graph.add_edge(e.source_id, e.target_id, e.weight, et);
    }

    fprintf(stderr, "[instincts] applied %zu ghosts, %zu edges\n",
            corpus.memories.size(), corpus.edges.size());
}

// ---- Ghost replacement ----

static float cosine_sim_vec(const Eigen::VectorXf& a, const Eigen::VectorXf& b) {
    if (a.size() != b.size() || a.size() == 0) return 0.0f;
    float dot = a.dot(b);
    float na = a.norm();
    float nb = b.norm();
    if (na < 1e-10f || nb < 1e-10f) return 0.0f;
    return dot / (na * nb);
}

size_t check_ghost_replacement(
    const Eigen::VectorXf& new_pattern,
    std::vector<BrainMemory>& memories,
    std::unordered_map<int64_t, size_t>& memory_index,
    HopfieldSubstrate& substrate,
    ConnectionGraph& graph
) {
    std::vector<int64_t> to_remove;

    for (auto& mem : memories) {
        if (mem.id >= 0) continue;
        if (mem.pattern.size() == 0) continue;
        float sim = cosine_sim_vec(new_pattern, mem.pattern);
        if (sim > GHOST_REPLACE_SIM) {
            to_remove.push_back(mem.id);
        }
    }

    size_t count = to_remove.size();

    for (int64_t ghost_id : to_remove) {
        substrate.remove(ghost_id);

        // Remove from graph
        graph.remove_node(ghost_id);

        // Swap-remove from memories
        auto it = memory_index.find(ghost_id);
        if (it != memory_index.end()) {
            size_t idx = it->second;
            size_t last = memories.size() - 1;
            if (idx < last) {
                memories[idx] = std::move(memories[last]);
                memory_index[memories[idx].id] = idx;
            }
            memories.pop_back();
            memory_index.erase(ghost_id);
        }
    }

    return count;
}

} // namespace brain
