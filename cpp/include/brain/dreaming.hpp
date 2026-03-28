#pragma once

#include "brain/types.hpp"
#include "brain/substrate.hpp"
#include "brain/graph.hpp"
#include <unordered_map>
#include <vector>
#include <cstdint>

namespace brain {

// ---- Constants ----

static constexpr float REPLAY_BOOST              = 0.05f;
static constexpr float REPLAY_EDGE_BOOST         = 0.01f;
static constexpr int   REPLAY_TOP_N              = 20;
static constexpr float MERGE_SIMILARITY_THRESHOLD = 0.92f;
static constexpr float MERGE_CONTENT_RATIO       = 0.70f;
static constexpr float PRUNE_DECAY_THRESHOLD     = 0.08f;
static constexpr float PRUNE_EDGE_THRESHOLD      = 0.02f;
static constexpr float DISCOVERY_SIM_THRESHOLD   = 0.35f;
static constexpr int   DISCOVERY_SAMPLE_SIZE     = 50;

// ---- Result struct ----

struct DreamCycleResult {
    size_t replayed       = 0;
    size_t merged         = 0;
    size_t pruned_patterns = 0;
    size_t pruned_edges   = 0;
    size_t discovered     = 0;
    size_t resolved       = 0;
    uint64_t cycle_time_ms = 0;
};

// ---- Main entry point ----

// Run one full dream cycle in place.
// Returns statistics about what was done.
DreamCycleResult dream_cycle(
    HopfieldSubstrate& substrate,
    ConnectionGraph& graph,
    std::vector<BrainMemory>& memories,
    std::unordered_map<int64_t, size_t>& memory_index,
    uint64_t cycle_number
);

} // namespace brain
