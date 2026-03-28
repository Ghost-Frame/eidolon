// test_dreaming.cpp -- assert-based tests for the dreaming module
#include "brain/dreaming.hpp"
#include "brain/substrate.hpp"
#include "brain/graph.hpp"
#include "brain/types.hpp"
#include "brain/decay.hpp"

#include <Eigen/Core>
#include <cassert>
#include <cstdio>
#include <cmath>
#include <vector>
#include <unordered_map>
#include <string>

namespace {

static int pass_count = 0;
static int fail_count = 0;

#define RUN_TEST(fn) do { \
    fprintf(stdout, "  [dreaming] %s ... ", #fn); \
    try { fn(); fprintf(stdout, "PASS\n"); ++pass_count; } \
    catch (const std::exception& e) { fprintf(stdout, "FAIL: %s\n", e.what()); ++fail_count; } \
} while(0)

#define ASSERT_TRUE(cond, msg) do { if (!(cond)) throw std::runtime_error(msg); } while(0)
#define ASSERT_EQ(a, b, msg) do { if ((a) != (b)) { \
    char _buf[256]; snprintf(_buf, 256, "%s: expected %d == %d", msg, (int)(a), (int)(b)); \
    throw std::runtime_error(_buf); } } while(0)

// Helper: build unit-ish vector from seed
static Eigen::VectorXf make_vec(int seed, int dim = brain::BRAIN_DIM) {
    Eigen::VectorXf v(dim);
    for (int i = 0; i < dim; ++i) {
        v[i] = std::sin(seed * 100.0f + i * 0.7f);
    }
    float n = v.norm();
    if (n > 1e-8f) v /= n;
    return v;
}

// Helper: build a simple BrainMemory
static brain::BrainMemory make_mem(int64_t id, const Eigen::VectorXf& pattern,
                                    float decay, double last_activated,
                                    const std::string& content = "") {
    brain::BrainMemory m;
    m.id = id;
    m.content = content.empty() ? ("memory " + std::to_string(id)) : content;
    m.category = "test";
    m.source = "test";
    m.importance = 5;
    m.created_at = "2026-01-01T00:00:00Z";
    m.pattern = pattern;
    m.decay_factor = decay;
    m.activation = 0.5f;
    m.last_activated = last_activated;
    m.access_count = 0;
    return m;
}

// Helper: build substrate, graph, and index from memories
static void build_infra(
    const std::vector<brain::BrainMemory>& mems,
    brain::HopfieldSubstrate& substrate,
    brain::ConnectionGraph& graph,
    std::unordered_map<int64_t, size_t>& index
) {
    for (size_t i = 0; i < mems.size(); ++i) {
        substrate.store(mems[i].id, mems[i].pattern, mems[i].decay_factor);
        graph.add_node(mems[i].id);
        index[mems[i].id] = i;
    }
}

// ---- Test: replay boosts recent memories ----

void test_replay() {
    std::vector<brain::BrainMemory> mems;
    for (int i = 1; i <= 5; ++i) {
        mems.push_back(make_mem(i, make_vec(i), 0.7f, static_cast<double>(i) * 100.0));
    }

    brain::HopfieldSubstrate substrate;
    brain::ConnectionGraph graph;
    std::unordered_map<int64_t, size_t> index;
    build_infra(mems, substrate, graph, index);

    std::vector<float> before;
    for (auto& m : mems) before.push_back(m.decay_factor);

    brain::DreamCycleResult result = brain::dream_cycle(substrate, graph, mems, index, 1);

    ASSERT_TRUE(result.replayed > 0, "should have replayed at least one memory");

    int boosted = 0;
    for (size_t i = 0; i < mems.size(); ++i) {
        if (mems[i].decay_factor > before[i]) ++boosted;
    }
    ASSERT_TRUE(boosted > 0, "at least one memory should have its decay_factor boosted");
}

// ---- Test: merge removes nearly-identical patterns ----

void test_merge() {
    Eigen::VectorXf p1 = Eigen::VectorXf::Zero(brain::BRAIN_DIM);
    p1[0] = 1.0f;
    p1.normalize();

    Eigen::VectorXf p2 = p1;
    p2[1] = 0.001f;
    p2.normalize();

    std::string shared = "alpha beta gamma delta epsilon zeta eta theta iota kappa lambda mu";

    auto m1 = make_mem(1, p1, 0.9f, 100.0, shared);
    auto m2 = make_mem(2, p2, 0.6f,  50.0, shared);

    std::vector<brain::BrainMemory> mems = {m1, m2};
    brain::HopfieldSubstrate substrate;
    brain::ConnectionGraph graph;
    std::unordered_map<int64_t, size_t> index;
    build_infra(mems, substrate, graph, index);

    brain::DreamCycleResult result = brain::dream_cycle(substrate, graph, mems, index, 1);

    ASSERT_EQ(result.merged, 1, "merged count");
    ASSERT_EQ((int)mems.size(), 1, "memories remaining after merge");
}

