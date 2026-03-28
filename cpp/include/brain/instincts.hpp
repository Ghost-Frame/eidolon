#pragma once

// instincts.hpp -- synthetic pre-training corpus for new Eidolon instances
//
// Generates ~200 structurally realistic ghost memories across 5 categories.
// Ghosts use negative IDs, start at strength 0.3, decay 2x faster than real.
// When a real memory with cosine_sim > 0.85 to a ghost is absorbed, ghost removed.

#include "types.hpp"
#include "substrate.hpp"
#include "graph.hpp"
#include "pca.hpp"

#include <string>
#include <vector>
#include <unordered_map>
#include <cstdint>
#include <optional>

namespace brain {

// Ghost constants
static constexpr float GHOST_STRENGTH     = 0.3f;
static constexpr float GHOST_REPLACE_SIM  = 0.85f;

// ---- Serializable types ----

struct SyntheticMemory {
    int64_t id;
    std::string content;
    std::string category;
    int32_t importance;
    std::string created_at;
    std::vector<float> embedding; // RAW_DIM floats, L2-normalized
};

struct SyntheticEdge {
    int64_t source_id;
    int64_t target_id;
    float weight;
    std::string edge_type; // "association" | "temporal" | "contradiction"
};

struct InstinctsCorpus {
    uint32_t version;
    std::string generated_at;
    std::vector<SyntheticMemory> memories;
    std::vector<SyntheticEdge> edges;
};

// ---- API ----

// Generate the deterministic synthetic corpus (200 memories, 5 categories)
InstinctsCorpus generate_instincts();

// Serialize corpus to compressed binary file (INST magic + gzip JSON)
bool save_instincts(const InstinctsCorpus& corpus, const std::string& path);

// Load and decompress corpus from file; returns empty optional if missing/corrupt
std::optional<InstinctsCorpus> load_instincts(const std::string& path);

// Apply corpus as ghost patterns into the brain substrate
void apply_instincts(
    std::vector<BrainMemory>& memories,
    std::unordered_map<int64_t, size_t>& memory_index,
    HopfieldSubstrate& substrate,
    ConnectionGraph& graph,
    PcaTransform& pca,
    const InstinctsCorpus& corpus
);

// Check if any ghost patterns are superseded by a new real memory pattern.
// Returns the number of ghosts removed.
size_t check_ghost_replacement(
    const Eigen::VectorXf& new_pattern,
    std::vector<BrainMemory>& memories,
    std::unordered_map<int64_t, size_t>& memory_index,
    HopfieldSubstrate& substrate,
    ConnectionGraph& graph
);

} // namespace brain
