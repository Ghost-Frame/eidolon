# Engram Living Brain -- Design Spec

> Engram stops being a memory database. It becomes a living neural substrate -- an organic, continuously learning intelligence that understands, corrects, forgets, and dreams.

**Date:** 2026-03-27
**Status:** Experimental research
**Supersedes:** 2026-03-27-neural-memory-design.md (fine-tuning approach abandoned)
**Location:** Inside Engram itself. The brain IS Engram. Engram IS the brain.
**Runtime:** Rocky (primary), Hetzner (production), Windows PC (secondary)

---

## The Problem

Engram stores documents and searches them. No matter how many scoring layers, rerankers, or graph traversals get stacked on top, it remains: store fact, retrieve fact. Robotic.

Agents constantly work with stale information because Engram returns what matches a query, not what is actually true. When an agent searches "where does Engram run," it gets ten results from different points in time and has to figure out which is current. A human would just know.

The deeper problem: Engram's intelligence is all infrastructure with no understanding. It computes entity cooccurrences, personality signals, causal chains, structured facts, memory links, contradiction detection, consolidation, reflections, and decay scoring. All of it stored. Almost none of it used during retrieval. And even if it were all wired in, it would still be robotic -- more scoring dimensions on the same document-retrieval paradigm.

the operator's brain doesn't work this way. Neither should Engram.

---

## The Vision

Engram becomes an extension of the operator's brain. Not a library to search -- a mind that knows what you know. When an agent queries it, they are not looking something up. They are asking the operator, except the operator is not there. And it answers the way the operator would -- with context, with history, with "no, that changed last week, keep up."

Example of how it should work:

> Agent: "Engram runs on Windows"
> Brain: "No. Engram used to run on Windows but it runs on Hetzner now. It has been on Hetzner for a week. What you think is wrong and here is why: [the migration happened on March 20th, here are the memories that document it, the Windows instance was decommissioned]."

This is not retrieval. This is maintained understanding with temporal awareness and the confidence to correct.

---

## Core Principles

### 1. No Tables, No Schemas -- A Continuous Neural Space

Every memory that enters Engram becomes a **pattern of activation** across a high-dimensional space. Not an embedding vector in a row. A living pattern that connects to other patterns, strengthens when reinforced, fades when ignored, and competes with contradictory patterns.

### 2. Associations Are Connection Weights, Not Foreign Keys

Two concepts that co-occur do not get a join table row. The connection between their activation patterns strengthens. Over time, thinking about "Engram" naturally activates "Hetzner" because the connection is strong. "Windows" barely activates because that connection has decayed.

### 3. Contradiction Is Interference, Not a Flag

When an old pattern ("Engram on Windows") and a new pattern ("Engram on Hetzner") overlap in the same conceptual region, they interfere. The stronger pattern dominates. The weaker one does not get deleted -- it fades. Still there if you dig, like a memory you can barely recall.

### 4. Querying Is Pattern Completion

An agent sends a partial signal. That signal activates a partial pattern. The network completes it -- filling in the strongest, most connected, most alive associations. The response is not retrieved. It crystallizes from the network state.

### 5. Forgetting Is Natural Decay

Patterns that never get activated gradually lose connection strength. They do not get deleted. They become unreachable. The network's finite capacity means old, unused patterns get overwritten by new, active ones.

### 6. Dreaming Is Offline Consolidation

When idle, the network replays patterns, strengthens important connections, resolves lingering interference, and merges redundant patterns. Not a cron job. A continuous background process.

### 7. Correction Is Competitive Learning

"That is wrong" is a strong counter-signal that suppresses the wrong pattern and reinforces the right one. The wrong pattern loses a fight and gets weaker. Over time the brain stops making that mistake -- not because a rule says so, but because the wrong pattern lost.

### 8. It Ships With Instincts, Not Memories

A new Engram instance is not an empty shell. It ships with pre-trained instincts: it knows what quality information looks like, it knows how to form associations, it knows that newer state tends to supersede older state. The neural wiring for HOW to think is there from day one. What to think about comes from the operator's data.

