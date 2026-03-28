// ============================================================================
// Evolution -- neuro-symbolic graph learning (feature-gated).
// ============================================================================

#ifdef BRAIN_EVOLUTION

#include "brain/evolution.hpp"
#include <sqlite3.h>
#include <nlohmann/json.hpp>
#include <algorithm>
#include <cmath>
#include <sstream>

using json = nlohmann::json;

namespace brain {

// Helper: clamp a float between lo and hi
static inline float clampf(float v, float lo, float hi) {
    return std::max(lo, std::min(hi, v));
}

// ---- EvolutionState ----

EvolutionState::EvolutionState() {}

void EvolutionState::record_feedback(FeedbackSignal signal) {
    feedback_buffer.push_back(std::move(signal));
}

void EvolutionState::train_step() {
    for (const auto& signal : feedback_buffer) {
        float delta = signal.useful ? learning_rate : -learning_rate;

        for (int64_t id : signal.memory_ids) {
            auto it = node_weights.find(id);
            if (it == node_weights.end()) {
                node_weights[id] = clampf(1.0f + delta, 0.1f, 2.0f);
            } else {
                it->second = clampf(it->second + delta, 0.1f, 2.0f);
            }
        }

        for (const auto& pair : signal.edge_pairs) {
            auto it = edge_weights.find(pair);
            if (it == edge_weights.end()) {
                edge_weights[pair] = clampf(1.0f + delta, 0.1f, 2.0f);
            } else {
                it->second = clampf(it->second + delta, 0.1f, 2.0f);
            }
        }
    }

    feedback_buffer.clear();
    ++generation;
}

float EvolutionState::get_node_weight(int64_t id) const {
    auto it = node_weights.find(id);
    return it == node_weights.end() ? 1.0f : it->second;
}

float EvolutionState::get_edge_weight(int64_t source, int64_t target) const {
    auto it = edge_weights.find({source, target});
    return it == edge_weights.end() ? 1.0f : it->second;
}

std::string EvolutionState::save_state(sqlite3* db) const {
    if (!db) return "db is null";

    // Build JSON: keys as strings (node weights as id string, edge weights as "src,tgt")
    json j;
    j["generation"] = generation;
    j["learning_rate"] = learning_rate;

    json nw = json::object();
    for (const auto& [id, w] : node_weights) {
        nw[std::to_string(id)] = w;
    }
    j["node_weights"] = nw;

    json ew = json::object();
    for (const auto& [pair, w] : edge_weights) {
        std::string key = std::to_string(pair.first) + "," + std::to_string(pair.second);
        ew[key] = w;
    }
    j["edge_weights"] = ew;

    std::string json_str = j.dump();
    const void* blob = json_str.data();
    int blob_size = static_cast<int>(json_str.size());

    sqlite3_stmt* stmt = nullptr;
    int rc = sqlite3_prepare_v2(db,
        "INSERT OR REPLACE INTO brain_meta (key, value, updated_at) VALUES ('evolution_state', ?, datetime('now'))",
        -1, &stmt, nullptr);
    if (rc != SQLITE_OK) {
        return std::string("prepare failed: ") + sqlite3_errmsg(db);
    }

    sqlite3_bind_blob(stmt, 1, blob, blob_size, SQLITE_STATIC);
    rc = sqlite3_step(stmt);
    sqlite3_finalize(stmt);

    if (rc != SQLITE_DONE) {
        return std::string("step failed: ") + sqlite3_errmsg(db);
    }

    return "";
}

EvolutionState EvolutionState::load_state(sqlite3* db) {
    EvolutionState state;
    if (!db) return state;

    sqlite3_stmt* stmt = nullptr;
    int rc = sqlite3_prepare_v2(db,
        "SELECT value FROM brain_meta WHERE key = 'evolution_state'",
        -1, &stmt, nullptr);
    if (rc != SQLITE_OK) return state;

    if (sqlite3_step(stmt) != SQLITE_ROW) {
        sqlite3_finalize(stmt);
        return state;
    }

    const void* blob = sqlite3_column_blob(stmt, 0);
    int blob_size = sqlite3_column_bytes(stmt, 0);

    if (!blob || blob_size <= 0) {
        sqlite3_finalize(stmt);
        return state;
    }

    std::string json_str(static_cast<const char*>(blob), blob_size);
    sqlite3_finalize(stmt);

    try {
        json j = json::parse(json_str);

        state.generation = j.value("generation", 0u);
        state.learning_rate = j.value("learning_rate", 0.01f);

        if (j.contains("node_weights") && j["node_weights"].is_object()) {
            for (auto& [key, val] : j["node_weights"].items()) {
                try {
                    int64_t id = std::stoll(key);
                    state.node_weights[id] = val.get<float>();
                } catch (...) {}
            }
        }

        if (j.contains("edge_weights") && j["edge_weights"].is_object()) {
            for (auto& [key, val] : j["edge_weights"].items()) {
                auto comma = key.find(',');
                if (comma == std::string::npos) continue;
                try {
                    int64_t src = std::stoll(key.substr(0, comma));
                    int64_t tgt = std::stoll(key.substr(comma + 1));
                    state.edge_weights[{src, tgt}] = val.get<float>();
                } catch (...) {}
            }
        }
    } catch (...) {
        // Return fresh state on any parse error
        return EvolutionState{};
    }

    return state;
}

EvolutionStats EvolutionState::stats() const {
    return EvolutionStats{
        generation,
        node_weights.size(),
        edge_weights.size(),
        learning_rate
    };
}

} // namespace brain

