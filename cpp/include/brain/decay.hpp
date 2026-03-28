#pragma once

#include "types.hpp"
#include <string>

namespace brain {

static constexpr float BASE_DECAY_RATE        = 0.995f;
static constexpr float IMPORTANCE_PROTECTION  = 0.002f;
static constexpr float DEATH_THRESHOLD        = 0.05f;
static constexpr float RECALL_BOOST           = 0.3f;
static constexpr float EDGE_DECAY_RATE        = 0.998f;

// Compute new decay factor after ticks. Higher importance = slower decay.
float compute_pattern_decay(float current_factor, int32_t importance, int ticks = 1);

// Boost decay factor after a recall event.
float apply_recall_boost(float current_factor);

// Health classification string.
std::string classify_health(float decay_factor);

// True if decay factor is below death threshold.
bool is_dead(float decay_factor);

} // namespace brain
