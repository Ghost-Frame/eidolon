#include "brain/types.hpp"
#include "brain/pca.hpp"
#include "brain/substrate.hpp"
#include "brain/graph.hpp"
#include "brain/interference.hpp"
#include "brain/decay.hpp"
#include "brain/absorb.hpp"
#include "brain/persistence.hpp"
#include "brain/dreaming.hpp"
#include "brain/instincts.hpp"

#ifdef BRAIN_EVOLUTION
#include "brain/evolution.hpp"
#endif

#include <nlohmann/json.hpp>

#include <iostream>
#include <fstream>
#include <string>
#include <vector>
#include <unordered_map>
#include <unordered_set>
#include <algorithm>
#include <numeric>
#include <chrono>
#include <ctime>
#include <cmath>

using json = nlohmann::json;

namespace brain {

// ---- Brain orchestrator ----
class Brain {
public:
    std::vector<BrainMemory> memories;
    std::unordered_map<int64_t, size_t> memory_index; // id -> index in memories
    PcaTransform pca;
    HopfieldSubstrate substrate;
    ConnectionGraph graph;
    sqlite3* db = nullptr;
    std::string db_path;
    std::string data_dir;
    bool initialized = false;
    uint64_t dream_cycle_count = 0;

#ifdef BRAIN_EVOLUTION
    EvolutionState evolution;
#endif

    // Initialize from brain.db
    std::string init(const std::string& path) {
        db_path = path;
        std::string errmsg;

        db = db_open(path, errmsg);
        if (!db) return "Failed to open db: " + errmsg;

        // Load memories
        memories = load_memories(db, errmsg);
        if (!errmsg.empty()) {
            return "Failed to load memories: " + errmsg;
        }

        if (memories.empty()) {
            initialized = true;
            // If instincts.bin exists, apply as ghost pre-training
            if (!data_dir.empty()) {
                std::string inst_path = data_dir + "/instincts.bin";
                auto corpus_opt = brain::load_instincts(inst_path);
                if (corpus_opt.has_value()) {
                    auto& corpus = corpus_opt.value();
                    // Bootstrap PCA from corpus embeddings
                    std::vector<int> valid_idx;
                    for (int i = 0; i < (int)corpus.memories.size(); ++i) {
                        if ((int)corpus.memories[i].embedding.size() == RAW_DIM)
                            valid_idx.push_back(i);
                    }
                    if ((int)valid_idx.size() >= 2) {
                        int n = std::min((int)valid_idx.size(), 50);
                        Eigen::MatrixXf embed_matrix(n, RAW_DIM);
                        for (int r = 0; r < n; ++r) {
                            int mi = valid_idx[r];
                            embed_matrix.row(r) = Eigen::Map<const Eigen::VectorXf>(
                                corpus.memories[mi].embedding.data(), RAW_DIM).transpose();
                        }
                        try { pca.fit(embed_matrix, BRAIN_DIM); } catch (...) {}
                    }
                    brain::apply_instincts(memories, memory_index, substrate, graph, pca, corpus);
                    fprintf(stderr, "[brain] applied instincts: %zu ghost patterns\n", memories.size());
                } else {
                    fprintf(stderr, "[brain] no instincts.bin at %s, starting empty\n", inst_path.c_str());
                }
            }
            return "";
        }

        // Build memory_index
        for (size_t i = 0; i < memories.size(); ++i) {
            memory_index[memories[i].id] = i;
        }

        // Try to load PCA state; if missing, fit it
        bool pca_loaded = load_pca_state(db, pca, errmsg);
        if (!pca_loaded) {
            // Build embedding matrix for fitting
            // Find memories with valid embeddings
            std::vector<int> valid_indices;
            for (int i = 0; i < (int)memories.size(); ++i) {
                if ((int)memories[i].embedding.size() == RAW_DIM) {
                    valid_indices.push_back(i);
                }
            }

            if ((int)valid_indices.size() >= 2) {
                Eigen::MatrixXf embed_matrix(valid_indices.size(), RAW_DIM);
                for (int r = 0; r < (int)valid_indices.size(); ++r) {
                    int mi = valid_indices[r];
                    embed_matrix.row(r) = Eigen::Map<const Eigen::VectorXf>(
                        memories[mi].embedding.data(), RAW_DIM).transpose();
                }
                try {
                    pca.fit(embed_matrix, BRAIN_DIM);
                    save_pca_state(db, pca, errmsg);
                } catch (const std::exception& ex) {
                    fprintf(stderr, "[brain] PCA fit failed: %s\n", ex.what());
                }
            }
        }

        // Project all memories to pattern space (zero-pad to BRAIN_DIM)
        for (auto& mem : memories) {
            if ((int)mem.embedding.size() == RAW_DIM && pca.is_fitted()) {
                Eigen::VectorXf raw = Eigen::Map<const Eigen::VectorXf>(
                    mem.embedding.data(), RAW_DIM);
                Eigen::VectorXf proj = pca.project(raw);
                mem.pattern = Eigen::VectorXf::Zero(BRAIN_DIM);
                int copy_dims = std::min((int)proj.size(), BRAIN_DIM);
                mem.pattern.head(copy_dims) = proj.head(copy_dims);
            }
            substrate.store(mem.id, mem.pattern, mem.decay_factor);
            graph.add_node(mem.id);
        }

        // Load edges
        auto edges = load_edges(db, errmsg);
        for (auto& e : edges) {
            graph.add_edge(e.source_id, e.target_id, e.weight, e.edge_type);
        }

        // Load evolution state if feature is enabled
#ifdef BRAIN_EVOLUTION
        evolution = EvolutionState::load_state(db);
#endif

        initialized = true;
        return "";
    }

