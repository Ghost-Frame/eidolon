#pragma once

#include "types.hpp"
#include <string>
#include <cmath>

namespace brain {

static constexpr float SUPPRESSION_FACTOR = 0.1f;
static constexpr float WINNER_BOOST       = 1.2f;
static constexpr float RECENCY_HALF_LIFE_DAYS = 30.0f;

// Parse ISO-8601 datetime string to approximate Unix epoch seconds.
// Handles "YYYY-MM-DDTHH:MM:SS..." format.
double parse_datetime_approx(const std::string& dt);

// Recency score: 2^(-age_days / 30).
float recency_score(double created_epoch, double now_epoch);

// Effective strength: activation * decay_factor * importance_factor * recency.
float effective_strength(float activation, float decay_factor,
                         int32_t importance, double created_epoch,
                         double now_epoch);

// Resolve interference between two competing memories.
// Modifies activations in place. Returns true if a was the winner.
bool resolve_interference(float& activation_a, float& activation_b,
                          float decay_a, float decay_b,
                          int32_t importance_a, int32_t importance_b,
                          double created_a, double created_b,
                          double now_epoch);

} // namespace brain