// ---- Test: prune removes dead patterns ----

void test_prune() {
    Eigen::VectorXf pd = make_vec(10);
    Eigen::VectorXf pa = make_vec(20);

    auto dead  = make_mem(1, pd, 0.01f, 0.0);   // decay below threshold
    auto alive = make_mem(2, pa, 0.90f, 100.0);

    std::vector<brain::BrainMemory> mems = {dead, alive};
    brain::HopfieldSubstrate substrate;
    brain::ConnectionGraph graph;
    std::unordered_map<int64_t, size_t> index;
    build_infra(mems, substrate, graph, index);

    brain::DreamCycleResult result = brain::dream_cycle(substrate, graph, mems, index, 1);

    ASSERT_EQ((int)result.pruned_patterns, 1, "pruned_patterns count");
    ASSERT_EQ((int)mems.size(), 1, "memories remaining after prune");
    ASSERT_EQ((int)mems[0].id, 2, "surviving memory id");
}

// ---- Test: discover creates edge between similar unconnected patterns ----

void test_discover() {
    Eigen::VectorXf p1 = Eigen::VectorXf::Zero(brain::BRAIN_DIM);
    p1[0] = 1.0f; p1[1] = 0.8f;
    p1.normalize();

    Eigen::VectorXf p2 = Eigen::VectorXf::Zero(brain::BRAIN_DIM);
    p2[0] = 0.9f; p2[1] = 0.7f;
    p2.normalize();

    auto m1 = make_mem(1, p1, 0.9f, 100.0);
    auto m2 = make_mem(2, p2, 0.9f, 100.0);

    std::vector<brain::BrainMemory> mems = {m1, m2};
    brain::HopfieldSubstrate substrate;
    brain::ConnectionGraph graph;
    std::unordered_map<int64_t, size_t> index;
    build_infra(mems, substrate, graph, index);

    // Verify not connected initially
    const auto* initial_neighbors = graph.neighbors(1);
    bool initially_connected = initial_neighbors && !initial_neighbors->empty();
    ASSERT_TRUE(!initially_connected, "patterns should not be connected initially");

    brain::DreamCycleResult result = brain::dream_cycle(substrate, graph, mems, index, 1);

    ASSERT_TRUE(result.discovered > 0, "should have discovered at least one connection");
    const auto* neighbors = graph.neighbors(1);
    bool connected = false;
    if (neighbors) {
        for (const auto& [tgt, w, et] : *neighbors) {
            if (tgt == 2) { connected = true; break; }
        }
    }
    ASSERT_TRUE(connected, "edge should exist between pattern 1 and 2 after discovery");
}

// ---- Test: full cycle on small substrate does not crash ----

void test_full_cycle() {
    std::vector<brain::BrainMemory> mems;
    for (int i = 1; i <= 5; ++i) {
        float decay = 0.7f + i * 0.05f;
        mems.push_back(make_mem(i, make_vec(i), decay, static_cast<double>(i) * 50.0));
    }

    brain::HopfieldSubstrate substrate;
    brain::ConnectionGraph graph;
    std::unordered_map<int64_t, size_t> index;
    build_infra(mems, substrate, graph, index);

    brain::DreamCycleResult result = brain::dream_cycle(substrate, graph, mems, index, 1);

    ASSERT_TRUE(result.cycle_time_ms < 10000, "cycle_time_ms should be reasonable");
}

// ---- Test: empty substrate does not crash ----

void test_empty_substrate() {
    std::vector<brain::BrainMemory> mems;
    brain::HopfieldSubstrate substrate;
    brain::ConnectionGraph graph;
    std::unordered_map<int64_t, size_t> index;

    brain::DreamCycleResult result = brain::dream_cycle(substrate, graph, mems, index, 1);
    ASSERT_EQ((int)result.replayed, 0, "replayed");
    ASSERT_EQ((int)result.merged, 0, "merged");
    ASSERT_EQ((int)result.pruned_patterns, 0, "pruned_patterns");
}

} // anonymous namespace

void run_dreaming_tests(int& passes, int& fails) {
    fprintf(stdout, "=== Dreaming Tests ===\n");
    pass_count = 0; fail_count = 0;
    RUN_TEST(test_replay);
    RUN_TEST(test_merge);
    RUN_TEST(test_prune);
    RUN_TEST(test_discover);
    RUN_TEST(test_full_cycle);
    RUN_TEST(test_empty_substrate);
    passes += pass_count;
    fails  += fail_count;
}
