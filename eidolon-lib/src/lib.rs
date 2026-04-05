pub mod types;
pub mod pca;
pub mod substrate;
pub mod graph;
pub mod interference;
pub mod decay;
pub mod absorb;
pub mod persistence;
pub mod dreaming;

#[cfg(feature = "evolution")]
pub mod evolution;
#[cfg(feature = "reasoning")]
pub mod reasoning;
pub mod instincts;
pub mod brain;
