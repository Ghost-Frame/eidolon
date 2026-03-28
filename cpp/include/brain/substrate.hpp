#pragma once

#include "types.hpp"
#include <Eigen/Core>
#include <vector>
#include <unordered_map>
#include <cstdint>

namespace brain {

struct RetrievedPattern {
    int64_t id;
    float activation;
    int index; // internal index in patterns_ matrix
};

class HopfieldSubstrate {
public:
    static constexpr float DEFAULT_BETA = 8.0f;
    static constexpr float ACTIVATION_THRESHOLD = 0.01f;

    HopfieldSubstrate();

    // Store or update a pattern (BRAIN_DIM vector) for the given id.
    void store(int64_t id, const Eigen::VectorXf& pattern, float strength = 1.0f);

    // Retrieve top_k patterns most similar to query.
    // Returns activations (softmax of beta * similarity * strength).
    std::vector<RetrievedPattern> retrieve(const Eigen::VectorXf& query,
                                           int top_k = 10,
                                           float beta = DEFAULT_BETA) const;

    // Iterative pattern completion: return refined query after attention steps.
    Eigen::VectorXf complete(const Eigen::VectorXf& noisy_query,
                             int steps = 5,
                             float beta = DEFAULT_BETA) const;

    // Remove a pattern by id.
    void remove(int64_t id);

    // Update strength for existing id.
    void update_strength(int64_t id, float strength);

    int size() const { return static_cast<int>(pattern_ids_.size()); }
    bool has(int64_t id) const { return id_to_index_.count(id) > 0; }

private:
    // patterns_ rows are patterns, columns are dimensions: (N x BRAIN_DIM)
    Eigen::MatrixXf patterns_;
    std::vector<float> strengths_;
    std::vector<int64_t> pattern_ids_;
    std::unordered_map<int64_t, int> id_to_index_;

    // Softmax over a vector
    static Eigen::VectorXf softmax(const Eigen::VectorXf& logits);
};

} // namespace brain
