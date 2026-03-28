#pragma once

#include "types.hpp"
#include <unordered_map>
#include <unordered_set>
#include <vector>
#include <tuple>
#include <cstdint>

namespace brain {

struct SpreadResult {
    int64_t id;
    float activation;
    int hops;
};

class ConnectionGraph {
public:
    static constexpr float SPREAD_DECAY_PER_HOP = 0.5f;
    static constexpr float MIN_SPREAD_ACTIVATION = 0.005f;
    static constexpr float MIN_EDGE_WEIGHT = 0.01f;

    // Add a node (memory id) to the graph
    void add_node(int64_t id);

    // Add or update a directed edge (also adds reverse for Association/Temporal)
    void add_edge(int64_t src, int64_t tgt, float weight, EdgeType et);

    // Multi-hop activation spread from seed activations.
    // seed: map of id -> initial activation. max_hops: hop limit.
    // Returns all reached nodes with their final activation and hop count.
    std::vector<SpreadResult> spread(
        const std::unordered_map<int64_t, float>& seeds,
        int max_hops = 3) const;

    // Find contradiction pairs among a set of active node ids.
    std::vector<std::pair<int64_t, int64_t>> contradiction_pairs(
        const std::unordered_set<int64_t>& active) const;

    // Hebbian strengthening: boost edge weight between two co-activated nodes.
    void strengthen_edge(int64_t a, int64_t b, float boost = 0.05f);

    // Decay all edge weights by rate. Remove edges below min_weight.
    void decay_edges(float rate = 0.998f);

    bool has_node(int64_t id) const { return nodes_.count(id) > 0; }

    size_t node_count() const { return nodes_.size(); }

    size_t edge_count() const {
        size_t total = 0;
        for (auto& kv : adjacency_) total += kv.second.size();
        return total;
    }

    // Get neighbors of a node (id, weight, EdgeType)
    const std::vector<std::tuple<int64_t, float, EdgeType>>* neighbors(int64_t id) const;

private:
    using Neighbors = std::vector<std::tuple<int64_t, float, EdgeType>>;
    std::unordered_map<int64_t, Neighbors> adjacency_;
    std::unordered_set<int64_t> nodes_;

    void upsert_directed(int64_t src, int64_t tgt, float weight, EdgeType et);
};

} // namespace brain
