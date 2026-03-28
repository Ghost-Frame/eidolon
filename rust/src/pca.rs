use ndarray::{Array1, Array2, Axis};
use crate::types::{BRAIN_DIM, RAW_DIM};

#[derive(Debug, Clone)]
pub struct PcaTransform {
    pub components: Array2<f32>,
    pub mean: Array1<f32>,
    pub n_components: usize,
}

impl PcaTransform {
    pub fn new_empty() -> Self {
        PcaTransform {
            components: Array2::zeros((0, RAW_DIM)),
            mean: Array1::zeros(RAW_DIM),
            n_components: 0,
        }
    }

    /// Fit PCA on data matrix (n_samples x RAW_DIM).
    /// Uses power iteration with deflation to find top min(BRAIN_DIM, n_samples-1) eigenvectors.
    pub fn fit(data: &Array2<f32>) -> Self {
        let n_samples = data.nrows();
        let n_features = data.ncols();
        let n_components = BRAIN_DIM.min(n_samples.saturating_sub(1)).max(1);

        // Compute mean
        let mean = data.mean_axis(Axis(0)).unwrap();

        // Center data
        let centered = data - &mean;

        // Compute covariance matrix: (X^T X) / (n-1)
        // For n_features = 1024, this is 1024x1024
        let n = (n_samples as f32 - 1.0).max(1.0);
        let cov = centered.t().dot(&centered) / n;

        // Power iteration with deflation
        let mut components: Vec<Array1<f32>> = Vec::with_capacity(n_components);
        let mut residual = cov.clone();

        for _ in 0..n_components {
            // Random-ish initialization using deterministic seed
            let mut v = Array1::<f32>::zeros(n_features);
            for i in 0..n_features {
                // Deterministic pseudo-random init
                let seed = (i as f32 * 1.6180339887 + components.len() as f32 * 2.7182818284).sin();
                v[i] = seed;
            }
            // Normalize
            let norm = v.dot(&v).sqrt();
            if norm > 1e-10 {
                v /= norm;
            }

            // Power iteration: 100 max iterations, 1e-6 convergence
            for _ in 0..100 {
                let v_new = residual.dot(&v);
                let norm = v_new.dot(&v_new).sqrt();
                if norm < 1e-10 {
                    break;
                }
                let v_new_normalized = v_new / norm;
                let diff = (&v_new_normalized - &v).mapv(|x| x.abs()).sum();
                v = v_new_normalized;
                if diff < 1e-6 {
                    break;
                }
            }

            // Gram-Schmidt orthogonalization against previous components
            for prev in &components {
                let proj = v.dot(prev);
                v = v - prev * proj;
            }

            // Renormalize
            let norm = v.dot(&v).sqrt();
            if norm < 1e-10 {
                // Degenerate eigenvector, use unit vector
                v = Array1::zeros(n_features);
                let idx = components.len() % n_features;
                v[idx] = 1.0;
            } else {
                v /= norm;
            }

            // Deflate: residual -= eigenvalue * v v^T
            let eigenvalue = v.dot(&residual.dot(&v));
            let outer = outer_product(&v, &v) * eigenvalue;
            residual = residual - outer;

            components.push(v);
        }

        // Stack into matrix: n_components x n_features
        let comp_matrix = Array2::from_shape_fn((n_components, n_features), |(i, j)| {
            components[i][j]
        });

        PcaTransform {
            components: comp_matrix,
            mean,
            n_components,
        }
    }

    /// Project a single 1024-dim embedding to brain space (n_components-dim), L2 normalized.
    pub fn project(&self, embedding: &Array1<f32>) -> Array1<f32> {
        if self.n_components == 0 {
            return Array1::zeros(BRAIN_DIM);
        }
        let centered = embedding - &self.mean;
        let projected = self.components.dot(&centered);
        l2_normalize(projected)
    }

    /// Project a batch of embeddings (n x RAW_DIM) -> (n x n_components), row-wise L2 normalized.
    pub fn project_batch(&self, data: &Array2<f32>) -> Array2<f32> {
        if self.n_components == 0 || data.nrows() == 0 {
            return Array2::zeros((data.nrows(), self.n_components.max(1)));
        }
        let centered = data - &self.mean;
        let projected = centered.dot(&self.components.t());
        // Row-wise L2 normalize
        Array2::from_shape_fn((projected.nrows(), projected.ncols()), |(i, j)| {
            projected[[i, j]]
        })
        .rows()
        .into_iter()
        .enumerate()
        .fold(
            Array2::zeros((projected.nrows(), projected.ncols())),
            |mut acc, (i, row)| {
                let norm: f32 = row.dot(&row).sqrt();
                if norm > 1e-10 {
                    for j in 0..row.len() {
                        acc[[i, j]] = row[j] / norm;
                    }
                }
                acc
            },
        )
    }
}

fn outer_product(a: &Array1<f32>, b: &Array1<f32>) -> Array2<f32> {
    let n = a.len();
    Array2::from_shape_fn((n, n), |(i, j)| a[i] * b[j])
}

pub fn l2_normalize(mut v: Array1<f32>) -> Array1<f32> {
    let norm: f32 = v.dot(&v).sqrt();
    if norm > 1e-10 {
        v /= norm;
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_abs_diff_eq;

    fn make_test_data(n: usize, d: usize) -> Array2<f32> {
        Array2::from_shape_fn((n, d), |(i, j)| {
            ((i as f32 * 0.1 + j as f32 * 0.01) * std::f32::consts::PI).sin()
        })
    }

    fn cosine_sim(a: &Array1<f32>, b: &Array1<f32>) -> f32 {
        let dot = a.dot(b);
        let na = a.dot(a).sqrt();
        let nb = b.dot(b).sqrt();
        if na < 1e-10 || nb < 1e-10 {
            return 0.0;
        }
        dot / (na * nb)
    }

    #[test]
    fn pca_basic() {
        let data = make_test_data(64, RAW_DIM);
        let pca = PcaTransform::fit(&data);
        // n_components = min(BRAIN_DIM, 64-1) = 63
        assert!(pca.n_components > 0);
        assert!(pca.n_components <= BRAIN_DIM);
        assert_eq!(pca.components.ncols(), RAW_DIM);
        assert_eq!(pca.mean.len(), RAW_DIM);
    }

    #[test]
    fn pca_preserves_similarity() {
        // Two similar vectors should have high cosine sim after projection
        let mut data = make_test_data(32, RAW_DIM);
        // Make rows 0 and 1 nearly identical
        let row0 = data.row(0).to_owned();
        let noisy = &row0 + &Array1::from_shape_fn(RAW_DIM, |i| (i as f32 * 0.001).sin() * 0.01);
        data.row_mut(1).assign(&noisy);

        let pca = PcaTransform::fit(&data);
        let p0 = pca.project(&data.row(0).to_owned());
        let p1 = pca.project(&data.row(1).to_owned());
        let sim = cosine_sim(&p0, &p1);
        assert!(sim > 0.95, "cosine similarity after PCA: {}", sim);
    }

    #[test]
    fn pca_project_normalized() {
        let data = make_test_data(32, RAW_DIM);
        let pca = PcaTransform::fit(&data);
        let p = pca.project(&data.row(0).to_owned());
        let norm: f32 = p.dot(&p).sqrt();
        assert_abs_diff_eq!(norm, 1.0, epsilon = 1e-5);
    }
}
