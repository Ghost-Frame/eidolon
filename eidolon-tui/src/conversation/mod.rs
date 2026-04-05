pub mod personality;
pub mod manager;

// Router moved to intelligence::router.
// Re-export for backward compatibility with existing imports.
pub mod router {
    pub use crate::intelligence::router::*;
}
