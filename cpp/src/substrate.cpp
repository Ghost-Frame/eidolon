#include "brain/substrate.hpp"
#include <Eigen/Dense>
#include <algorithm>
#include <cmath>
#include <stdexcept>

namespace brain {

HopfieldSubstrate::HopfieldSubstrate() {
    patterns_ = Eigen::MatrixXf(0, BRAIN_DIM);
}

void HopfieldSubstrate::store(int64_t id, const Eigen::VectorXf& pattern, float strength) {
    auto it = id_to_index_.find(id);
    if (it != id_to_index_.end()) {
        // Update existing
        int idx = it->second;
        patterns_.row(idx) = pattern.transpose();
        strengths_[idx] = strength;
        return;
    }

    // Append new row
    int new_idx = static_cast<int>(pattern_ids_.size());
    patterns_.conservativeResize(new_idx + 1, BRAIN_DIM);
    patterns_.row(new_idx) = pattern.transpose();
    strengths_.push_back(strength);
    pattern_ids_.push_back(id);
    id_to_index_[id] = new_idx;
}

Eigen::VectorXf HopfieldSubstrate::softmax(const Eigen::VectorXf& logits) {
    // Numerically stable softmax
    float max_val = logits.maxCoeff();
    Eigen::VectorXf exps = (logits.array() - max_val).exp();
    float sum = exps.sum();
    if (sum < 1e-12f) {
        return Eigen::VectorXf::Constant(logits.size(), 1.0f / logits.size());
    }
    return exps / sum;
}

std::vector<RetrievedPattern> HopfieldSubstrate::retrieve(
    const Eigen::VectorXf& query, int top_k, float beta) const
{
    int N = static_cast<int>(pattern_ids_.size());
    if (N == 0) return {};

    // similarities = patterns_ * query (dot products since rows and query are L2 normalized)
    Eigen::VectorXf similarities = patterns_ * query;

    // strengths as vector
    Eigen::VectorXf str = Eigen::Map<const Eigen::VectorXf>(strengths_.data(), N);

    // logits = beta * similarity * strength
    Eigen::VectorXf logits = beta * similarities.cwiseProduct(str);

    Eigen::VectorXf activations = softmax(logits);

    // Collect all above threshold
    std::vector<RetrievedPattern> results;
    results.reserve(N);
    for (int i = 0; i < N; ++i) {
        if (activations[i] >= ACTIVATION_THRESHOLD) {
            results.push_back({pattern_ids_[i], activations[i], i});
        }
    }

    // Sort descending by activation
    std::sort(results.begin(), results.end(),
              [](const RetrievedPattern& a, const RetrievedPattern& b) {
                  return a.activation > b.activation;
              });

    if (static_cast<int>(results.size()) > top_k) {
        results.resize(top_k);
    }
    return results;
}

Eigen::VectorXf HopfieldSubstrate::complete(
    const Eigen::VectorXf& noisy_query, int steps, float beta) const
{
    int N = static_cast<int>(pattern_ids_.size());
    if (N == 0) return noisy_query;

    Eigen::VectorXf current = noisy_query;
    // Normalize starting point
    float norm = current.norm();
    if (norm > 1e-8f) current /= norm;

    Eigen::VectorXf str = Eigen::Map<const Eigen::VectorXf>(strengths_.data(), N);

    for (int step = 0; step < steps; ++step) {
        Eigen::VectorXf sims = patterns_ * current;
        Eigen::VectorXf logits = beta * sims.cwiseProduct(str);
        Eigen::VectorXf weights = softmax(logits);

        // Weighted sum of stored patterns
        Eigen::VectorXf next = Eigen::VectorXf::Zero(BRAIN_DIM);
        for (int i = 0; i < N; ++i) {
            next += weights[i] * patterns_.row(i).transpose();
        }

        // L2 normalize
        norm = next.norm();
        if (norm > 1e-8f) {
            next /= norm;
        } else {
            break;
        }
        current = next;
    }
    return current;
}

void HopfieldSubstrate::remove(int64_t id) {
    auto it = id_to_index_.find(id);
    if (it == id_to_index_.end()) return;

    int idx = it->second;
    int last = static_cast<int>(pattern_ids_.size()) - 1;

    if (idx != last) {
        // Swap with last
        patterns_.row(idx) = patterns_.row(last);
        strengths_[idx] = strengths_[last];
        int64_t last_id = pattern_ids_[last];
        pattern_ids_[idx] = last_id;
        id_to_index_[last_id] = idx;
    }

    // Remove last
    patterns_.conservativeResize(last, BRAIN_DIM);
    strengths_.pop_back();
    pattern_ids_.pop_back();
    id_to_index_.erase(id);
}

void HopfieldSubstrate::update_strength(int64_t id, float strength) {
    auto it = id_to_index_.find(id);
    if (it != id_to_index_.end()) {
        strengths_[it->second] = strength;
    }
}

} // namespace brain
