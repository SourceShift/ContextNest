# Epic — Disk-first substrate (heap diet)

**Status:** In progress on `feat/disk-first-substrate`.

**Last updated:** 2026-09-30.

## The measured problem

Live operator substrate, 2026-09-30, `contextnest serve` up 13.5 h:

| Signal | Value |
|---|---|
| Fragments / basins / edges | 334,295 / 188,214 / 7,769,824 |
| RSS | 11 MB (meaningless: compressor + swap) |
| `vmmap --summary` bytes allocated | **12.4 GB** in 92 M live allocations |
| Swapped | 12.5 GB |
| `wal.canonical.sqlite` | 15 GB (`objects` table 13.8 GB) |
| Heap per fragment | **~37 KB** for ~1 KB of text |

The "burst" the operator saw (RSS 4.5 GB, 60 % CPU during a Stop-hook
flood) was the consolidation worker brute-force scanning every node
vector, which forced the kernel to decompress the whole swapped heap.

### Where the 12.4 GB lives

Reconciled against malloc size classes (each bucket within ~5 %):

```
 EDGE GRAPH ........ ~6.7 GB  ≤256 B class: 7.77 M edges × 10 allocs
 EMBEDDINGS ........ ~4.8 GB  1.18 M copies of 4 KB vectors
 texts/meta/ids .... ~0.9 GB
```

**Edges, ~870 B each.** `create_connection` stores the UUID edge id five
times (edge map key, the edge, both incident lists, `connection_weights`),
source/target ids twice more (edge + adjacency list), and boxes every edge
in its own one-element `Vec`. Needed payload: two node indices + weight.

**Embeddings, 3.5 copies each.** Fragment store (`MemoryFragment.content`),
graph node (`MemoryNode.content`), the `embeddings_by_id` sidecar (filled
for every fragment at bootstrap), plus 188 k basin centres.

**CPU.** Three linear scans over scattered heap vectors:
`create_connections_for_node` (every node, per new fragment),
`find_nearest_basin_with_distance` (every basin, per new fragment) and the
similarity retrieval strategy (every node, per query).

**Startup.** `CheckpointStore::load` materialises the entire canonical
state as one `CanonicalSnapshot`, then copies every vector into
`embeddings_by_id`, so peak memory at boot exceeds steady state.

**Disk.** Vectors are persisted as JSON text (~11 KB per 1024-d vector
instead of 4 KB), once for the fragment and again for its node.

## Why not "just put it on disk"

Everything is in RAM because every write and query does a full linear
scan. The fix is structural: store each vector once, contiguously, in a
file-backed arena the kernel can page; index the graph with integers; and
persist binary vectors. Graph traversal stays in RAM because it is small
once compact (~0.6 GB) and latency-sensitive (one lookup per hop).

## Target layout

```
            ┌──────────────── process heap (~1 GB) ─────────────────┐
            │ compact graph: 64 B edge records + u32 incident lists │
            │ id ↔ row table (one Arc<str> per fragment)            │
            │ fragment metadata (no vectors), basin state           │
            └──────────────────────────┬────────────────────────────┘
                                       │ contiguous row scans
     ┌─────────────────────────────────▼─────────────────────────────┐
     │ VectorArena: one row per fragment, f32, shared by fragment    │
     │ store + graph. Heap-backed by default; file-backed (mmap)     │
     │ for the operator substrate so the kernel pages it to disk     │
     │ instead of compressing/swapping anonymous memory.             │
     ├───────────────────────────────────────────────────────────────┤
     │ SQLite checkpoint: JSON payloads without vectors + a binary   │
     │ `vectors` table (fragment and basin vectors, little-endian).  │
     └───────────────────────────────────────────────────────────────┘
```

## Stages

Each stage is one commit on the epic branch, gated on `cargo test --lib`,
`cargo fmt --check` and no new clippy errors in touched files.

| # | Stage | Heap saved | Acceptance |
|---|---|---|---|
| 1 | Bound `EmbeddingService` response cache (LRU) | leak guard | `CONTEXTNEST_EMBEDDING_CACHE_MAX_ENTRIES`; eviction unit test |
| 2 | `VectorArena`: single contiguous copy shared by fragment store and graph | ~2.7 GB | fragment + node share one row; scans are contiguous; hydrate-on-read keeps `MemoryFragment` API |
| 3 | Compact graph: u32 node rows, 64 B edge slab, u32 incident lists | ~6 GB | public `ConnectionNetwork` API unchanged; `remove_node` / `reinforce_connections` O(degree) |
| 4 | `embeddings_by_id` holds only unconsolidated ids | ~1.4 GB | bootstrap no longer copies vectors; readers fall back to the arena |
| 5 | Binary checkpoint + streaming restore + offline `checkpoint compact` | ~10 GB disk, boot peak | old JSON rows still load; compaction writes a new file, never touches the source |
| 6 | File-backed arena for the operator substrate | pages to disk | `CONTEXTNEST_VECTOR_ARENA_DIR`; rebuilt from the checkpoint every boot |

### Deliberately deferred

- **HNSW / approximate index.** Contiguous exact scan keeps connection
  formation bit-identical to today. Gate: add an ANN index only if the
  exact-scan benchmark exceeds ~50 ms per scan at the live substrate size.
- **Basin centres in an arena.** Centres move (basin dynamics, merges), so
  they need their own mutable arena; 0.77 GB, follow-up.
- **f16 quantisation.** The arena feeds the canonical checkpoint, so it must
  stay exact. Quantisation only makes sense for a derived index.

## Env knobs added

| Env | Default | Effect |
|---|---|---|
| `CONTEXTNEST_EMBEDDING_CACHE_MAX_ENTRIES` | 4096 | LRU bound on the text→vector response cache (~16 MB at 1024-d). `0` disables caching. |
| `CONTEXTNEST_VECTOR_ARENA_DIR` | unset (heap) | Directory for the operator's file-backed vector arena. The file is derived state, truncated and rebuilt from the checkpoint on every boot. Put it on a volume with free space. |

## Migration and safety

- Old checkpoints load unchanged: a payload whose vector field is present
  is used as-is; a payload with an empty vector reads from `vectors`.
  New saves write the binary form, so the file converges as fragments are
  re-persisted.
- `contextnest checkpoint compact --from <db> --into <new-db>` rewrites a
  whole checkpoint into the binary form by streaming into a **new file**.
  The source is opened read-only. Swap files manually after verifying.
- The operator home volume was 99 % full (7.6 GB free) when this was
  measured, so backups and the compacted output belong on
  `/Volumes/docker-ssd`, not in `~/.contextnest`.

## Verification

- Unit tests for the arena (claim/release holders, free-row reuse, growth,
  file backing), the compact graph (neighbour symmetry, remove/reinforce,
  durable round-trip), cache eviction and checkpoint old/new format loads.
- `#[ignore]` scale benchmark: exact top-k over 334 k × 1024 rows.
- Live: operator restarts on the new binary and compares
  `vmmap --summary <pid>` bytes allocated against the 12.4 GB baseline.
