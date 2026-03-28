// test_substrate.cpp -- assert-based tests for HopfieldSubstrate
#include "brain/substrate.hpp"
#include "brain/types.hpp"
#include <Eigen/Core>
#include <cassert>
#include <cstdio>
#include <cmath>
#include <vector>

namespace {

// Build a random unit vector seeded by seed
static Eigen::VectorXf random_unit(int seed, int dim = brain::BRAIN_DIM) {
    Eigen::VectorXf v(dim);
    for (int i = 0; i < dim; ++i) {
        float x = static_cast<float>(std::sin(seed * 1000.0 + i * 0.3));
        v[i] = x;
    }
    v.normalize();
    return v;
}

// Add noise to a pattern
static Eigen::VectorXf add_noise(const Eigen::VectorXf& v, float noise_level = 0.3f) {
    Eigen::VectorXf noisy = v;
    for (int i = 0; i < v.size(); ++i) {
        noisy[i] += noise_level * static_cast<float>(std::sin(i * 7.3));
    }
    noisy.normalize();
    return noisy;
}

static int pass_count = 0;
static int fail_count = 0;

#define RUN_TEST(fn) do { \
    fprintf(stdout, "  [substrate] %s ... ", #fn); \
    try { fn(); fprintf(stdout, "PASS\n"); ++pass_count; } \
    catch (const std::exception& e) { fprintf(stdout, "FAIL: %s\n", e.what()); ++fail_count; } \
} while(0)

#define ASSERT_TRUE(cond, msg) do { if (!(cond)) throw std::runtime_error(msg); } while(0)
#define ASSERT_NEAR(a, b, tol) do { if (std::abs((a)-(b)) > (tol)) { \
    char _buf[128]; snprintf(_buf, 128, "expected %g near %g (tol %g)", (double)(a), (double)(b), (double)(tol)); \
    throw std::runtime_error(_buf); } } while(0)

void test_store_and_retrieve() {
    brain::HopfieldSubstrate sub;
    Eigen::VectorXf p1 = random_unit(1);
    sub.store(42, p1, 1.0f);

    auto results = sub.retrieve(p1, 5);
    ASSERT_TRUE(!results.empty(), "results should not be empty");
    ASSERT_TRUE(results[0].id == 42, "top result should be id 42");
    ASSERT_TRUE(results[0].activation > 0.5f, "activation should be > 0.5");
}

void test_multiple_patterns() {
    brain::HopfieldSubstrate sub;
    for (int i = 0; i < 5; ++i) {
        sub.store(i, random_unit(i * 100), 1.0f);
    }
    // Pattern 3 should be top hit
    Eigen::VectorXf query = random_unit(300);
    auto results = sub.retrieve(query, 5);
    ASSERT_TRUE(!results.empty(), "results not empty");
    ASSERT_TRUE(results[0].id == 3, "pattern 3 should be top hit");
}

void test_strength_affects_ranking() {
    brain::HopfieldSubstrate sub;
    Eigen::VectorXf p1 = random_unit(1);
    Eigen::VectorXf p2 = random_unit(2);

    // Store p2 with high strength, p1 with low strength
    // Make p1 slightly more similar to query but p2 has 10x strength
    sub.store(1, p1, 0.1f);
    sub.store(2, p2, 2.0f);

    // Query closer to p2
    auto results = sub.retrieve(p2, 5, 8.0f);
    ASSERT_TRUE(!results.empty(), "results not empty");
    ASSERT_TRUE(results[0].id == 2, "strong pattern should rank higher");
}

void test_completion() {
    brain::HopfieldSubstrate sub;
    Eigen::VectorXf original = random_unit(42);
    sub.store(1, original, 1.0f);

    // Add several other patterns for context
    for (int i = 2; i <= 5; ++i) {
        sub.store(i, random_unit(i * 200), 1.0f);
    }

    Eigen::VectorXf noisy = add_noise(original, 0.5f);
    Eigen::VectorXf completed = sub.complete(noisy, 5);

    float sim_before = original.dot(noisy);
    float sim_after  = original.dot(completed);
    ASSERT_TRUE(sim_after >= sim_before - 0.05f,
                "completion should not degrade similarity");
}

void test_store_update_no_dup() {
    brain::HopfieldSubstrate sub;
    Eigen::VectorXf p1 = random_unit(1);
    Eigen::VectorXf p2 = random_unit(2);

    sub.store(99, p1, 1.0f);
    sub.store(99, p2, 1.0f); // Update -- not a new pattern

    ASSERT_TRUE(sub.size() == 1, "should be 1 pattern after update");
    auto results = sub.retrieve(p2, 5);
    ASSERT_TRUE(!results.empty() && results[0].id == 99, "updated pattern stored");
}

void test_remove() {
    brain::HopfieldSubstrate sub;
    for (int i = 0; i < 4; ++i) {
        sub.store(i, random_unit(i * 50), 1.0f);
    }
    ASSERT_TRUE(sub.size() == 4, "4 patterns before remove");
    sub.remove(2);
    ASSERT_TRUE(sub.size() == 3, "3 patterns after remove");
    ASSERT_TRUE(!sub.has(2), "removed pattern not found");
    ASSERT_TRUE(sub.has(0) && sub.has(1) && sub.has(3), "other patterns intact");
}

} // namespace

void run_substrate_tests(int& passes, int& fails) {
    fprintf(stdout, "=== Substrate Tests ===\n");
    pass_count = 0; fail_count = 0;
    RUN_TEST(test_store_and_retrieve);
    RUN_TEST(test_multiple_patterns);
    RUN_TEST(test_strength_affects_ranking);
    RUN_TEST(test_completion);
    RUN_TEST(test_store_update_no_dup);
    RUN_TEST(test_remove);
    passes += pass_count;
    fails  += fail_count;
}
