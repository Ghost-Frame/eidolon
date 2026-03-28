// test_instincts.cpp -- assert-based tests for the instincts module

#include "brain/instincts.hpp"
#include "brain/substrate.hpp"
#include "brain/graph.hpp"
#include "brain/types.hpp"
#include "brain/pca.hpp"

#include <Eigen/Core>
#include <cassert>
#include <cstdio>
#include <cmath>
#include <cstring>
#include <vector>
#include <unordered_map>
#include <unordered_set>
#include <string>
#include <stdexcept>
#include <sys/stat.h>
#include <filesystem>

namespace {

static int pass_count = 0;
static int fail_count = 0;

#define RUN_TEST(fn) do { \
    fprintf(stdout, "  [instincts] %s ... ", #fn); \
    try { fn(); fprintf(stdout, "PASS\n"); ++pass_count; } \
    catch (const std::exception& e) { fprintf(stdout, "FAIL: %s\n", e.what()); ++fail_count; } \
} while(0)

#define ASSERT_TRUE(cond, msg) do { if (!(cond)) throw std::runtime_error(msg); } while(0)
#define ASSERT_EQ(a, b, msg) do { if ((a) != (b)) { \
    char _buf[256]; snprintf(_buf, 256, "%s: expected %d == %d", msg, (int)(a), (int)(b)); \
    throw std::runtime_error(_buf); } } while(0)
#define ASSERT_GT(a, b, msg) do { if (!((a) > (b))) { \
    char _buf[256]; snprintf(_buf, 256, "%s: expected %d > %d", msg, (int)(a), (int)(b)); \
    throw std::runtime_error(_buf); } } while(0)

// Helper: build a test PCA from 20 synthetic RAW_DIM-dim vectors
static brain::PcaTransform build_test_pca() {
    int n = 20;
    Eigen::MatrixXf data(n, brain::RAW_DIM);
    for (int i = 0; i < n; ++i) {
        for (int j = 0; j < brain::RAW_DIM; ++j) {
            data(i, j) = std::sin((i * 0.3f + j * 0.07f) * (float)M_PI);
        }
    }
    brain::PcaTransform pca;
    pca.fit(data, brain::BRAIN_DIM);
    return pca;
}

// ---- Test: generate corpus ----

void test_generate_corpus() {
    brain::InstinctsCorpus corpus = brain::generate_instincts();

    ASSERT_GT((int)corpus.memories.size(), 100, "expected at least 100 memories");

    // All IDs must be negative
    for (auto& m : corpus.memories) {
        ASSERT_TRUE(m.id < 0, "ghost ID should be negative");
    }

    // Category diversity: at least 4 distinct categories
    std::unordered_set<std::string> cats;
    for (auto& m : corpus.memories) cats.insert(m.category);
    ASSERT_GT((int)cats.size(), 3, "expected at least 4 distinct categories");

    // Edges must exist
    ASSERT_GT((int)corpus.edges.size(), 0, "expected edges in corpus");

    // All embeddings must be RAW_DIM
    for (auto& m : corpus.memories) {
        ASSERT_EQ((int)m.embedding.size(), brain::RAW_DIM, "embedding should be RAW_DIM");
    }

    // Determinism: generate again and compare
    brain::InstinctsCorpus corpus2 = brain::generate_instincts();
    ASSERT_EQ((int)corpus.memories.size(), (int)corpus2.memories.size(), "determinism: same memory count");

    for (size_t i = 0; i < corpus.memories.size(); ++i) {
        ASSERT_EQ(corpus.memories[i].id, corpus2.memories[i].id, "determinism: same IDs");
        ASSERT_TRUE(corpus.memories[i].content == corpus2.memories[i].content,
                    "determinism: same content");
        float diff = 0.0f;
        for (int j = 0; j < brain::RAW_DIM; ++j) {
            diff += std::abs(corpus.memories[i].embedding[j] - corpus2.memories[i].embedding[j]);
        }
        ASSERT_TRUE(diff < 1e-5f, "determinism: embeddings should be identical");
    }
}

// ---- Test: save/load roundtrip ----