    // Query: retrieve activated memories for a given embedding
    json query(const std::vector<float>& raw_embedding, int top_k, float beta, int spread_hops) {
        auto t0 = std::chrono::steady_clock::now();

        // Project query (zero-pad to BRAIN_DIM for consistent substrate dims)
        Eigen::VectorXf query_pattern;
        if ((int)raw_embedding.size() == RAW_DIM && pca.is_fitted()) {
            Eigen::VectorXf raw = Eigen::Map<const Eigen::VectorXf>(raw_embedding.data(), RAW_DIM);
            Eigen::VectorXf proj = pca.project(raw);
            query_pattern = Eigen::VectorXf::Zero(BRAIN_DIM);
            int copy_dims = std::min((int)proj.size(), BRAIN_DIM);
            query_pattern.head(copy_dims) = proj.head(copy_dims);
        } else if ((int)raw_embedding.size() == BRAIN_DIM) {
            query_pattern = Eigen::Map<const Eigen::VectorXf>(raw_embedding.data(), BRAIN_DIM);
            float norm = query_pattern.norm();
            if (norm > 1e-8f) query_pattern /= norm;
        } else {
            return json{{"error", "invalid embedding dimension"}};
        }

        // Hopfield retrieve (2x candidates for spreading)
        auto retrieved = substrate.retrieve(query_pattern, top_k * 2, beta);

        // Build seed activations for graph spread
        std::unordered_map<int64_t, float> seeds;
        for (auto& r : retrieved) {
            seeds[r.id] = r.activation;
        }

        // Graph spread
        auto spread_results = graph.spread(seeds, spread_hops);

        // Merge: max activation between Hopfield and spread
        std::unordered_map<int64_t, float> merged;
        std::unordered_map<int64_t, std::string> source_map;
        std::unordered_map<int64_t, int> hop_map;

        for (auto& r : retrieved) {
            merged[r.id] = r.activation;
            source_map[r.id] = "hopfield";
            hop_map[r.id] = 0;
        }
        for (auto& s : spread_results) {
            auto it = merged.find(s.id);
            if (it == merged.end()) {
                merged[s.id] = s.activation;
                source_map[s.id] = "spread";
                hop_map[s.id] = s.hops;
            } else {
                if (s.activation > it->second) {
                    it->second = s.activation;
                    source_map[s.id] = "both";
                    hop_map[s.id] = s.hops;
                } else {
                    source_map[s.id] = "both";
                }
            }
        }

        // Evolution post-processing: apply learned node weights
#ifdef BRAIN_EVOLUTION
        for (auto& [id, act] : merged) {
            act *= evolution.get_node_weight(id);
        }
#endif

        // Apply decay factor weight
        double now_epoch = static_cast<double>(std::time(nullptr));
        for (auto& [id, act] : merged) {
            auto mit = memory_index.find(id);
            if (mit == memory_index.end()) continue;
            auto& mem = memories[mit->second];
            act *= mem.decay_factor;
        }

        // Find contradiction pairs
        std::unordered_set<int64_t> active_set;
        for (auto& [id, _] : merged) active_set.insert(id);
        auto contra_pairs = graph.contradiction_pairs(active_set);

        // Resolve interference for contradiction pairs
        for (auto& [id_a, id_b] : contra_pairs) {
            auto ia = memory_index.find(id_a);
            auto ib = memory_index.find(id_b);
            if (ia == memory_index.end() || ib == memory_index.end()) continue;
            auto& mem_a = memories[ia->second];
            auto& mem_b = memories[ib->second];
            float act_a = merged[id_a];
            float act_b = merged[id_b];
            double epoch_a = parse_datetime_approx(mem_a.created_at);
            double epoch_b = parse_datetime_approx(mem_b.created_at);
            resolve_interference(act_a, act_b,
                                  mem_a.decay_factor, mem_b.decay_factor,
                                  mem_a.importance, mem_b.importance,
                                  epoch_a, epoch_b, now_epoch);
            merged[id_a] = act_a;
            merged[id_b] = act_b;
        }

        // Hebbian: strengthen co-activated pairs (top seeds)
        if (retrieved.size() >= 2) {
            for (size_t i = 0; i < std::min((size_t)5, retrieved.size()); ++i) {
                for (size_t j = i + 1; j < std::min((size_t)5, retrieved.size()); ++j) {
                    graph.strengthen_edge(retrieved[i].id, retrieved[j].id, 0.02f);
                }
            }
        }

        // Apply recall boost to activated memories
        for (auto& [id, act] : merged) {
            auto mit = memory_index.find(id);
            if (mit == memory_index.end()) continue;
            auto& mem = memories[mit->second];
            mem.decay_factor = apply_recall_boost(mem.decay_factor);
            mem.activation = act;
            mem.access_count++;
            mem.last_activated = now_epoch;
            substrate.update_strength(id, mem.decay_factor);
        }

        // Sort by activation, take top_k
        std::vector<std::pair<int64_t, float>> ranked(merged.begin(), merged.end());
        std::sort(ranked.begin(), ranked.end(),
                  [](const auto& a, const auto& b) { return a.second > b.second; });
        if ((int)ranked.size() > top_k) ranked.resize(top_k);

        // Build response
        json activated = json::array();
        for (auto& [id, act] : ranked) {
            auto mit = memory_index.find(id);
            if (mit == memory_index.end()) continue;
            auto& mem = memories[mit->second];
            activated.push_back({
                {"id", mem.id},
                {"content", mem.content},
                {"category", mem.category},
                {"activation", act},
                {"source", source_map.count(id) ? source_map.at(id) : "hopfield"},
                {"hops", hop_map.count(id) ? hop_map.at(id) : 0},
                {"decay_factor", mem.decay_factor},
                {"importance", mem.importance},
                {"created_at", mem.created_at}
            });
        }

        json contradictions = json::array();
        for (auto& [id_a, id_b] : contra_pairs) {
            float act_a = merged.count(id_a) ? merged.at(id_a) : 0.0f;
            float act_b = merged.count(id_b) ? merged.at(id_b) : 0.0f;
            int64_t winner = (act_a >= act_b) ? id_a : id_b;
            int64_t loser  = (act_a >= act_b) ? id_b : id_a;
            contradictions.push_back({
                {"winner_id", winner},
                {"loser_id", loser},
                {"winner_activation", std::max(act_a, act_b)},
                {"loser_activation", std::min(act_a, act_b)},
                {"reason", "contradiction_edge"}
            });
        }

        auto t1 = std::chrono::steady_clock::now();
        double ms = std::chrono::duration<double, std::milli>(t1 - t0).count();

        return {
            {"activated", activated},
            {"contradictions", contradictions},
            {"total_patterns", (int)memories.size()},
            {"query_time_ms", ms}
        };
    }

