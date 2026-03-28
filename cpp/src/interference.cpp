#include "brain/interference.hpp"
#include <cmath>
#include <cstring>
#include <algorithm>

namespace brain {

double parse_datetime_approx(const std::string& dt) {
    // Minimal ISO-8601 parser: "YYYY-MM-DDTHH:MM:SS"
    if (dt.size() < 10) return 0.0;

    int year = 0, month = 1, day = 1, hour = 0, minute = 0, sec = 0;
    // Parse year
    if (dt.size() >= 4)  year   = std::stoi(dt.substr(0, 4));
    if (dt.size() >= 7)  month  = std::stoi(dt.substr(5, 2));
    if (dt.size() >= 10) day    = std::stoi(dt.substr(8, 2));
    if (dt.size() >= 13) hour   = std::stoi(dt.substr(11, 2));
    if (dt.size() >= 16) minute = std::stoi(dt.substr(14, 2));
    if (dt.size() >= 19) sec    = std::stoi(dt.substr(17, 2));

    // Days since Unix epoch (1970-01-01) using the Gregorian calendar formula.
    // Zeller-ish approach: shift so March = month 1.
    if (month <= 2) { month += 12; year -= 1; }
    int y = year, m = month, d = day;
    long jdn = 365L * y + y/4 - y/100 + y/400
             + (153 * m + 8) / 5
             + d - 719469;

    return static_cast<double>(jdn) * 86400.0
         + hour * 3600 + minute * 60 + sec;
}

float recency_score(double created_epoch, double now_epoch) {
    double age_secs = now_epoch - created_epoch;
    if (age_secs < 0.0) age_secs = 0.0;
    double age_days = age_secs / 86400.0;
    return static_cast<float>(std::pow(2.0, -age_days / RECENCY_HALF_LIFE_DAYS));
}

float effective_strength(float activation, float decay_factor,
                          int32_t importance, double created_epoch,
                          double now_epoch) {
    float importance_factor = static_cast<float>(importance) / 10.0f;
    float recency = recency_score(created_epoch, now_epoch);
    return activation * decay_factor * importance_factor * recency;
}

bool resolve_interference(float& activation_a, float& activation_b,
                           float decay_a, float decay_b,
                           int32_t importance_a, int32_t importance_b,
                           double created_a, double created_b,
                           double now_epoch) {
    float eff_a = effective_strength(activation_a, decay_a, importance_a, created_a, now_epoch);
    float eff_b = effective_strength(activation_b, decay_b, importance_b, created_b, now_epoch);

    bool a_wins = eff_a >= eff_b;
    if (a_wins) {
        activation_b *= SUPPRESSION_FACTOR;
        activation_a = std::min(1.0f, activation_a * WINNER_BOOST);
    } else {
        activation_a *= SUPPRESSION_FACTOR;
        activation_b = std::min(1.0f, activation_b * WINNER_BOOST);
    }
    return a_wins;
}

} // namespace brain
