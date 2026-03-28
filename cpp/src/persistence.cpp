#include "brain/persistence.hpp"
#include <sqlite3.h>
#include <cstring>
#include <Eigen/Core>

namespace brain {

sqlite3* db_open(const std::string& path, std::string& errmsg) {
    sqlite3* db = nullptr;
    int rc = sqlite3_open_v2(path.c_str(), &db,
                              SQLITE_OPEN_READWRITE | SQLITE_OPEN_CREATE, nullptr);
    if (rc != SQLITE_OK) {
        errmsg = sqlite3_errmsg(db);
        sqlite3_close(db);
        return nullptr;
    }
    sqlite3_exec(db, "PRAGMA journal_mode=WAL;", nullptr, nullptr, nullptr);
    sqlite3_exec(db, "PRAGMA synchronous=NORMAL;", nullptr, nullptr, nullptr);
    return db;
}

void db_close(sqlite3* db) {
    if (db) sqlite3_close(db);
}

std::vector<BrainMemory> load_memories(sqlite3* db, std::string& errmsg) {
    std::vector<BrainMemory> memories;
    // Check table exists first; return empty silently if not
    {
        const char* chk = "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='brain_memories'";
        sqlite3_stmt* chk_stmt = nullptr;
        sqlite3_prepare_v2(db, chk, -1, &chk_stmt, nullptr);
        bool tbl_exists = false;
        if (sqlite3_step(chk_stmt) == SQLITE_ROW) {
            tbl_exists = sqlite3_column_int(chk_stmt, 0) > 0;
        }
        sqlite3_finalize(chk_stmt);
        if (!tbl_exists) return memories;
    }
    const char* sql =
        "SELECT id, content, category, source, importance, created_at, "
        "embedding, tags FROM brain_memories";
    sqlite3_stmt* stmt = nullptr;
    int rc = sqlite3_prepare_v2(db, sql, -1, &stmt, nullptr);
    if (rc != SQLITE_OK) { errmsg = sqlite3_errmsg(db); return memories; }
    while (sqlite3_step(stmt) == SQLITE_ROW) {
        BrainMemory mem;
        mem.id = sqlite3_column_int64(stmt, 0);
        auto col_str = [&](int col) -> std::string {
            const char* t = reinterpret_cast<const char*>(sqlite3_column_text(stmt, col));
            return t ? t : "";
        };
        mem.content    = col_str(1);
        mem.category   = col_str(2);
        mem.source     = col_str(3);
        mem.importance = sqlite3_column_int(stmt, 4);
        mem.created_at = col_str(5);
        int blob_bytes = sqlite3_column_bytes(stmt, 6);
        const void* blob = sqlite3_column_blob(stmt, 6);
        // Validate that the blob is exactly RAW_DIM floats to prevent
        // buffer overread/overwrite from a corrupt or truncated database row.
        if (blob && blob_bytes == RAW_DIM * static_cast<int>(sizeof(float))) {
            mem.embedding.resize(RAW_DIM);
            std::memcpy(mem.embedding.data(), blob, blob_bytes);
        } else if (blob && blob_bytes > 0) {
            // Unexpected size -- skip embedding rather than risk memory corruption
            errmsg = "warning: skipped embedding with unexpected blob size";
        }
        mem.activation   = 0.0f;
        mem.decay_factor = 1.0f;
        mem.access_count = 0;
        memories.push_back(std::move(mem));
    }
    sqlite3_finalize(stmt);
    return memories;
}

std::vector<BrainEdge> load_edges(sqlite3* db, std::string& errmsg) {
    std::vector<BrainEdge> edges;
    // Check table exists first
    {
        const char* chk = "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='brain_edges'";
        sqlite3_stmt* chk_stmt = nullptr;
        sqlite3_prepare_v2(db, chk, -1, &chk_stmt, nullptr);
        bool tbl_exists = false;
        if (sqlite3_step(chk_stmt) == SQLITE_ROW) {
            tbl_exists = sqlite3_column_int(chk_stmt, 0) > 0;
        }
        sqlite3_finalize(chk_stmt);
        if (!tbl_exists) return edges;
    }
    const char* sql =
        "SELECT source_id, target_id, weight, edge_type, created_at FROM brain_edges";
    sqlite3_stmt* stmt = nullptr;
    int rc = sqlite3_prepare_v2(db, sql, -1, &stmt, nullptr);
    if (rc != SQLITE_OK) { errmsg = sqlite3_errmsg(db); return edges; }
    while (sqlite3_step(stmt) == SQLITE_ROW) {
        BrainEdge e;
        e.source_id = sqlite3_column_int64(stmt, 0);
        e.target_id = sqlite3_column_int64(stmt, 1);
        e.weight    = static_cast<float>(sqlite3_column_double(stmt, 2));
        const char* et = reinterpret_cast<const char*>(sqlite3_column_text(stmt, 3));
        e.edge_type = et ? edge_type_from_str(std::string(et)) : EdgeType::Association;
        const char* dt = reinterpret_cast<const char*>(sqlite3_column_text(stmt, 4));
        e.created_at = dt ? dt : "";
        edges.push_back(e);
    }
    sqlite3_finalize(stmt);
    return edges;
}

bool save_edge(sqlite3* db, const BrainEdge& e, std::string& errmsg) {
    const char* sql =
        "INSERT OR REPLACE INTO brain_edges"
        "(source_id, target_id, weight, edge_type, created_at) "
        "VALUES (?, ?, ?, ?, ?)";
    sqlite3_stmt* stmt = nullptr;
    if (sqlite3_prepare_v2(db, sql, -1, &stmt, nullptr) != SQLITE_OK) {
        errmsg = sqlite3_errmsg(db); return false;
    }
    sqlite3_bind_int64(stmt, 1, e.source_id);
    sqlite3_bind_int64(stmt, 2, e.target_id);
    sqlite3_bind_double(stmt, 3, static_cast<double>(e.weight));
    sqlite3_bind_text(stmt, 4, edge_type_str(e.edge_type), -1, SQLITE_STATIC);
    sqlite3_bind_text(stmt, 5, e.created_at.c_str(), -1, SQLITE_TRANSIENT);
    bool ok = sqlite3_step(stmt) == SQLITE_DONE;
    if (!ok) errmsg = sqlite3_errmsg(db);
    sqlite3_finalize(stmt);
    return ok;
}

static std::vector<char> matrix_to_bytes(const Eigen::MatrixXf& m) {
    size_t bytes = static_cast<size_t>(m.size()) * sizeof(float);
    std::vector<char> buf(bytes);
    std::memcpy(buf.data(), m.data(), bytes);
    return buf;
}

static std::vector<char> vector_to_bytes(const Eigen::VectorXf& v) {
    size_t bytes = static_cast<size_t>(v.size()) * sizeof(float);
    std::vector<char> buf(bytes);
    std::memcpy(buf.data(), v.data(), bytes);
    return buf;
}

bool save_pca_state(sqlite3* db, const PcaTransform& pca, std::string& errmsg) {
    if (!pca.fitted) { errmsg = "PCA not fitted"; return false; }
    sqlite3_exec(db,
        "CREATE TABLE IF NOT EXISTS brain_meta ("
        "key TEXT PRIMARY KEY, value BLOB, updated_at TEXT)",
        nullptr, nullptr, nullptr);
    auto components_bytes = matrix_to_bytes(pca.components);
    auto mean_bytes       = vector_to_bytes(pca.mean);
    const char* upsert_sql =
        "INSERT OR REPLACE INTO brain_meta(key, value, updated_at) "
        "VALUES(?, ?, datetime('now'))";
    auto upsert = [&](const char* key, const std::vector<char>& data) -> bool {
        sqlite3_stmt* stmt = nullptr;
        if (sqlite3_prepare_v2(db, upsert_sql, -1, &stmt, nullptr) != SQLITE_OK) {
            errmsg = sqlite3_errmsg(db); return false;
        }
        sqlite3_bind_text(stmt, 1, key, -1, SQLITE_STATIC);
        sqlite3_bind_blob(stmt, 2, data.data(), static_cast<int>(data.size()), SQLITE_TRANSIENT);
        bool ok = sqlite3_step(stmt) == SQLITE_DONE;
        if (!ok) errmsg = sqlite3_errmsg(db);
        sqlite3_finalize(stmt);
        return ok;
    };
    if (!upsert("pca_components", components_bytes)) return false;
    if (!upsert("pca_mean", mean_bytes)) return false;
    std::vector<char> nc_buf(sizeof(int));
    std::memcpy(nc_buf.data(), &pca.n_components, sizeof(int));
    return upsert("pca_n_components", nc_buf);
}

bool load_pca_state(sqlite3* db, PcaTransform& pca, std::string& errmsg) {
    const char* check_sql =
        "SELECT count(*) FROM sqlite_master WHERE type='table' AND name='brain_meta'";
    sqlite3_stmt* check = nullptr;
    sqlite3_prepare_v2(db, check_sql, -1, &check, nullptr);
    bool exists = (sqlite3_step(check) == SQLITE_ROW && sqlite3_column_int(check, 0) > 0);
    sqlite3_finalize(check);
    if (!exists) return false;
    auto load_blob = [&](const char* key, std::vector<char>& out) -> bool {
        const char* sql = "SELECT value FROM brain_meta WHERE key = ?";
        sqlite3_stmt* stmt = nullptr;
        if (sqlite3_prepare_v2(db, sql, -1, &stmt, nullptr) != SQLITE_OK) return false;
        sqlite3_bind_text(stmt, 1, key, -1, SQLITE_STATIC);
        bool found = false;
        if (sqlite3_step(stmt) == SQLITE_ROW) {
            int bytes = sqlite3_column_bytes(stmt, 0);
            const void* blob = sqlite3_column_blob(stmt, 0);
            if (blob && bytes > 0) {
                out.resize(bytes);
                std::memcpy(out.data(), blob, bytes);
                found = true;
            }
        }
        sqlite3_finalize(stmt);
        return found;
    };
    std::vector<char> nc_buf, components_buf, mean_buf;
    if (!load_blob("pca_n_components", nc_buf)) return false;
    if (!load_blob("pca_components", components_buf)) return false;
    if (!load_blob("pca_mean", mean_buf)) return false;
    int n_comp;
    std::memcpy(&n_comp, nc_buf.data(), sizeof(int));
    // Validate n_comp is within the expected range [1, BRAIN_DIM]
    // to prevent resize/memcpy with an adversarially large or zero value.
    if (n_comp < 1 || n_comp > BRAIN_DIM) {
        errmsg = "PCA n_components out of valid range [1, BRAIN_DIM]";
        return false;
    }
    int raw_dim = static_cast<int>(mean_buf.size() / sizeof(float));
    pca.mean.resize(raw_dim);
    std::memcpy(pca.mean.data(), mean_buf.data(), mean_buf.size());
    pca.components.resize(n_comp, raw_dim);
    std::memcpy(pca.components.data(), components_buf.data(), components_buf.size());
    pca.n_components = n_comp;
    pca.fitted = true;
    return true;
}

} // namespace brain
