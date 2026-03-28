#pragma once

#include <Eigen/Core>
#include <string>
#include <vector>
#include <cstdint>

namespace brain {

static constexpr int BRAIN_DIM = 512;
static constexpr int RAW_DIM  = 1024;

enum class EdgeType {
    Association,
    Temporal,
    Contradiction
};

inline const char* edge_type_str(EdgeType et) {
    switch (et) {
        case EdgeType::Association:   return "association";
        case EdgeType::Temporal:      return "temporal";
        case EdgeType::Contradiction: return "contradiction";
    }
    return "association";
}

inline EdgeType edge_type_from_str(const std::string& s) {
    if (s == "temporal")      return EdgeType::Temporal;
    if (s == "contradiction")  return EdgeType::Contradiction;
    return EdgeType::Association;
}

struct BrainMemory {
    int64_t id;
    std::string content;
    std::string category;
    std::string source;
    int32_t importance;
    std::string created_at;
    std::vector<float> embedding;      // RAW_DIM floats
    Eigen::VectorXf pattern;           // BRAIN_DIM floats after PCA
    float activation;
    double last_activated;
    uint32_t access_count;
    float decay_factor;
    std::vector<std::string> tags;

    BrainMemory()
        : id(0), importance(5), activation(0.0f),
          last_activated(0.0), access_count(0), decay_factor(1.0f)
    {
        pattern = Eigen::VectorXf::Zero(BRAIN_DIM);
    }

    std::string content_preview(size_t max_len = 80) const {
        if (content.size() <= max_len) return content;
        return content.substr(0, max_len) + "...";
    }
};

struct BrainEdge {
    int64_t source_id;
    int64_t target_id;
    float weight;
    EdgeType edge_type;
    std::string created_at;

    BrainEdge() : source_id(0), target_id(0), weight(0.0f), edge_type(EdgeType::Association) {}
    BrainEdge(int64_t src, int64_t tgt, float w, EdgeType et, std::string ts = "")
        : source_id(src), target_id(tgt), weight(w), edge_type(et), created_at(std::move(ts)) {}
};

} // namespace brain
