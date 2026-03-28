#pragma once

#include "types.hpp"
#include "pca.hpp"
#include "substrate.hpp"
#include "graph.hpp"
#include <vector>
#include <unordered_map>

namespace brain {

static constexpr float ASSOCIATION_THRESHOLD     = 0.4f;
static constexpr double TEMPORAL_WINDOW_SECS     = 86400.0;
static constexpr int   MAX_EDGES_PER_MEMORY      = 15;
static constexpr float CONTRADICTION_SIM_THRESHOLD = 0.75f;

// Cosine similarity between two L2-normalized vectors (just dot product).
float cosine_sim(const Eigen::VectorXf& a, const Eigen::VectorXf& b);

// Absorb a new memory into the substrate and graph.
// Updates pattern in mem_out (with PCA projection).
// Adds cosine-similarity edges, temporal edges, contradiction detection.
void absorb_memory(BrainMemory& mem_out,
                   const std::vector<BrainMemory*>& all_memories,
                   PcaTransform& pca,
                   HopfieldSubstrate& substrate,
                   ConnectionGraph& graph);

} // namespace brain
