// test_graph.cpp -- assert-based tests for ConnectionGraph
#include "brain/graph.hpp"
#include "brain/types.hpp"
#include <cassert>
#include <cstdio>
#include <cmath>
#include <stdexcept>
#include <unordered_set>

namespace {

static int pass_count = 0;
static int fail_count = 0;

#define RUN_TEST(fn) do { \
    fprintf(stdout, "  [graph] %s ... ", #fn); \
    try { fn(); fprintf(stdout, "PASS\n"); ++pass_count; } \
    catch (const std::exception& e) { fprintf(stdout, "FAIL: %s\n", e.what()); ++fail_count; } \
} while(0)

#define ASSERT_TRUE(cond, msg) do { if (!(cond)) throw std::runtime_error(msg); } while(0)

void test_spreading_basic() {
    brain::ConnectionGraph g;
    // A -> B with weight 1.0, B -> C with weight 1.0
    g.add_edge(1, 2, 1.0f, brain::EdgeType::Association);
    g.add_edge(2, 3, 1.0f, brain::EdgeType::Association);

    std::unordered_map<int64_t, float> seeds{{1, 1.0f}};
    auto results = g.spread(seeds, 3);

    // Node 1 should be there (seed), node 2 via 1 hop, node 3 via 2 hops
    bool found2 = false, found3 = false;
    for (auto& r : results) {
        if (r.id == 2) { found2 = true; ASSERT_TRUE(r.hops == 1, "node 2 is 1 hop"); }
        if (r.id == 3) { found3 = true; ASSERT_TRUE(r.hops == 2, "node 3 is 2 hops"); }
    }
    ASSERT_TRUE(found2, "node 2 should be reachable");
    ASSERT_TRUE(found3, "node 3 should be reachable via 2 hops");
}

void test_multi_hop_decay() {
    brain::ConnectionGraph g;
    g.add_edge(1, 2, 1.0f, brain::EdgeType::Association);
    g.add_edge(2, 3, 1.0f, brain::EdgeType::Association);
    g.add_edge(3, 4, 1.0f, brain::EdgeType::Association);

    std::unordered_map<int64_t, float> seeds{{1, 1.0f}};
    auto results = g.spread(seeds, 4);

    float act2 = 0.0f, act3 = 0.0f, act4 = 0.0f;
    for (auto& r : results) {
        if (r.id == 2) act2 = r.activation;
        if (r.id == 3) act3 = r.activation;
        if (r.id == 4) act4 = r.activation;
    }
    // Each hop decays by SPREAD_DECAY_PER_HOP (0.5)
    ASSERT_TRUE(act2 > act3, "activation decays with hops");
    ASSERT_TRUE(act3 > act4, "activation decays further");
    ASSERT_TRUE(act4 > brain::ConnectionGraph::MIN_SPREAD_ACTIVATION,
                "node 4 should still be above threshold");
}

void test_strengthen_and_decay() {
    brain::ConnectionGraph g;
    g.add_edge(1, 2, 0.5f, brain::EdgeType::Association);

    g.strengthen_edge(1, 2, 0.1f);
    // Weight should be 0.6
    auto* nbrs = g.neighbors(1);
    ASSERT_TRUE(nbrs != nullptr, "neighbors not null");
    float w = 0.0f;
    for (auto& [nid, weight, et] : *nbrs) {
        if (nid == 2) w = weight;
    }
    ASSERT_TRUE(std::abs(w - 0.6f) < 0.01f, "weight should be ~0.6 after strengthen");

    // Decay heavily
    for (int i = 0; i < 500; ++i) {
        g.decay_edges(0.990f);
    }
    // After 500 decays at 0.99, weight starts at 0.6: 0.6 * 0.99^500 ~ 0.0037, below MIN_EDGE_WEIGHT
    nbrs = g.neighbors(1);
    bool still_exists = false;
    if (nbrs) {
        for (auto& [nid, weight, et] : *nbrs) {
            if (nid == 2) still_exists = true;
        }
    }
    ASSERT_TRUE(!still_exists, "edge should be removed after extensive decay");
}

void test_contradiction_pairs() {
    brain::ConnectionGraph g;
    g.add_edge(1, 2, 0.8f, brain::EdgeType::Contradiction);
    g.add_edge(3, 4, 0.9f, brain::EdgeType::Association);

    std::unordered_set<int64_t> active{1, 2, 3, 4};
    auto pairs = g.contradiction_pairs(active);

    ASSERT_TRUE(pairs.size() == 1, "one contradiction pair");
    ASSERT_TRUE((pairs[0].first == 1 && pairs[0].second == 2) ||
                (pairs[0].first == 2 && pairs[0].second == 1),
                "contradiction between 1 and 2");
}

void test_no_spread_through_contradiction() {
    brain::ConnectionGraph g;
    // Contradiction edge should not spread activation
    g.add_edge(1, 2, 1.0f, brain::EdgeType::Contradiction);

    std::unordered_map<int64_t, float> seeds{{1, 1.0f}};
    auto results = g.spread(seeds, 3);

    // Node 2 should NOT be in results (contradictions don't spread)
    bool found2 = false;
    for (auto& r : results) {
        if (r.id == 2 && r.hops > 0) found2 = true;
    }
    ASSERT_TRUE(!found2, "contradiction edges should not spread activation");
}

} // namespace

void run_graph_tests(int& passes, int& fails) {
    fprintf(stdout, "=== Graph Tests ===\n");
    pass_count = 0; fail_count = 0;
    RUN_TEST(test_spreading_basic);
    RUN_TEST(test_multi_hop_decay);
    RUN_TEST(test_strengthen_and_decay);
    RUN_TEST(test_contradiction_pairs);
    RUN_TEST(test_no_spread_through_contradiction);
    passes += pass_count;
    fails  += fail_count;
}
