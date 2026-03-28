// test_e2e.cpp -- end-to-end integration tests (no external db required)
// Uses synthetic data built in memory.

#include "brain/types.hpp"
#include "brain/pca.hpp"
#include "brain/substrate.hpp"
#include "brain/graph.hpp"
#include "brain/absorb.hpp"
#include "brain/decay.hpp"
#include "brain/interference.hpp"

#include <Eigen/Core>
#include <cassert>
#include <cstdio>
#include <cmath>
#include <stdexcept>
#include <vector>
#include <algorithm>

// Forward declarations of other test runners
void run_substrate_tests(int& passes, int& fails);
void run_graph_tests(int& passes, int& fails);
void run_decay_tests(int& passes, int& fails);
void run_dreaming_tests(int& passes, int& fails);
void run_instincts_tests(int& passes, int& fails);

namespace {

static int pass_count = 0;
static int fail_count = 0;

#define RUN_TEST(fn) do { \
    fprintf(stdout, "  [e2e] %s ... ", #fn); \
    try { fn(); fprintf(stdout, "PASS\n"); ++pass_count; } \
    catch (const std::exception& e) { fprintf(stdout, "FAIL: %s\n", e.what()); ++fail_count; } \
} while(0)

#define ASSERT_TRUE(cond, msg) do { if (!(cond)) throw std::runtime_error(msg); } while(0)
#define ASSERT_NEAR(a, b, tol, msg) do { if (std::abs((a)-(b)) > (tol)) { \
    char _buf[256]; snprintf(_buf, 256, "%s: expected %g near %g (tol %g)", msg, (double)(a), (double)(b), (double)(tol)); \
    throw std::runtime_error(_buf); } } while(0)

// Build a deterministic embedding using sin
static std::vector<float> make_embedding(int seed, int dim = brain::RAW_DIM) {
    std::vector<float> emb(dim);
    float sum_sq = 0.0f;
    for (int i = 0; i < dim; ++i) {
        emb[i] = static_cast<float>(std::sin(seed * 13.7 + i * 0.17));
        sum_sq += emb[i] * emb[i];
    }
    float norm = std::sqrt(sum_sq);
    for (int i = 0; i < dim; ++i) emb[i] /= norm;
    return emb;
}

// Build a cluster of similar embeddings (slight perturbations of a base)
static std::vector<float> perturb_embedding(const std::vector<float>& base,
                                             float scale, int variant) {
    std::vector<float> v = base;
    float sum_sq = 0.0f;
    for (int i = 0; i < (int)v.size(); ++i) {
        v[i] += scale * static_cast<float>(std::sin(variant * 100.0 + i * 0.7));
        sum_sq += v[i] * v[i];
    }
    float norm = std::sqrt(sum_sq);
    for (int i = 0; i < (int)v.size(); ++i) v[i] /= norm;
    return v;
}

void test_pca_fit_and_project() {
    // Build a small dataset: 50 samples x RAW_DIM
    int N = 50;
    Eigen::MatrixXf data(N, brain::RAW_DIM);
    for (int i = 0; i < N; ++i) {
        auto emb = make_embedding(i);
        data.row(i) = Eigen::Map<Eigen::VectorXf>(emb.data(), brain::RAW_DIM).transpose();
    }

    brain::PcaTransform pca;
    // Cap to N-1 = 49 components since N=50
    pca.fit(data, brain::BRAIN_DIM);
    ASSERT_TRUE(pca.is_fitted(), "PCA should be fitted");
    ASSERT_TRUE(pca.n_components <= 49, "n_components clamped to N-1");

    auto emb0 = make_embedding(0);
    Eigen::VectorXf raw = Eigen::Map<Eigen::VectorXf>(emb0.data(), brain::RAW_DIM);
    Eigen::VectorXf projected = pca.project(raw);

    ASSERT_TRUE(projected.size() == pca.n_components, "projection has correct dim");
    float norm = projected.norm();
    ASSERT_NEAR(norm, 1.0f, 0.01f, "projected vector should be unit length");
}

void test_pca_preserves_similarity() {
    // Similar inputs should project to similar outputs
    int N = 100;
    Eigen::MatrixXf data(N, brain::RAW_DIM);
    auto base_emb = make_embedding(999);
    for (int i = 0; i < N; ++i) {
        auto emb = (i < 10) ? perturb_embedding(base_emb, 0.1f, i)
                             : make_embedding(i + 1000);
        data.row(i) = Eigen::Map<Eigen::VectorXf>(emb.data(), brain::RAW_DIM).transpose();
    }

    brain::PcaTransform pca;
    pca.fit(data, 50);

    auto emb_a = perturb_embedding(base_emb, 0.05f, 1000);
    auto emb_b = perturb_embedding(base_emb, 0.05f, 1001);
    auto emb_c = make_embedding(5555);

    Eigen::VectorXf pa = pca.project(Eigen::Map<Eigen::VectorXf>(emb_a.data(), brain::RAW_DIM));
    Eigen::VectorXf pb = pca.project(Eigen::Map<Eigen::VectorXf>(emb_b.data(), brain::RAW_DIM));
    Eigen::VectorXf pc = pca.project(Eigen::Map<Eigen::VectorXf>(emb_c.data(), brain::RAW_DIM));

    float sim_ab = pa.dot(pb);
    float sim_ac = pa.dot(pc);

    ASSERT_TRUE(sim_ab > sim_ac, "similar inputs should project closer together");
}

