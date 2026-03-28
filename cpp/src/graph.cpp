#include "brain/graph.hpp"
#include <algorithm>
#include <queue>
#include <limits>

namespace brain {

void ConnectionGraph::add_node(int64_t id) {
    nodes_.insert(id);
    if (adjacency_.find(id) == adjacency_.end()) {
        adjacency_[id] = {};
    }
}

void ConnectionGraph::upsert_directed(int64_t src, int64_t tgt, float weight, EdgeType et) {
    auto& neighbors = adjacency_[src];
    for (auto& [nid, w, e] : neighbors) {
        if (nid == tgt) {
            // Update weight and type
            w = weight;
            e = et;
            return;
        }
    }
    neighbors.emplace_back(tgt, weight, et);
}

void ConnectionGraph::add_edge(int64_t src, int64_t tgt, float weight, EdgeType et) {
    nodes_.insert(src);
    nodes_.insert(tgt);
    if (adjacency_.find(src) == adjacency_.end()) adjacency_[src] = {};
    if (adjacency_.find(tgt) == adjacency_.end()) adjacency_[tgt] = {};

    upsert_directed(src, tgt, weight, et);
    // Contradiction is directed both ways; Association and Temporal are undirected
    if (et == EdgeType::Association || et == EdgeType::Temporal || et == EdgeType::Contradiction) {
        upsert_directed(tgt, src, weight, et);
    }
}

std::vector<SpreadResult> ConnectionGraph::spread(
    const std::unordered_map<int64_t, float>& seeds,
    int max_hops) const
{
    // Best activation seen for each node
    std::unordered_map<int64_t, float> best_activation;
    std::unordered_map<int64_t, int> best_hops;

    // BFS frontier: (id, activation, hop)
    using Item = std::tuple<int64_t, float, int>;
    std::queue<Item> frontier;

    for (auto& [id, act] : seeds) {
        best_activation[id] = act;
        best_hops[id] = 0;
        frontier.emplace(id, act, 0);
    }

    while (!frontier.empty()) {
        auto [cur_id, cur_act, cur_hop] = frontier.front();
        frontier.pop();

        if (cur_hop >= max_hops) continue;

        auto it = adjacency_.find(cur_id);
        if (it == adjacency_.end()) continue;

        for (auto& [neighbor_id, weight, et] : it->second) {
            // Skip contradiction edges for spreading (they suppress, not activate)
            if (et == EdgeType::Contradiction) continue;

            float spread_act = cur_act * weight * SPREAD_DECAY_PER_HOP;
            if (spread_act < MIN_SPREAD_ACTIVATION) continue;

            auto ba_it = best_activation.find(neighbor_id);
            if (ba_it == best_activation.end() || spread_act > ba_it->second) {
                best_activation[neighbor_id] = spread_act;
                best_hops[neighbor_id] = cur_hop + 1;
                frontier.emplace(neighbor_id, spread_act, cur_hop + 1);
            }
        }
    }

    std::vector<SpreadResult> results;
    results.reserve(best_activation.size());
    for (auto& [id, act] : best_activation) {
        int hops = best_hops.count(id) ? best_hops.at(id) : 0;
        results.push_back({id, act, hops});
    }

    // Sort descending by activation
    std::sort(results.begin(), results.end(),
              [](const SpreadResult& a, const SpreadResult& b) {
                  return a.activation > b.activation;
              });

    return results;
}

std::vector<std::pair<int64_t, int64_t>> ConnectionGraph::contradiction_pairs(
    const std::unordered_set<int64_t>& active) const
{
    std::vector<std::pair<int64_t, int64_t>> pairs;

    for (int64_t id : active) {
        auto it = adjacency_.find(id);
        if (it == adjacency_.end()) continue;

        for (auto& [neighbor_id, weight, et] : it->second) {
            if (et == EdgeType::Contradiction && active.count(neighbor_id)) {
                // Avoid duplicates by ordering pair
                if (id < neighbor_id) {
                    pairs.emplace_back(id, neighbor_id);
                }
            }
        }
    }

    return pairs;
}

void ConnectionGraph::strengthen_edge(int64_t a, int64_t b, float boost) {
    auto strengthen_directed = [&](int64_t src, int64_t tgt) {
        auto it = adjacency_.find(src);
        if (it == adjacency_.end()) return;
        for (auto& [nid, w, e] : it->second) {
            if (nid == tgt) {
                w = std::min(1.0f, w + boost);
                return;
            }
        }
    };
    strengthen_directed(a, b);
    strengthen_directed(b, a);
}

void ConnectionGraph::decay_edges(float rate) {
    for (auto& [src, neighbors] : adjacency_) {
        // Decay and remove weak edges
        neighbors.erase(
            std::remove_if(neighbors.begin(), neighbors.end(),
                [rate](std::tuple<int64_t, float, EdgeType>& e) {
                    std::get<1>(e) *= rate;
                    return std::get<1>(e) < MIN_EDGE_WEIGHT;
                }),
            neighbors.end()
        );
    }
}

const std::vector<std::tuple<int64_t, float, EdgeType>>*
ConnectionGraph::neighbors(int64_t id) const {
    auto it = adjacency_.find(id);
    if (it == adjacency_.end()) return nullptr;
    return &it->second;
}

} // namespace brain
