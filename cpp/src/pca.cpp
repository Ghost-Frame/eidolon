#include "brain/pca.hpp"
#include <Eigen/Dense>
#include <stdexcept>
#include <algorithm>

namespace brain {

PcaTransform::PcaTransform()
    : n_components(0), fitted(false)
{
    mean = Eigen::VectorXf::Zero(RAW_DIM);
    components = Eigen::MatrixXf::Zero(BRAIN_DIM, RAW_DIM);
}

void PcaTransform::fit(const Eigen::MatrixXf& data, int n_comp) {
    int N = static_cast<int>(data.rows());
    int D = static_cast<int>(data.cols());

    if (N < 2) {
        throw std::runtime_error("PCA requires at least 2 samples");
    }

    // Clamp n_components
    n_comp = std::min({n_comp, N - 1, D, BRAIN_DIM});
    n_components = n_comp;

    // Compute mean
    mean = data.colwise().mean();

    // Center the data
    Eigen::MatrixXf centered = data.rowwise() - mean.transpose();

    // Covariance matrix (D x D), using 1/(N-1)
    Eigen::MatrixXf cov = (centered.transpose() * centered) / static_cast<float>(N - 1);

    // Eigendecomposition of symmetric covariance matrix
    // Eigenvalues returned ascending, so top components are at the end
    Eigen::SelfAdjointEigenSolver<Eigen::MatrixXf> solver(cov);
    if (solver.info() != Eigen::Success) {
        throw std::runtime_error("PCA eigendecomposition failed");
    }

    // Take top n_comp eigenvectors (last columns = largest eigenvalues)
    // eigenvectors().cols() == D (total eigenvalues)
    components = solver.eigenvectors()
        .rightCols(n_comp)
        .transpose()
        .topRows(n_comp);

    fitted = true;
}

Eigen::VectorXf PcaTransform::project(const Eigen::VectorXf& v) const {
    if (!fitted) {
        throw std::runtime_error("PCA not fitted");
    }
    Eigen::VectorXf centered = v - mean;
    Eigen::VectorXf projected = components * centered;

    // L2 normalize
    float norm = projected.norm();
    if (norm > 1e-8f) {
        projected /= norm;
    }
    return projected;
}

Eigen::MatrixXf PcaTransform::project_batch(const Eigen::MatrixXf& data) const {
    if (!fitted) {
        throw std::runtime_error("PCA not fitted");
    }

    // Center: subtract mean from each row
    Eigen::MatrixXf centered = data.rowwise() - mean.transpose();

    // Project: (N x D) * (D x n_comp) = (N x n_comp)
    Eigen::MatrixXf projected = centered * components.transpose();

    // Row-wise L2 normalize
    for (int i = 0; i < static_cast<int>(projected.rows()); ++i) {
        float norm = projected.row(i).norm();
        if (norm > 1e-8f) {
            projected.row(i) /= norm;
        }
    }
    return projected;
}

} // namespace brain
