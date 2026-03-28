// test_decay.cpp -- assert-based tests for decay and interference
#include "brain/decay.hpp"
#include "brain/interference.hpp"
#include <cassert>
#include <cstdio>
#include <cmath>
#include <stdexcept>

namespace {

static int pass_count = 0;
static int fail_count = 0;

#define RUN_TEST(fn) do { \
    fprintf(stdout, "  [decay] %s ... ", #fn); \
    try { fn(); fprintf(stdout, "PASS\n"); ++pass_count; } \
    catch (const std::exception& e) { fprintf(stdout, "FAIL: %s\n", e.what()); ++fail_count; } \
} while(0)

#define ASSERT_TRUE(cond, msg) do { if (!(cond)) throw std::runtime_error(msg); } while(0)
#define ASSERT_NEAR(a, b, tol) do { if (std::abs((a)-(b)) > (tol)) { \
    char _buf[128]; snprintf(_buf, 128, "expected %g near %g (tol %g)", (double)(a), (double)(b), (double)(tol)); \
    throw std::runtime_error(_buf); } } while(0)

void test_decay_reduces() {
    float f = brain::compute_pattern_decay(1.0f, 5, 1);
    ASSERT_TRUE(f < 1.0f, "decay should reduce factor");
    ASSERT_TRUE(f > 0.9f, "decay should not be too drastic per tick");
}

void test_importance_protects() {
    float f_low  = brain::compute_pattern_decay(1.0f, 1, 100);
    float f_high = brain::compute_pattern_decay(1.0f, 9, 100);
    ASSERT_TRUE(f_high > f_low, "high importance should decay slower");
}

void test_recall_boost() {
    float f = 0.5f;
    float boosted = brain::apply_recall_boost(f);
    ASSERT_TRUE(boosted > f, "recall should boost decay factor");
    ASSERT_TRUE(boosted <= 1.0f, "recall boost should not exceed 1.0");
}

void test_eventual_death() {
    float f = 1.0f;
    for (int i = 0; i < 5000; ++i) {
        f = brain::compute_pattern_decay(f, 1, 1);
        if (brain::is_dead(f)) goto done;
    }
    throw std::runtime_error("pattern never died after 5000 ticks");
done:;
}

void test_classify_health() {
    ASSERT_TRUE(brain::classify_health(0.95f) == "strong",  "0.95 is strong");
    ASSERT_TRUE(brain::classify_health(0.75f) == "healthy", "0.75 is healthy");
    ASSERT_TRUE(brain::classify_health(0.50f) == "fading",  "0.50 is fading");
    ASSERT_TRUE(brain::classify_health(0.10f) == "weak",    "0.10 is weak");
    ASSERT_TRUE(brain::classify_health(0.01f) == "dead",    "0.01 is dead");
}

void test_recency_score() {
    // Recent memory: same time as now
    double now = 1700000000.0;
    float rec_now = brain::recency_score(now, now);
    ASSERT_NEAR(rec_now, 1.0f, 0.01f);

    // 30 days ago: score should be 0.5
    float rec_30d = brain::recency_score(now - 30 * 86400, now);
    ASSERT_NEAR(rec_30d, 0.5f, 0.05f);

    // Older memory has lower score
    float rec_old = brain::recency_score(now - 365 * 86400, now);
    ASSERT_TRUE(rec_old < rec_30d, "older memory has lower recency");
}

void test_resolve_newer_wins() {
    double now = 1700000000.0;
    double recent = now - 86400;     // 1 day ago
    double old    = now - 30 * 86400; // 30 days ago

    float act_a = 0.5f, act_b = 0.5f;
    bool a_wins = brain::resolve_interference(act_a, act_b,
                                               1.0f, 1.0f,
                                               5, 5,
                                               recent, old, now);
    ASSERT_TRUE(a_wins, "newer memory (a) should win");
    ASSERT_TRUE(act_a > act_b, "winner activation should be higher");
}

void test_resolve_importance_wins() {
    double now = 1700000000.0;
    double same_time = now - 86400;

    float act_a = 0.5f, act_b = 0.5f;
    bool a_wins = brain::resolve_interference(act_a, act_b,
                                               1.0f, 1.0f,
                                               9, 3, // a has higher importance
                                               same_time, same_time, now);
    ASSERT_TRUE(a_wins, "higher importance memory should win");
}

void test_parse_datetime() {
    double epoch = brain::parse_datetime_approx("2024-01-01T00:00:00");
    ASSERT_TRUE(epoch > 1700000000.0, "2024 should be after Nov 2023 epoch");
    ASSERT_TRUE(epoch < 1800000000.0, "2024 should be before 2027");
}

} // namespace

void run_decay_tests(int& passes, int& fails) {
    fprintf(stdout, "=== Decay + Interference Tests ===\n");
    pass_count = 0; fail_count = 0;
    RUN_TEST(test_decay_reduces);
    RUN_TEST(test_importance_protects);
    RUN_TEST(test_recall_boost);
    RUN_TEST(test_eventual_death);
    RUN_TEST(test_classify_health);
    RUN_TEST(test_recency_score);
    RUN_TEST(test_resolve_newer_wins);
    RUN_TEST(test_resolve_importance_wins);
    RUN_TEST(test_parse_datetime);
    passes += pass_count;
    fails  += fail_count;
}