    // Absorb a new memory from JSON
    json absorb(const json& cmd) {
        BrainMemory mem;
        mem.id         = cmd.value("id", (int64_t)0);
        mem.content    = cmd.value("content", std::string(""));
        mem.category   = cmd.value("category", std::string(""));
        mem.source     = cmd.value("source", std::string(""));
        mem.importance = cmd.value("importance", 5);
        mem.created_at = cmd.value("created_at", std::string(""));
        mem.decay_factor = 1.0f;

        if (cmd.contains("embedding") && cmd["embedding"].is_array()) {
            for (auto& v : cmd["embedding"]) {
                mem.embedding.push_back(v.get<float>());
            }
        }
        if (cmd.contains("tags") && cmd["tags"].is_array()) {
            for (auto& t : cmd["tags"]) {
                mem.tags.push_back(t.get<std::string>());
            }
        }

        // Build pointer list for existing memories
        std::vector<BrainMemory*> all_ptrs;
        all_ptrs.reserve(memories.size());
        for (auto& m : memories) all_ptrs.push_back(&m);

        absorb_memory(mem, all_ptrs, pca, substrate, graph);

        // Check ghost replacement: remove ghosts similar to this real memory
        {
            size_t removed = brain::check_ghost_replacement(
                mem.pattern, memories, memory_index, substrate, graph);
            if (removed > 0) {
                fprintf(stderr, "[brain] absorbed real memory id=%lld, replaced %zu ghost(s)\n",
                        (long long)mem.id, removed);
            }
        }

        // Register in memories list
        size_t new_idx = memories.size();
        memories.push_back(std::move(mem));
        memory_index[memories.back().id] = new_idx;

        return {{"absorbed_id", memories.back().id}};
    }

