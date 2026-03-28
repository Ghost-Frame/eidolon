#include "brain/absorb.hpp"
#include "brain/interference.hpp"
#include <algorithm>
#include <vector>
#include <utility>

namespace brain {

float cosine_sim(const Eigen::VectorXf& a, const Eigen::VectorXf& b) {
    return a.dot(b); // assumes L2 normalized
}

void absorb_memory(BrainMemory& mem,
                   const std::vector<BrainMemory*>& all_memories,
                   PcaTransform& pca,
                   HopfieldSubstrate& substrate,
                   ConnectionGraph& graph)
{
    // Step 1: PCA project if we have a raw embedding
    if (!mem.embedding.empty() && pca.is_fitted()) {
        Eigen::VectorXf raw = Eigen::Map<const Eigen::VectorXf>(
            mem.embedding.data(), static_cast<int>(mem.embedding.size()));
        Eigen::VectorXf proj = pca.project(raw);
        // Zero-pad to BRAIN_DIM so substrate always gets consistent dimensions
        mem.pattern = Eigen::VectorXf::Zero(BRAIN_DIM);
        int copy_dims = std::min((int)proj.size(), BRAIN_DIM);
        mem.pattern.head(copy_dims) = proj.head(copy_dims);
    }

    // Step 2: Store in Hopfield substrate
    substrate.store(mem.id, mem.pattern, mem.decay_factor);

    // Step 3: Add node to graph
    graph.add_node(mem.id);

    // Step 4: Cosine-similarity edges
    double me_epoch = parse_datetime_approx(mem.created_at);

    struct ScoredEdge {
        int64_t target_id;
        float sim;
        EdgeType et;
    };
    std::vector<ScoredEdge> candidates;

    for (BrainMemory* other : all_memories) {
        if (other->id == mem.id) continue;
        if (other->pattern.size() != BRAIN_DIM) continue;

        float sim = cosine_sim(mem.pattern, other->pattern);

        if (sim >= CONTRADICTION_SIM_THRESHOLD &&
            mem.category == other->category)
        {
            // High similarity, same category -- potential contradiction
            // Simple heuristic: different content (not exact match)
            if (mem.content != other->content) {
                candidates.push_back({other->id, sim, EdgeType::Contradiction});
                continue;
            }
        }

        if (sim >= ASSOCIATION_THRESHOLD) {
            candidates.push_back({other->id, sim, EdgeType::Association});
        }
    }

    // Sort by sim descending, limit to MAX_EDGES_PER_MEMORY
    std::sort(candidates.begin(), candidates.end(),
              [](const ScoredEdge& a, const ScoredEdge& b) {
                  return a.sim > b.sim;
              });
    if (static_cast<int>(candidates.size()) > MAX_EDGES_PER_MEMORY) {
        candidates.resize(MAX_EDGES_PER_MEMORY);
    }

    for (auto& c : candidates) {
        graph.add_edge(mem.id, c.target_id, c.sim, c.et);
    }

    // Step 5: Temporal edges
    for (BrainMemory* other : all_memories) {
        if (other->id == mem.id) continue;
        double other_epoch = parse_datetime_approx(other->created_at);
        double diff = std::abs(me_epoch - other_epoch);
        if (diff <= TEMPORAL_WINDOW_SECS && diff > 0.0) {
            // Temporal weight: stronger for closer in time
            float weight = static_cast<float>(1.0 - diff / TEMPORAL_WINDOW_SECS) * 0.5f;
            if (weight > 0.05f) {
                graph.add_edge(mem.id, other->id, weight, EdgeType::Temporal);
            }
        }
    }
}

} // namespace brain
