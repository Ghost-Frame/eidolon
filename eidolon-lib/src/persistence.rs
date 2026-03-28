use rusqlite::{Connection, OptionalExtension};
use ndarray::{Array1, Array2};
use crate::types::{BrainMemory, BrainEdge, EdgeType, BRAIN_DIM, RAW_DIM};
use crate::pca::PcaTransform;

/// Load all memories from brain.db.
pub fn load_memories(conn: &Connection) -> Result<Vec<BrainMemory>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT id, content, category, source, importance, created_at, embedding, tags FROM brain_memories"
    )?;

    let memories = stmt.query_map([], |row| {
        let id: i64 = row.get(0)?;
        let content: String = row.get(1)?;
        let category: String = row.get(2)?;
        let source: String = row.get(3)?;
        let importance: i32 = row.get(4)?;
        let created_at: String = row.get(5)?;
        let embedding_blob: Vec<u8> = row.get(6)?;
        let tags_json: Option<String> = row.get(7)?;

        let embedding = blob_to_f32(&embedding_blob);
        let tags: Vec<String> = tags_json
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();

        Ok(BrainMemory {
            id,
            content,
            category,
            source,
            importance,
            created_at,
            embedding,
            pattern: Array1::zeros(BRAIN_DIM),
            activation: 1.0,
            last_activated: 0.0,
            access_count: 0,
            decay_factor: 1.0,
            tags,
        })
    })?
    .filter_map(|r| r.ok())
    .collect();

    Ok(memories)
}

/// Load all edges from brain.db.
pub fn load_edges(conn: &Connection) -> Result<Vec<BrainEdge>, rusqlite::Error> {
    let mut stmt = conn.prepare(
        "SELECT source_id, target_id, weight, edge_type, created_at FROM brain_edges"
    )?;

    let edges = stmt.query_map([], |row| {
        let source_id: i64 = row.get(0)?;
        let target_id: i64 = row.get(1)?;
        let weight: f64 = row.get(2)?;
        let edge_type_str: String = row.get(3)?;
        let created_at: String = row.get(4)?;

        Ok(BrainEdge {
            source_id,
            target_id,
            weight: weight as f32,
            edge_type: EdgeType::from_str(&edge_type_str),
            created_at,
        })
    })?
    .filter_map(|r| r.ok())
    .collect();

    Ok(edges)
}

/// Save a new or updated edge to brain.db.
pub fn save_edge(conn: &Connection, edge: &BrainEdge) -> Result<(), rusqlite::Error> {
    conn.execute(
        "INSERT OR REPLACE INTO brain_edges (source_id, target_id, weight, edge_type, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![
            edge.source_id,
            edge.target_id,
            edge.weight as f64,
            edge.edge_type.as_str(),
            edge.created_at,
        ],
    )?;
    Ok(())
}

/// Save PCA state to brain_meta table.
pub fn save_pca_state(conn: &Connection, pca: &PcaTransform) -> Result<(), rusqlite::Error> {
    // Save mean as blob
    let mean_blob = f32_to_blob(&pca.mean.to_vec());
    conn.execute(
        "INSERT OR REPLACE INTO brain_meta (key, value, updated_at) VALUES ('pca_mean', ?1, datetime('now'))",
        rusqlite::params![mean_blob],
    )?;

    // Save components as blob (row-major flattened)
    let comp_vec: Vec<f32> = pca.components.iter().cloned().collect();
    let comp_blob = f32_to_blob(&comp_vec);
    conn.execute(
        "INSERT OR REPLACE INTO brain_meta (key, value, updated_at) VALUES ('pca_components', ?1, datetime('now'))",
        rusqlite::params![comp_blob],
    )?;

    // Save n_components as metadata
    let meta_json = serde_json::json!({
        "n_components": pca.n_components,
        "n_features": pca.components.ncols(),
    });
    let meta_blob = meta_json.to_string().into_bytes();
    conn.execute(
        "INSERT OR REPLACE INTO brain_meta (key, value, updated_at) VALUES ('pca_meta', ?1, datetime('now'))",
        rusqlite::params![meta_blob],
    )?;

    Ok(())
}

/// Load PCA state from brain_meta table. Returns None if not found.
pub fn load_pca_state(conn: &Connection) -> Result<Option<PcaTransform>, rusqlite::Error> {
    // Load meta
    let meta_blob: Option<Vec<u8>> = conn
        .query_row(
            "SELECT value FROM brain_meta WHERE key = 'pca_meta'",
            [],
            |row| row.get(0),
        )
        .optional()?;

    let meta_blob = match meta_blob {
        None => return Ok(None),
        Some(b) => b,
    };

    let meta: serde_json::Value = match serde_json::from_slice(&meta_blob) {
        Ok(v) => v,
        Err(_) => return Ok(None),
    };

    let n_components = meta["n_components"].as_u64().unwrap_or(0) as usize;
    let n_features = meta["n_features"].as_u64().unwrap_or(RAW_DIM as u64) as usize;

    if n_components == 0 {
        return Ok(None);
    }

    // Load mean
    let mean_blob: Option<Vec<u8>> = conn
        .query_row(
            "SELECT value FROM brain_meta WHERE key = 'pca_mean'",
            [],
            |row| row.get(0),
        )
        .optional()?;

    let mean = match mean_blob {
        None => return Ok(None),
        Some(b) => Array1::from(blob_to_f32(&b)),
    };

    if mean.len() != n_features {
        return Ok(None);
    }

    // Load components
    let comp_blob: Option<Vec<u8>> = conn
        .query_row(
            "SELECT value FROM brain_meta WHERE key = 'pca_components'",
            [],
            |row| row.get(0),
        )
        .optional()?;

    let comp_vec = match comp_blob {
        None => return Ok(None),
        Some(b) => blob_to_f32(&b),
    };

    if comp_vec.len() != n_components * n_features {
        return Ok(None);
    }

    let components = Array2::from_shape_vec((n_components, n_features), comp_vec)
        .map_err(|_| rusqlite::Error::InvalidColumnType(0, "pca_components shape".to_string(), rusqlite::types::Type::Blob))?;

    Ok(Some(PcaTransform {
        components,
        mean,
        n_components,
    }))
}

/// Convert a BLOB (4-byte little-endian f32 chunks) to Vec<f32>.
pub fn blob_to_f32(blob: &[u8]) -> Vec<f32> {
    blob.chunks_exact(4)
        .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect()
}

/// Convert Vec<f32> to BLOB (4-byte little-endian per float).
pub fn f32_to_blob(data: &[f32]) -> Vec<u8> {
    let mut blob = Vec::with_capacity(data.len() * 4);
    for &f in data {
        blob.extend_from_slice(&f.to_le_bytes());
    }
    blob
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blob_roundtrip() {
        let original = vec![1.5_f32, -2.7, 0.0, f32::MAX, f32::MIN_POSITIVE];
        let blob = f32_to_blob(&original);
        let recovered = blob_to_f32(&blob);
        assert_eq!(original.len(), recovered.len());
        for (a, b) in original.iter().zip(recovered.iter()) {
            assert!((a - b).abs() < 1e-10, "mismatch: {} vs {}", a, b);
        }
    }
}
