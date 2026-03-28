#pragma once
// ============================================================================
// Evolution -- neuro-symbolic graph learning (feature-gated).
// Per-node and per-edge learned weights trained from feedback signals.
//
// Feature gate: build with -DBRAIN_EVOLUTION=ON to enable.
// When disabled, zero code paths are affected.
// ============================================================================

#ifdef BRAIN_EVOLUTION

#include <cstdint>
#include <string>
#include <unordered_map>
#include <utility>
#include <vector>
#include <functional>

// Forward declare sqlite3 to avoid including the full header here
struct sqlite3;

struct PairHash {
    std::size_t operator()(const std::pair<int64_t, int64_t>& p) const noexcept {
        std::size_t h1 = std::hash<int64_t>{}(p.first);
        std::size_t h2 = std::hash<int64_t>{}(p.second);
        return h1 ^ (h2 * 2654435761ULL + 0x9e3779b9ULL + (h1 << 6) + (h1 >> 2));
    }
};

namespace brain {

// ---- FeedbackSignal ----

struct FeedbackSignal {
    std::vector<int64_t> memory_ids;
    std::vector<std::pair<int64_t, int64_t>> edge_pairs;
    bool useful = false;
    double timestamp = 0.0;
};

// ---- EvolutionStats ----

struct EvolutionStats {
    uint32_t generation = 0;
    size_t num_node_weights = 0;
    size_t num_edge_weights = 0;
    float learning_rate = 0.01f;
};

// ---- EvolutionState ----

class EvolutionState {
public:
    std::unordered_map<int64_t, float> node_weights;
    std::unordered_map<std::pair<int64_t, int64_t>, float, PairHash> edge_weights;
    std::vector<FeedbackSignal> feedback_buffer;
    float learning_rate = 0.01f;
    uint32_t generation = 0;

    // All weights default 1.0, learning_rate 0.01
    EvolutionState();

    // Add a feedback signal to the buffer
    void record_feedback(FeedbackSignal signal);

    // Process all buffered feedback signals.
    // Positive: weights += learning_rate, capped at 2.0
    // Negative: weights -= learning_rate, floored at 0.1
    void train_step();

    // Returns learned node weight or 1.0 default
    float get_node_weight(int64_t id) const;

    // Returns learned edge weight or 1.0 default
    float get_edge_weight(int64_t source, int64_t target) const;

    // Persist to brain_meta table as JSON
    // Returns empty string on success, error message on failure
    std::string save_state(sqlite3* db) const;

    // Load from brain_meta table. Returns fresh state if not found.
    static EvolutionState load_state(sqlite3* db);

    // Get stats summary
    EvolutionStats stats() const;
};

} // namespace brain

#endif // BRAIN_EVOLUTION