### 9. Always Learning, Never "Trained"

There is no training phase and inference phase. The brain is always absorbing, always adjusting. A new memory shifts the network. A correction reshapes it. An idle period consolidates it. It never stops.

### 10. You Do Not Configure It, You Teach It

No threshold tuning, no scoring weights. You use it. It learns what matters from how you and your agents interact with it.

---

## Architecture: The Neural Substrate

### The Space

A high-dimensional continuous vector space where every concept, entity, fact, and relationship exists as a region of activation. This is not an embedding index -- it is a dynamic, mutable space where the geometry itself encodes understanding.

- **Dimensionality:** 256-1024 (to be determined experimentally)
- **Every memory creates a pattern** that occupies a region in this space
- **Patterns overlap** -- "Engram" and "Hetzner" share dimensions because they are connected
- **The space evolves** -- dimensions shift meaning as the brain learns

### The Network

A graph of learned connections between activation patterns.

- **Nodes:** Activation patterns (derived from memories, entities, concepts)
- **Edges:** Weighted, typed connections (association, temporal, contradiction, causation)
- **Edge weights are mutable** -- they strengthen with use, decay with neglect
- **Message passing** propagates activation through the network -- querying one pattern activates its neighborhood

### The Lifecycle of a Memory

1. **Absorption:** Raw memory enters. The brain generates an activation pattern (not just an embedding -- a pattern shaped by existing network context). Connections form to existing patterns based on semantic and temporal proximity.

2. **Integration:** The new pattern settles into the network. If it reinforces existing patterns, those connections strengthen. If it contradicts, interference occurs and the competing patterns enter competition.

3. **Consolidation:** During idle periods, the brain revisits recent patterns. Strong, reinforced patterns get deeper integration. Weak, isolated patterns begin to fade. Redundant patterns merge.

4. **Recall:** A query activates a partial pattern. The network completes it via activation spreading. The strongest, most connected response crystallizes. The act of recall itself reinforces the recalled patterns (spaced repetition as a natural consequence, not a scheduled algorithm).

5. **Decay:** Patterns that are never activated lose connection strength over time. They become progressively harder to recall. Eventually they are effectively gone -- still technically present but unreachable, like a human memory that has faded beyond retrieval.

6. **Correction:** Negative feedback suppresses patterns. Positive feedback reinforces them. The brain reshapes around corrections through competitive dynamics, not database updates.

---

## Two Branches

### Branch B: Association + Reasoning (Conservative)

The brain returns memories, not generated text. But it returns them with understanding.

- Query activates a pattern, the network completes it
- Returns a constellation of related memories, ordered by activation strength
- Includes temporal context ("this changed on March 20th")
- Includes contradiction awareness ("the old answer was X, current answer is Y")
- Can do multi-hop reasoning through the activation network
- **Cannot hallucinate** -- only returns content derived from stored patterns

### Branch C: Generative Oracle (Frontier)

Same neural substrate, but with a generation layer that synthesizes natural language responses.

- Query activates a pattern, the network completes it
- A response generation layer produces a natural language answer grounded in the activated patterns
- Cites source memories
- Pushes back on wrong assumptions
- **Will hallucinate initially.** The plan: let it hallucinate, identify patterns in hallucination, tune the generation layer to suppress them, iterate. Learn from what does not work.

Both branches share the same living substrate. The difference is only in the output layer.

---

## The Curated Corpus

The brain needs clean food. Current Engram has ~1900 memories with significant noise: early development debris, agent troubleshooting, stale state, duplicates, debug output.

### Approach: Separate Clean Database, Progressive Curation

**`brain.db`** -- a new database alongside `memory.db` containing only curated, high-quality memories worthy of being absorbed into the neural substrate.

**Initial population:**
1. Export current memories
2. Multi-pass filtering:
   - Dedup: collapse near-duplicates (SimHash)
   - Staleness: identify superseded state, keep only current truth
   - Noise: remove debug output, test data, build logs, agent troubleshooting chatter
   - Value: score by lasting utility
3. Filtered candidates go to review queue. the operator approves/rejects until filters are trusted.

