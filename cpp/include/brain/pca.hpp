#pragma once

#include "types.hpp"
#include <Eigen/Core>
#include <vector>
#include <stdexcept>

namespace brain {

class PcaTransform {
public:
    // components: (BRAIN_DIM x RAW_DIM) -- each row is a principal component
    Eigen::MatrixXf components;
    // mean: (RAW_DIM) -- subtracted before projection
    Eigen::VectorXf mean;
    int n_components;
    bool fitted;

    PcaTransform();

    // Fit PCA on data matrix (N x RAW_DIM).
    // Uses Eigen SelfAdjointEigenSolver on covariance matrix.
    // n_comp clamped to min(n_comp, N-1, RAW_DIM).
    void fit(const Eigen::MatrixXf& data, int n_comp = BRAIN_DIM);

    // Project a single RAW_DIM vector to BRAIN_DIM, L2 normalized.
    Eigen::VectorXf project(const Eigen::VectorXf& v) const;

    // Project a batch (N x RAW_DIM) to (N x BRAIN_DIM), each row L2 normalized.
    Eigen::MatrixXf project_batch(const Eigen::MatrixXf& data) const;

    bool is_fitted() const { return fitted; }
};

} // namespace brain