    // Decay tick
    json decay_tick(int ticks) {
        int removed = 0;
        std::vector<int64_t> dead_ids;

        for (auto& mem : memories) {
            mem.decay_factor = compute_pattern_decay(mem.decay_factor, mem.importance, ticks);
            if (is_dead(mem.decay_factor)) {
                dead_ids.push_back(mem.id);
            }
        }

        // Remove dead memories
        for (int64_t dead_id : dead_ids) {
            substrate.remove(dead_id);
            auto mit = memory_index.find(dead_id);
            if (mit != memory_index.end()) {
                size_t idx = mit->second;
                // Swap with last
                if (idx < memories.size() - 1) {
                    memories[idx] = std::move(memories.back());
                    memory_index[memories[idx].id] = idx;
                }
                memories.pop_back();
                memory_index.erase(dead_id);
                ++removed;
            }
        }

        // Decay edges
        graph.decay_edges(EDGE_DECAY_RATE);

        return {
            {"ticks", ticks},
            {"removed_patterns", removed},
            {"remaining_patterns", (int)memories.size()}
        };
    }

    // Dream cycle
    brain::DreamCycleResult run_dream_cycle() {
        ++dream_cycle_count;
        return brain::dream_cycle(substrate, graph, memories, memory_index, dream_cycle_count);
    }

    // Stats
    json get_stats() {
        if (memories.empty()) {
            return {
                {"total_patterns", 0},
                {"total_edges", 0},
                {"avg_activation", 0.0},
                {"avg_decay_factor", 0.0},
                {"health_distribution", json::object()},
                {"top_activated", json::array()},
                {"bottom_activated", json::array()}
            };
        }

        double sum_act = 0.0, sum_decay = 0.0;
        std::unordered_map<std::string, int> health_dist;

        for (auto& mem : memories) {
            sum_act += mem.activation;
            sum_decay += mem.decay_factor;
            health_dist[classify_health(mem.decay_factor)]++;
        }

        double avg_act   = sum_act / memories.size();
        double avg_decay = sum_decay / memories.size();

        // Top 10 by activation
        std::vector<size_t> by_act(memories.size());
        std::iota(by_act.begin(), by_act.end(), 0);
        std::partial_sort(by_act.begin(),
                          by_act.begin() + std::min((size_t)10, by_act.size()),
                          by_act.end(),
                          [&](size_t a, size_t b) {
                              return memories[a].activation > memories[b].activation;
                          });

        json top_act = json::array();
        for (size_t i = 0; i < std::min((size_t)10, memories.size()); ++i) {
            auto& m = memories[by_act[i]];
            top_act.push_back({
                {"id", m.id},
                {"content_preview", m.content_preview()},
                {"activation", m.activation}
            });
        }

        // Bottom 10 by decay_factor
        std::vector<size_t> by_decay(memories.size());
        std::iota(by_decay.begin(), by_decay.end(), 0);
        std::partial_sort(by_decay.begin(),
                          by_decay.begin() + std::min((size_t)10, by_decay.size()),
                          by_decay.end(),
                          [&](size_t a, size_t b) {
                              return memories[a].decay_factor < memories[b].decay_factor;
                          });

        json bottom_act = json::array();
        for (size_t i = 0; i < std::min((size_t)10, memories.size()); ++i) {
            auto& m = memories[by_decay[i]];
            bottom_act.push_back({
                {"id", m.id},
                {"content_preview", m.content_preview()},
                {"decay_factor", m.decay_factor}
            });
        }

        return {
            {"total_patterns", (int)memories.size()},
            {"total_edges", (int)graph.edge_count()},
            {"avg_activation", avg_act},
            {"avg_decay_factor", avg_decay},
            {"health_distribution", health_dist},
            {"top_activated", top_act},
            {"bottom_activated", bottom_act}
        };
    }