void test_save_load_roundtrip() {
    brain::InstinctsCorpus corpus = brain::generate_instincts();

    // Write to a temp file
    std::string tmp_path = "/tmp/test_instincts_roundtrip.bin";

    bool saved = brain::save_instincts(corpus, tmp_path);
    ASSERT_TRUE(saved, "save_instincts should return true");

    // File should have content
    struct stat st;
    if (stat(tmp_path.c_str(), &st) != 0 || st.st_size < 100) {
        throw std::runtime_error("instincts file missing or too small");
    }

    auto loaded_opt = brain::load_instincts(tmp_path);
    ASSERT_TRUE(loaded_opt.has_value(), "load_instincts should return a value");

    auto& loaded = loaded_opt.value();

    ASSERT_EQ((int)corpus.version, (int)loaded.version, "version should match");
    ASSERT_EQ((int)corpus.memories.size(), (int)loaded.memories.size(), "memory count should match");
    ASSERT_EQ((int)corpus.edges.size(), (int)loaded.edges.size(), "edge count should match");

    ASSERT_TRUE(corpus.memories[0].content == loaded.memories[0].content,
                "first memory content should match after roundtrip");

    // Check embedding roundtrip
    float diff = 0.0f;
    for (int j = 0; j < brain::RAW_DIM; ++j) {
        diff += std::abs(corpus.memories[0].embedding[j] - loaded.memories[0].embedding[j]);
    }
    if (diff > 1e-3f) {
        char buf[128];
        snprintf(buf, sizeof(buf), "embedding roundtrip diff too large: %f", diff);
        throw std::runtime_error(buf);
    }

    std::remove(tmp_path.c_str());
}

// ---- Test: ghost patterns ----

void test_ghost_patterns() {
    brain::InstinctsCorpus corpus = brain::generate_instincts();
    brain::PcaTransform pca = build_test_pca();

    std::vector<brain::BrainMemory> memories;
    std::unordered_map<int64_t, size_t> memory_index;
    brain::HopfieldSubstrate substrate;
    brain::ConnectionGraph graph;

    brain::apply_instincts(memories, memory_index, substrate, graph, pca, corpus);

    // All patterns should have negative IDs
    for (auto& m : memories) {
        ASSERT_TRUE(m.id < 0, "ghost ID should be negative");
    }

    // Ghost strength should be GHOST_STRENGTH
    for (auto& m : memories) {
        float diff = std::abs(m.decay_factor - brain::GHOST_STRENGTH);
        if (diff > 0.01f) {
            char buf[128];
            snprintf(buf, sizeof(buf), "ghost decay_factor should be %.2f, got %.2f for id %lld",
                     brain::GHOST_STRENGTH, m.decay_factor, (long long)m.id);
            throw std::runtime_error(buf);
        }
    }

    // Substrate should have patterns
    ASSERT_GT((int)substrate.size(), 0, "substrate should have ghost patterns");

    // memory_index consistency
    ASSERT_EQ((int)memories.size(), (int)memory_index.size(), "memories and index same size");
    for (auto& [id, idx] : memory_index) {
        ASSERT_EQ(memories[idx].id, id, "memory_index should be consistent");
    }
}

// ---- Test: ghost replacement ----

void test_ghost_replacement() {
    brain::InstinctsCorpus corpus = brain::generate_instincts();
    brain::PcaTransform pca = build_test_pca();

    std::vector<brain::BrainMemory> memories;
    std::unordered_map<int64_t, size_t> memory_index;
    brain::HopfieldSubstrate substrate;
    brain::ConnectionGraph graph;

    brain::apply_instincts(memories, memory_index, substrate, graph, pca, corpus);

    int ghost_count_before = 0;
    for (auto& m : memories) if (m.id < 0) ++ghost_count_before;
    ASSERT_GT(ghost_count_before, 0, "should have ghosts before replacement");

    // Get the first ghost's pattern (cosine_sim = 1.0 with itself)
    const brain::BrainMemory* first_ghost = nullptr;
    for (auto& m : memories) {
        if (m.id < 0) { first_ghost = &m; break; }
    }
    ASSERT_TRUE(first_ghost != nullptr, "should find a ghost");

    int64_t first_ghost_id = first_ghost->id;
    Eigen::VectorXf near_pattern = first_ghost->pattern;

    size_t removed = brain::check_ghost_replacement(
        near_pattern, memories, memory_index, substrate, graph);

    ASSERT_GT((int)removed, 0, "at least one ghost should be removed");

    int ghost_count_after = 0;
    for (auto& m : memories) if (m.id < 0) ++ghost_count_after;
    ASSERT_TRUE(ghost_count_after < ghost_count_before, "ghost count should decrease");

    // Replaced ghost should not be in memory_index
    ASSERT_TRUE(memory_index.find(first_ghost_id) == memory_index.end(),
                "replaced ghost should not be in memory_index");

    // memory_index consistency
    ASSERT_EQ((int)memories.size(), (int)memory_index.size(), "memories and index same size after removal");
    for (auto& [id, idx] : memory_index) {
        ASSERT_EQ(memories[idx].id, id, "memory_index should be consistent after removal");
    }
}

} // anonymous namespace

void run_instincts_tests(int& passes, int& fails) {
    fprintf(stdout, "=== Instincts Tests ===\n");
    pass_count = 0; fail_count = 0;
    RUN_TEST(test_generate_corpus);
    RUN_TEST(test_save_load_roundtrip);
    RUN_TEST(test_ghost_patterns);
    RUN_TEST(test_ghost_replacement);
    passes += pass_count;
    fails  += fail_count;
}