// ============================================================================
// C++ Evolution Tests (compiled only when BRAIN_EVOLUTION is defined and
// the test binary is requested)
// ============================================================================

#ifdef BRAIN_EVOLUTION_TESTS

#include <cassert>
#include <cstdio>
#include <cmath>

// Minimal in-memory SQLite helper for tests
static sqlite3* open_test_db() {
    sqlite3* db = nullptr;
    sqlite3_open(":memory:", &db);
    sqlite3_exec(db,
        "CREATE TABLE brain_meta (key TEXT PRIMARY KEY, value BLOB, updated_at TEXT);",
        nullptr, nullptr, nullptr);
    return db;
}

static void test_default_weights() {
    brain::EvolutionState state;
    assert(state.get_node_weight(42) == 1.0f);
    assert(state.get_edge_weight(1, 2) == 1.0f);
    assert(state.learning_rate == 0.01f);
    assert(state.generation == 0);
    printf("[PASS] test_default_weights\n");
}

static void test_positive_feedback() {
    brain::EvolutionState state;
    brain::FeedbackSignal sig;
    sig.memory_ids = {1, 2};
    sig.edge_pairs = {{1, 2}};
    sig.useful = true;
    state.record_feedback(sig);
    state.train_step();

    float expected = 1.0f + 0.01f;
    assert(std::fabs(state.get_node_weight(1) - expected) < 1e-5f);
    assert(std::fabs(state.get_node_weight(2) - expected) < 1e-5f);
    assert(std::fabs(state.get_edge_weight(1, 2) - expected) < 1e-5f);
    assert(state.generation == 1);
    printf("[PASS] test_positive_feedback\n");
}

static void test_negative_feedback() {
    brain::EvolutionState state;
    brain::FeedbackSignal sig;
    sig.memory_ids = {5};
    sig.edge_pairs = {{5, 6}};
    sig.useful = false;
    state.record_feedback(sig);
    state.train_step();

    float expected = 1.0f - 0.01f;
    assert(std::fabs(state.get_node_weight(5) - expected) < 1e-5f);
    assert(std::fabs(state.get_edge_weight(5, 6) - expected) < 1e-5f);
    printf("[PASS] test_negative_feedback\n");
}

static void test_weight_bounds() {
    // Ceiling test
    brain::EvolutionState state;
    for (int i = 0; i < 200; ++i) {
        brain::FeedbackSignal sig;
        sig.memory_ids = {99};
        sig.edge_pairs = {{99, 100}};
        sig.useful = true;
        state.record_feedback(sig);
        state.train_step();
    }
    assert(state.get_node_weight(99) <= 2.0f);
    assert(std::fabs(state.get_node_weight(99) - 2.0f) < 1e-3f);
    assert(state.get_edge_weight(99, 100) <= 2.0f);

    // Floor test
    brain::EvolutionState state2;
    for (int i = 0; i < 200; ++i) {
        brain::FeedbackSignal sig;
        sig.memory_ids = {77};
        sig.edge_pairs = {{77, 78}};
        sig.useful = false;
        state2.record_feedback(sig);
        state2.train_step();
    }
    assert(state2.get_node_weight(77) >= 0.1f);
    assert(std::fabs(state2.get_node_weight(77) - 0.1f) < 1e-3f);
    printf("[PASS] test_weight_bounds\n");
}

static void test_save_load_roundtrip() {
    sqlite3* db = open_test_db();

    brain::EvolutionState state;
    state.node_weights[10] = 1.5f;
    state.node_weights[20] = 0.7f;
    state.edge_weights[{10, 20}] = 1.3f;
    state.generation = 7;

    std::string err = state.save_state(db);
    assert(err.empty());

    brain::EvolutionState loaded = brain::EvolutionState::load_state(db);
    assert(loaded.generation == 7);
    assert(std::fabs(loaded.get_node_weight(10) - 1.5f) < 1e-5f);
    assert(std::fabs(loaded.get_node_weight(20) - 0.7f) < 1e-5f);
    assert(std::fabs(loaded.get_edge_weight(10, 20) - 1.3f) < 1e-5f);
    assert(loaded.get_node_weight(999) == 1.0f);

    sqlite3_close(db);
    printf("[PASS] test_save_load_roundtrip\n");
}

int main() {
    test_default_weights();
    test_positive_feedback();
    test_negative_feedback();
    test_weight_bounds();
    test_save_load_roundtrip();
    printf("All evolution tests passed.\n");
    return 0;
}

#endif // BRAIN_EVOLUTION_TESTS
#endif // BRAIN_EVOLUTION
