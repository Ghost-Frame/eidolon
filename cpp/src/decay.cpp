#include "brain/decay.hpp"
#include <cmath>
#include <algorithm>

namespace brain {

float compute_pattern_decay(float current_factor, int32_t importance, int ticks) {
    // Higher importance = slightly slower decay via protection offset
    float protection = IMPORTANCE_PROTECTION * static_cast<float>(importance);
    float effective_rate = BASE_DECAY_RATE + protection;
    if (effective_rate > 0.9999f) effective_rate = 0.9999f;

    float result = current_factor * std::pow(effective_rate, static_cast<float>(ticks));
    return std::max(0.0f, result);
}

float apply_recall_boost(float current_factor) {
    return current_factor + RECALL_BOOST * (1.0f - current_factor);
}

std::string classify_health(float decay_factor) {
    if (decay_factor >= 0.8f)  return "strong";
    if (decay_factor >= 0.6f)  return "healthy";
    if (decay_factor >= 0.4f)  return "fading";
    if (decay_factor >= DEATH_THRESHOLD) return "weak";
    return "dead";
}

bool is_dead(float decay_factor) {
    return decay_factor < DEATH_THRESHOLD;
}

} // namespace brain
