#pragma once

#include "types.hpp"
#include "pca.hpp"
#include <vector>
#include <string>

// Forward declare sqlite3 to avoid including the full header here
struct sqlite3;

namespace brain {

// Open or create a SQLite database at path.
// Returns nullptr on failure (check errmsg).
sqlite3* db_open(const std::string& path, std::string& errmsg);
void db_close(sqlite3* db);

// Load all memories from brain_memories table.
// Embeddings are stored as BLOBs (4 bytes per float, little-endian).
std::vector<BrainMemory> load_memories(sqlite3* db, std::string& errmsg);

// Load edges from brain_edges table.
std::vector<BrainEdge> load_edges(sqlite3* db, std::string& errmsg);

// Save a single edge to brain_edges (INSERT OR REPLACE).
bool save_edge(sqlite3* db, const BrainEdge& edge, std::string& errmsg);

// Save PCA state to brain_meta table.
bool save_pca_state(sqlite3* db, const PcaTransform& pca, std::string& errmsg);

// Load PCA state from brain_meta table.
// Returns true if loaded successfully, false if not found.
bool load_pca_state(sqlite3* db, PcaTransform& pca, std::string& errmsg);

} // namespace brain