**Ongoing:**
- New memories hit `memory.db` immediately (nothing breaks for current consumers)
- Curation pipeline processes them into `brain.db`
- The brain trains on `brain.db` only
- Over time, as quality improves, `brain.db` becomes the source of truth

**Graduation:** When the curated corpus is clean and complete enough, the brain runs entirely on curated data. `memory.db` becomes the raw archive.

---

## Shipping: Instincts, Not Memories

The open source deliverable is the architecture, training pipeline, and curation tools. No weights shipped. Every instance grows its own brain.

But it is not an empty shell. It ships with **instincts** -- pre-trained on a synthetic corpus of structurally realistic but non-personal memories. These instincts give the brain:

- Quality intuition (knows noise from signal without being told)
- Association grammar (knows that temporal proximity implies context)
- State transition awareness (knows that newer state tends to supersede)
- Learning velocity (the first 100 real memories teach it fast because the wiring is primed)

The synthetic corpus is generic. The real data makes it personal. The instincts make it useful from day one.

---

## Phase 1: The Minimum Living Thing

**Goal:** Build the smallest possible thing that exhibits organic behavior. Not feature-complete. Alive.

1. **Build the neural space** -- A dynamic activation network that can absorb memories as patterns, form connections, and decay unused ones.

2. **Build pattern completion** -- Given a partial query, the network completes it by activating the strongest connected patterns. This is the core "recall" operation.

3. **Build interference** -- When contradictory patterns exist, the stronger one wins during completion. This gives us the "that is wrong, here is what is current" behavior.

4. **Build the curation pipeline** -- Filter current memories into a clean corpus. Feed that corpus to the brain.

5. **Build the absorption loop** -- New memories enter the brain continuously. No training phases.

6. **Build decay** -- Unused patterns fade over time without explicit deletion.

7. **Test with the OpenSpace question** -- "Does OpenSpace exist?" should produce an answer that demonstrates understanding, not retrieval. If the brain knows OpenSpace was absorbed into Engram's graph module despite no single memory stating that directly, Phase 1 is a success.

### What Phase 1 Is NOT

- Not optimized
- Not feature-complete
- Not production-grade
- Not the final architecture
- A living experiment that we learn from

---

## Open Questions (To Be Answered By Building)

1. What dimensionality produces the best patterns for ~2000 memories?
2. How fast should decay be? Too fast and the brain forgets useful things. Too slow and noise persists.
3. How does interference resolution actually work in practice? Does the stronger pattern always win, or do we need nuance?
4. Can pattern completion handle multi-hop reasoning, or do we need explicit message-passing layers?
5. What does "dreaming" look like as an implementation? Background thread? Idle hook? Periodic sweep?
6. How do we evaluate whether the brain "understands" something vs just pattern-matching?
7. What is the threshold where the brain has enough curated data to be useful?
8. How do instincts work in practice? What does the synthetic training corpus look like?
9. Can Branch C's hallucinations actually be tuned out, or is generation fundamentally at odds with the "only know what you have learned" principle?
10. How does the brain handle genuinely ambiguous or uncertain knowledge?

These are not blockers. They are the things we will learn by building. The experiment IS the answer.

---

## Evolution Target: The Neuro-Symbolic Graph

Phase 1 uses established building blocks (Hopfield networks, GNNs, learned embeddings) composed in a novel way to create organic behavior. But the long-term vision is more radical:

**The memory graph itself IS the neural network.** No separate "model" and "data." Every memory node carries trainable weights. Every edge carries a learned transformation. Intelligence emerges from the graph structure plus learned propagation rules. The graph literally thinks.

We build toward this as the architecture matures and we understand what works.

---

## What Success Looks Like

Not metrics. Behaviors.

- An agent asks a question and gets an answer that demonstrates understanding, not just keyword matching
- The brain corrects agents who have stale information, unprompted
- Old, irrelevant patterns fade without manual cleanup
- New information integrates naturally without retraining commands
- Two Engram instances with different data develop fundamentally different understanding
- The brain gets better the more you use it, without configuration
- It feels like talking to someone who knows, not searching a database