    ~Brain() {
        if (db) db_close(db);
    }
};

} // namespace brain

// ---- Main stdio JSON loop ----
int main() {
    // Disable buffering on stdout for line-by-line JSON
    std::ios::sync_with_stdio(false);
    std::cout.setf(std::ios::unitbuf);

    brain::Brain brain_instance;

    auto make_response = [](bool ok, const std::string& cmd, uint64_t seq,
                             const json& data = nullptr,
                             const std::string& error = "") -> json {
        json r;
        r["ok"]  = ok;
        r["cmd"] = cmd;
        r["seq"] = seq;
        if (!error.empty()) r["error"] = error;
        if (!data.is_null()) r["data"] = data;
        return r;
    };

    std::string line;
    while (std::getline(std::cin, line)) {
        if (line.empty()) continue;

        json req;
        try {
            req = json::parse(line);
        } catch (const std::exception& ex) {
            json err = make_response(false, "parse_error", 0, nullptr,
                                     std::string("JSON parse error: ") + ex.what());
            std::cout << err.dump() << "\n";
            continue;
        }

        std::string cmd  = req.value("cmd", std::string(""));
        uint64_t seq     = req.value("seq", (uint64_t)0);

        json response;

        if (cmd == "init") {
            std::string db_path = req.value("db_path", std::string(""));
            std::string data_dir_val = req.value("data_dir", std::string(""));
            brain_instance.data_dir = data_dir_val;
            if (db_path.empty()) {
                response = make_response(false, cmd, seq, nullptr, "db_path required");
            } else {
                std::string err = brain_instance.init(db_path);
                if (!err.empty()) {
                    response = make_response(false, cmd, seq, nullptr, err);
                } else {
                    json d = {
                        {"patterns_loaded", (int)brain_instance.memories.size()},
                        {"pca_fitted", brain_instance.pca.is_fitted()}
                    };
                    response = make_response(true, cmd, seq, d);
                }
            }
        } else if (cmd == "query") {
            if (!brain_instance.initialized) {
                response = make_response(false, cmd, seq, nullptr, "brain not initialized");
            } else {
                std::vector<float> emb;
                if (req.contains("embedding") && req["embedding"].is_array()) {
                    for (auto& v : req["embedding"]) emb.push_back(v.get<float>());
                }
                int top_k       = req.value("top_k", 10);
                float beta      = req.value("beta", brain::HopfieldSubstrate::DEFAULT_BETA);
                int spread_hops = req.value("spread_hops", 3);
                json result     = brain_instance.query(emb, top_k, beta, spread_hops);
                if (result.contains("error")) {
                    response = make_response(false, cmd, seq, nullptr,
                                             result["error"].get<std::string>());
                } else {
                    response = make_response(true, cmd, seq, result);
                }
            }
        } else if (cmd == "absorb") {
            if (!brain_instance.initialized) {
                response = make_response(false, cmd, seq, nullptr, "brain not initialized");
            } else {
                json result = brain_instance.absorb(req);
                response = make_response(true, cmd, seq, result);
            }
        } else if (cmd == "decay_tick") {
            if (!brain_instance.initialized) {
                response = make_response(false, cmd, seq, nullptr, "brain not initialized");
            } else {
                int ticks = req.value("ticks", 1);
                json result = brain_instance.decay_tick(ticks);
                response = make_response(true, cmd, seq, result);
            }
        } else if (cmd == "get_stats") {
            if (!brain_instance.initialized) {
                response = make_response(false, cmd, seq, nullptr, "brain not initialized");
            } else {
                json result = brain_instance.get_stats();
                response = make_response(true, cmd, seq, result);
            }
        } else if (cmd == "dream_cycle") {
            if (!brain_instance.initialized) {
                response = make_response(false, cmd, seq, nullptr, "brain not initialized");
            } else {
                auto result = brain_instance.run_dream_cycle();
                json d = {
                    {"replayed",        (int)result.replayed},
                    {"merged",          (int)result.merged},
                    {"pruned_patterns", (int)result.pruned_patterns},
                    {"pruned_edges",    (int)result.pruned_edges},
                    {"discovered",      (int)result.discovered},
                    {"resolved",        (int)result.resolved},
                    {"cycle_time_ms",   (int)result.cycle_time_ms}
                };
                response = make_response(true, cmd, seq, d);
            }
        } else if (cmd == "feedback_signal") {
#ifdef BRAIN_EVOLUTION
            brain::FeedbackSignal sig;
            sig.useful = req.value("useful", false);
            sig.timestamp = static_cast<double>(std::time(nullptr));

            if (req.contains("memory_ids") && req["memory_ids"].is_array()) {
                for (auto& v : req["memory_ids"]) {
                    sig.memory_ids.push_back(v.get<int64_t>());
                }
            }
            if (req.contains("edge_pairs") && req["edge_pairs"].is_array()) {
                for (auto& pair : req["edge_pairs"]) {
                    if (pair.is_array() && pair.size() >= 2) {
                        sig.edge_pairs.push_back({pair[0].get<int64_t>(), pair[1].get<int64_t>()});
                    }
                }
            }

            brain_instance.evolution.record_feedback(std::move(sig));
            response = make_response(true, cmd, seq, json{{"recorded", true}});
#else
            response = make_response(false, cmd, seq, nullptr, "evolution not enabled");
#endif
        } else if (cmd == "evolution_train") {
#ifdef BRAIN_EVOLUTION
            brain_instance.evolution.train_step();
            if (brain_instance.db) {
                brain_instance.evolution.save_state(brain_instance.db);
            }
            response = make_response(true, cmd, seq,
                json{{"generation", brain_instance.evolution.generation}});
#else
            response = make_response(false, cmd, seq, nullptr, "evolution not enabled");
#endif
        } else if (cmd == "evolution_stats") {
#ifdef BRAIN_EVOLUTION
            auto s = brain_instance.evolution.stats();
            response = make_response(true, cmd, seq, json{
                {"generation", s.generation},
                {"num_node_weights", s.num_node_weights},
                {"num_edge_weights", s.num_edge_weights},
                {"learning_rate", s.learning_rate}
            });
#else
            response = make_response(false, cmd, seq, nullptr, "evolution not enabled");
#endif
        } else if (cmd == "generate_instincts") {
            std::string output_path = req.value("output_path", std::string(""));
            if (output_path.empty()) {
                response = make_response(false, cmd, seq, nullptr, "output_path required");
            } else {
                auto corpus = brain::generate_instincts();
                bool ok = brain::save_instincts(corpus, output_path);
                if (ok) {
                    size_t file_size = 0;
                    {
                        std::ifstream fs(output_path, std::ios::binary | std::ios::ate);
                        if (fs) file_size = (size_t)fs.tellg();
                    }
                    json d = {
                        {"memories", (int)corpus.memories.size()},
                        {"edges", (int)corpus.edges.size()},
                        {"output_path", output_path},
                        {"file_size_bytes", (int)file_size}
                    };
                    response = make_response(true, cmd, seq, d);
                    fprintf(stderr, "[brain] generated instincts: %zu memories, %zu edges, %zu bytes\n",
                            corpus.memories.size(), corpus.edges.size(), file_size);
                } else {
                    response = make_response(false, cmd, seq, nullptr, "failed to write " + output_path);
                }
            }
        } else if (cmd == "shutdown") {
            response = make_response(true, cmd, seq, json{{"msg", "bye"}});
            std::cout << response.dump() << "\n";
            break;
        } else {
            response = make_response(false, cmd, seq, nullptr,
                                     "unknown command: " + cmd);
        }

        std::cout << response.dump() << "\n";
    }

    return 0;
}