void test_full_pipeline_absorb_query() {
    // Build a small corpus via absorb_memory
    brain::PcaTransform pca;
    brain::HopfieldSubstrate sub;
    brain::ConnectionGraph graph;

    // First, fit PCA on corpus
    int N = 20;
    Eigen::MatrixXf data(N, brain::RAW_DIM);
    std::vector<std::vector<float>> embs;
    for (int i = 0; i < N; ++i) {
        auto emb = make_embedding(i * 7);
        embs.push_back(emb);
        data.row(i) = Eigen::Map<Eigen::VectorXf>(emb.data(), brain::RAW_DIM).transpose();
    }
    pca.fit(data, std::min(N - 1, brain::BRAIN_DIM));

    // Absorb memories
    std::vector<brain::BrainMemory> memories;
    for (int i = 0; i < N; ++i) {
        brain::BrainMemory mem;
        mem.id         = i + 1;
        mem.content    = "memory content " + std::to_string(i);
        mem.category   = "test";
        mem.source     = "test";
        mem.importance = 5;
        mem.created_at = "2024-01-01T00:00:00";
        mem.embedding  = embs[i];
        mem.decay_factor = 1.0f;

        std::vector<brain::BrainMemory*> ptrs;
        for (auto& m : memories) ptrs.push_back(&m);
        brain::absorb_memory(mem, ptrs, pca, sub, graph);
        memories.push_back(std::move(mem));
    }

    ASSERT_TRUE(sub.size() == N, "all memories stored in substrate");
    ASSERT_TRUE(graph.node_count() == (size_t)N, "all nodes in graph");

    // Query with the first memory's embedding (zero-pad to BRAIN_DIM to match stored patterns)
    Eigen::VectorXf query = Eigen::Map<Eigen::VectorXf>(embs[0].data(), brain::RAW_DIM);
    Eigen::VectorXf proj = pca.project(query);
    Eigen::VectorXf query_proj = Eigen::VectorXf::Zero(brain::BRAIN_DIM);
    int copy_d = std::min((int)proj.size(), brain::BRAIN_DIM);
    query_proj.head(copy_d) = proj.head(copy_d);

    auto results = sub.retrieve(query_proj, 5, brain::HopfieldSubstrate::DEFAULT_BETA);
    ASSERT_TRUE(!results.empty(), "query returned results");
    ASSERT_TRUE(results[0].id == 1, "first memory should be top result");
}

void test_decay_removes_dead() {
    brain::HopfieldSubstrate sub;
    brain::ConnectionGraph graph;

    // Store 5 patterns, decay heavily
    for (int i = 1; i <= 5; ++i) {
        Eigen::VectorXf p = Eigen::VectorXf::Zero(brain::BRAIN_DIM);
        p[i-1] = 1.0f;
        sub.store(i, p, 1.0f);
        graph.add_node(i);
    }

    // Decay until one dies
    float f = 1.0f;
    for (int t = 0; t < 3000 && !brain::is_dead(f); ++t) {
        f = brain::compute_pattern_decay(f, 1, 1);
    }
    ASSERT_TRUE(brain::is_dead(f), "pattern should be dead after heavy decay");

    // Remove it
    sub.remove(1);
    ASSERT_TRUE(sub.size() == 4, "4 patterns remain after removal");
}

void test_cosine_sim_accuracy() {
    // Test that cosine_sim returns 1.0 for identical vectors
    Eigen::VectorXf v = Eigen::VectorXf::Zero(brain::BRAIN_DIM);
    v[0] = 1.0f;

    float sim_self = brain::cosine_sim(v, v);
    ASSERT_NEAR(sim_self, 1.0f, 0.001f, "cosine_sim of identical vectors");

    // Orthogonal vectors
    Eigen::VectorXf w = Eigen::VectorXf::Zero(brain::BRAIN_DIM);
    w[1] = 1.0f;
    float sim_orth = brain::cosine_sim(v, w);
    ASSERT_NEAR(sim_orth, 0.0f, 0.001f, "cosine_sim of orthogonal vectors");
}

} // namespace

int main() {
    fprintf(stdout, "\n=== Engram C++ Brain Tests ===\n\n");

    int total_pass = 0, total_fail = 0;

    run_substrate_tests(total_pass, total_fail);
    fprintf(stdout, "\n");

    run_graph_tests(total_pass, total_fail);
    fprintf(stdout, "\n");

    run_decay_tests(total_pass, total_fail);
    fprintf(stdout, "\n");

    run_dreaming_tests(total_pass, total_fail);
    run_instincts_tests(total_pass, total_fail);
    fprintf(stdout, "\n");

    // E2E tests
    fprintf(stdout, "=== E2E Tests ===\n");
    pass_count = 0; fail_count = 0;
    RUN_TEST(test_pca_fit_and_project);
    RUN_TEST(test_pca_preserves_similarity);
    RUN_TEST(test_full_pipeline_absorb_query);
    RUN_TEST(test_decay_removes_dead);
    RUN_TEST(test_cosine_sim_accuracy);
    total_pass += pass_count;
    total_fail += fail_count;

    fprintf(stdout, "\n=== RESULTS: %d passed, %d failed ===\n\n",
            total_pass, total_fail);

    return (total_fail == 0) ? 0 : 1;
}
