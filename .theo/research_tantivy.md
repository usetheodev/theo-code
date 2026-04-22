## Research for: Phase 4 — Tantivy Persistent Indexing for Transcripts

### Current State (from internal codebase reading)

**Subsystem:** `MemoryTantivyIndex` in `crates/theo-engine-retrieval/src/memory_tantivy.rs`

**Key files:**
- `/home/paulo/Projetos/usetheo/theo-code/crates/theo-engine-retrieval/src/memory_tantivy.rs` — 277 LOC, RAM-only
- `/home/paulo/Projetos/usetheo/theo-code/crates/theo-engine-retrieval/src/tantivy_search.rs` — 938 LOC, FileTantivyIndex (also RAM-only)
- `/home/paulo/Projetos/usetheo/theo-code/crates/theo-infra-memory/src/retrieval/mod.rs` — TantivyMemoryBackend wiring

**Current architecture:**
- `MemoryTantivyIndex::build(docs: &[MemoryDoc])` creates index with `Index::create_in_ram(schema)` (line 74)
- Schema: 3 fields — `slug` (STRING+STORED), `source_type` (STRING+STORED), `body` (TEXT+STORED)
- Custom tokenizer: `SimpleTokenizer + LowerCaser`, registered as `"memory_simple"`
- Writer heap: 15MB (`index.writer(15_000_000)`)
- `commit()` called once after batch insert — no incremental writes
- `search()` opens fresh `reader()` per call — no reader cache
- Feature-gated on `tantivy-backend` via `#[cfg(feature = "tantivy-backend")] mod inner`
- No persistence: RAM index is lost on process exit
- No `session_id`, `turn_index`, or `timestamp_unix` fields

**Known gaps:**
- No disk persistence (AC-4.1 requires `MmapDirectory`)
- No transcript-specific fields (AC-4.2)
- No incremental indexing with hash-based skip (AC-4.3, AC-4.4)
- No `get_by_hash` or `get_session_messages` methods
- `IndexWriter` is created inside `build()` and dropped — no singleton writer pattern

---

### Reference 1: fff.nvim — Rust LMDB Persistence Patterns

**Files read:**
- `/home/paulo/Projetos/usetheo/theo-code/referencias/fff.nvim/crates/fff-core/src/frecency.rs` (577 LOC)
- `/home/paulo/Projetos/usetheo/theo-code/referencias/fff.nvim/crates/fff-core/src/lib.rs` (157 LOC)

**Patterns extracted:**

**Pattern 1: Open-or-create with directory guard**

```rust
// frecency.rs lines 66–108
pub fn new(db_path: impl AsRef<Path>, use_unsafe_no_lock: bool) -> Result<Self> {
    let db_path = db_path.as_ref();
    fs::create_dir_all(db_path).map_err(Error::CreateDir)?;  // <-- guard before open

    let env = unsafe {
        let mut opts = EnvOpenOptions::new();
        opts.map_size(24 * 1024 * 1024); // 24 MiB
        opts.open(db_path).map_err(Error::EnvOpen)?
    };

    // Try read-only first — avoids blocking write lock if DB exists
    let rtxn = env.read_txn().map_err(Error::DbStartReadTxn)?;
    let maybe_db = env.open_database(&rtxn, None).map_err(Error::DbOpen)?;
    drop(rtxn);

    let db = match maybe_db {
        Some(db) => db,                     // exists: no write lock needed
        None => {
            let mut wtxn = env.write_txn().map_err(Error::DbStartWriteTxn)?;
            let db = env.create_database(&mut wtxn, None).map_err(Error::DbCreate)?;
            wtxn.commit().map_err(Error::DbCommit)?;
            db
        }
    };
    Ok(FrecencyTracker { db, env: env.clone() })
}
```

Rust adaptation for `MemoryTantivyIndex`:
```rust
pub fn open_or_create(index_dir: &Path) -> Result<Self, tantivy::TantivyError> {
    std::fs::create_dir_all(index_dir)          // same guard pattern
        .map_err(|e| TantivyError::IoError(e.into()))?;
    let dir = MmapDirectory::open(index_dir)?;  // errors if not a dir
    let schema = Self::build_schema();
    let index = Index::open_or_create(dir, schema.clone())?;
    // ... register tokenizer
    Ok(Self { index, schema, ... })
}
```

**Pattern 2: Hash key for change detection (blake3)**

```rust
// frecency.rs lines 326–333
fn path_to_hash_bytes(path: &Path) -> Result<[u8; 32]> {
    let Some(key) = path.to_str() else {
        return Err(Error::InvalidPath(path.to_path_buf()));
    };
    Ok(*blake3::hash(key.as_bytes()).as_bytes())
}
```

This is used as the key in the LMDB database — the content (path string) is hashed before storage. The hash acts as the change-detection mechanism: if the path hasn't changed, the key is the same and the entry is found. For transcripts, `blake3::hash(session_id + last_N_messages)` gives an idempotency key.

**Pattern 3: Write transaction then commit — batch writes only**

```rust
// frecency.rs lines 296–309 (purge_stale_entries)
let mut wtxn = self.env.write_txn().map_err(Error::DbStartWriteTxn)?;
for key in &to_delete {
    self.db.delete(&mut wtxn, key).map_err(Error::DbWrite)?;
}
for (key, accesses) in &to_update {
    self.db.put(&mut wtxn, key, accesses).map_err(Error::DbWrite)?;
}
wtxn.commit().map_err(Error::DbCommit)?;  // one commit after all writes
```

Tantivy equivalent: add all documents with `writer.add_document()` in loop, then single `writer.commit()`. Never commit per-document.

**Pattern 4: GC in background thread, not main path**

```rust
// frecency.rs lines 121–128
pub fn spawn_gc(shared: SharedFrecency, db_path: String, use_unsafe_no_lock: bool)
    -> Result<std::thread::JoinHandle<()>> {
    Ok(std::thread::Builder::new()
        .name("fff-frecency-gc".into())
        .spawn(move || Self::run_frecency_gc(shared, db_path, use_unsafe_no_lock))?)
}
```

For Tantivy: writer.commit() + segment merging should happen in a background task (e.g. `tokio::spawn`), not blocking the session end path.

**Pattern 5: Corruption recovery — delete and recreate**

```rust
// frecency.rs lines 215–227 (compaction)
*guard = None;  // drop old tracker

let lock_path = PathBuf::from(&db_path).join("lock.mdb");
let _ = fs::remove_file(&data_path);
let _ = fs::remove_file(&lock_path);

// Recreate from scratch — write back all clean entries
let tracker = match FrecencyTracker::new(&db_path, use_unsafe_no_lock) {
    Ok(t) => t,
    Err(e) => {
        tracing::error!("Compaction reopen failed, frecency disabled: {e}");
        return;
    }
};
```

Tantivy disk index corruption recovery: detect via `Index::open()` returning `Err`, rename corrupt dir to `.corrupt-{timestamp}`, create fresh index, log event. Never panic.

---

### Reference 2: qmd — TypeScript Hash-Based Incremental Indexing

**Files read:**
- `/home/paulo/Projetos/usetheo/theo-code/referencias/qmd/src/store.ts` (lines 750–830, 1015–1060, 1220–1283, 2025–2033)

**Patterns extracted:**

**Pattern 6: Content-addressable storage (hash as primary key)**

```typescript
// store.ts lines 756–762
CREATE TABLE IF NOT EXISTS content (
  hash TEXT PRIMARY KEY,    // SHA-256 of file content
  doc TEXT NOT NULL,
  created_at TEXT NOT NULL
)
```

The content table is keyed on the SHA-256 hash of the content. Documents table maps `(collection, path)` → hash. Change detection is O(1): compare `existing.hash === new_hash`.

Rust adaptation for transcripts: store `session_id + transcript_hash` as a U64 field (hashed) or a separate metadata sidecar file. The simplest Tantivy approach: add a `content_hash` field (STRING, STORED) to the schema and query for `session_id = X` to retrieve the stored hash before re-indexing.

**Pattern 7: Hash-based skip on re-index**

```typescript
// store.ts lines 1237–1264
const hash = await hashContent(content);  // SHA-256
const existing = findOrMigrateLegacyDocument(db, collectionName, path);

if (existing) {
  if (existing.hash === hash) {
    unchanged++;  // Skip — content identical
  } else {
    insertContent(db, hash, content, now);
    updateDocument(db, existing.id, title, hash, modifiedAt);
    updated++;
  }
} else {
  insertContent(db, hash, content, now);
  insertDocument(db, collectionName, path, title, hash, ...);
  indexed++;
}
```

Rust adaptation:
```rust
pub fn contains_session_with_hash(&self, session_id: &str, hash: &str) -> bool {
    let reader = self.index.reader().ok()?;  // returns false on error
    let searcher = reader.searcher();
    let q = BooleanQuery::new(vec![
        (Occur::Must, term_query(self.f_session_id, session_id)),
        (Occur::Must, term_query(self.f_content_hash, hash)),
    ]);
    searcher.search(&q, &TopDocs::with_limit(1)).ok()
        .map(|hits| !hits.is_empty())
        .unwrap_or(false)
}
```

**Pattern 8: SHA-256 as async hash function**

```typescript
// store.ts lines 2029–2033
export async function hashContent(content: string): Promise<string> {
  const hash = createHash("sha256");
  hash.update(content);
  return hash.digest("hex");
}
```

Rust adaptation using `sha2` crate (already in workspace):
```rust
use sha2::{Digest, Sha256};

pub fn compute_transcript_hash(messages: &[Message]) -> String {
    let mut hasher = Sha256::new();
    for msg in messages {
        hasher.update(msg.role.as_str().as_bytes());
        hasher.update(b":");
        hasher.update(msg.content.as_deref().unwrap_or("").as_bytes());
        hasher.update(b"\n");
    }
    hex::encode(hasher.finalize())
}
```

**Pattern 9: Config hash skip — avoid sync when unchanged**

```typescript
// store.ts lines 1019–1026
const configJson = JSON.stringify(config);
const hash = createHash('sha256').update(configJson).digest('hex');

const existingHash = db.prepare(`SELECT value FROM store_config WHERE key = 'config_hash'`).get();
if (existingHash != null && existingHash.value === hash) {
  return; // Config unchanged, skip sync
}
```

The pattern: persist the hash of the config/content alongside the data. On next run, compare hashes first — if equal, skip expensive work. For Tantivy transcripts: a small sidecar JSON or a dedicated `session_meta` field in the same index.

---

### Reference 3: Tantivy 0.22.1 Source (disk persistence API)

**Files read:**
- `/home/paulo/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/tantivy-0.22.1/src/directory/mmap_directory.rs` (lines 196–268)

**Pattern 10: MmapDirectory::open contract**

```rust
// mmap_directory.rs lines 230–268
/// Opens a MmapDirectory in a directory.
/// Returns error if directory_path does not exist or is not a directory.
pub fn open(directory_path: impl AsRef<Path>) -> Result<MmapDirectory, OpenDirectoryError> {
    Self::open_impl_to_avoid_monomorphization(directory_path.as_ref())
}

fn open_impl_to_avoid_monomorphization(directory_path: &Path)
    -> Result<MmapDirectory, OpenDirectoryError>
{
    if !directory_path.exists() {
        return Err(OpenDirectoryError::DoesNotExist(PathBuf::from(directory_path)));
    }
    let canonical_path = directory_path.canonicalize()?;
    if !canonical_path.is_dir() {
        return Err(OpenDirectoryError::NotADirectory(PathBuf::from(directory_path)));
    }
    Ok(MmapDirectory::new(canonical_path, None))
}
```

**Critical implication:** `MmapDirectory::open` requires the directory to already exist. Always call `std::fs::create_dir_all(path)` before `MmapDirectory::open(path)`. This matches the fff.nvim Pattern 1 (directory guard before open).

**Pattern 11: `Index::open_or_create` — schema must match**

```rust
// Index::open_or_create signature from tantivy docs:
pub fn open_or_create<Dir: Into<Box<dyn Directory>>>(
    dir: Dir,
    schema: Schema,
) -> Result<Index, TantivyError>
```

If the index already exists on disk, `open_or_create` validates the schema. If schema fields changed between runs, it returns `TantivyError::SchemaError`. The migration strategy: on `SchemaError`, wipe the index directory and recreate (same as fff.nvim Pattern 5 for corruption).

---

### Delta Analysis

| Aspect | Current (theo-code) | SOTA (references) | Gap |
|---|---|---|---|
| Index storage | `Index::create_in_ram` | `MmapDirectory::open` + `Index::open_or_create` | Full: loses all data on process exit |
| Writer lifecycle | Created and dropped in `build()` | Singleton writer held by `MemoryTantivyIndex` struct | Partial: no issue for batch builds, problem for incremental |
| Commit strategy | Single `commit()` after batch insert | Single `commit()` after batch (correct), periodic background commit | Correct pattern, needs incremental path |
| Change detection | None | SHA-256 hash of content, compared before re-index | Full: will re-index same session every restart |
| Hash storage | N/A | Tantivy field or sidecar JSON | Full: no mechanism to store or query hash |
| Schema fields | `slug`, `source_type`, `body` | Need `session_id`, `turn_index`, `timestamp_unix`, `role`, `content_hash` | Full: transcript-specific fields missing |
| `session_id` field type | N/A | STRING (exact match, not tokenized) | N/A: must be STRING not TEXT |
| Corruption recovery | None | Rename corrupt dir, recreate, log event | Full: a corrupt index silently fails |
| Reader lifecycle | Fresh reader per call | Cached `IndexReader` (reload on commit) | Partial: unnecessary overhead |
| Incremental write | None (batch-only `build()`) | `add_transcript()` → `commit()` | Full: no way to add docs after initial build |

---

### Adaptation Notes

**TS pattern → idiomatic Rust:**

1. `createHash('sha256').update(content).digest('hex')` → `sha2::Sha256::digest(content.as_bytes())` with `hex::encode()`.

2. `existing.hash === hash` skip in qmd → `contains_session_with_hash(&session_id, &hash)` querying Tantivy for the `content_hash` field. Alternatively, a separate `HashMap<String, String>` sidecar file (`session_id → hash`) in the index directory avoids Tantivy query overhead for the skip check.

3. qmd's `store_config` table for config_hash → a `session_hashes.json` file in the Tantivy index directory. Written after every successful `commit()`. On startup, loaded into a `HashMap<String, String>`.

4. fff.nvim's `spawn_gc` background thread → `tokio::task::spawn_blocking` for the `writer.commit()` call (Tantivy writer is sync/blocking, not async).

5. fff.nvim's `read_txn`-first before write lock → Tantivy equivalent: always open index with `Index::open_or_create`, never `Index::create_in_dir` (which would wipe existing data).

---

### Implementation Plan for Phase 4

**Task 4.1 — Disk persistence (critical path)**

File: `crates/theo-engine-retrieval/src/memory_tantivy.rs`

Change: replace `Index::create_in_ram` with `MmapDirectory` + `open_or_create`.

```rust
use tantivy::directory::MmapDirectory;

impl MemoryTantivyIndex {
    pub fn open_or_create(index_dir: &Path) -> Result<Self, tantivy::TantivyError> {
        // Pattern 1 (fff.nvim): guard before open
        std::fs::create_dir_all(index_dir)
            .map_err(|e| tantivy::TantivyError::IoError(Arc::new(e)))?;
        // Pattern 10 (tantivy): open requires dir to exist
        let dir = MmapDirectory::open(index_dir)
            .map_err(|e| tantivy::TantivyError::IoError(Arc::new(std::io::Error::other(e.to_string()))))?;
        let schema = Self::build_schema();
        // If schema changed, this returns SchemaError → caller should wipe and retry
        let index = Index::open_or_create(dir, schema.clone())?;
        // Register tokenizer on every open (tokenizers are not persisted)
        let analyzer = TextAnalyzer::builder(SimpleTokenizer::default())
            .filter(LowerCaser)
            .build();
        index.tokenizers().register(MEMORY_TOKENIZER, analyzer);
        // Reuse existing reader (reload policy: on_commit)
        let reader = index.reader_builder()
            .reload_policy(tantivy::ReloadPolicy::OnCommitWithDelay)
            .try_into()?;
        Ok(Self { index, reader, schema, f_slug, f_source_type, f_body,
                  f_session_id, f_turn_index, f_timestamp_unix, f_role, f_content_hash })
    }
}
```

Approximate lines: ~40. Addresses AC-4.1, AC-4.6.

**Task 4.2 — Schema extension for transcripts**

Add 4 fields to `build_schema()`:
- `session_id`: STRING (not TEXT) — exact match needed, no tokenization.
- `turn_index`: u64 fast field — for sorting within session.
- `timestamp_unix`: u64 fast field — for recency-weighted scoring.
- `role`: STRING — "user" | "assistant" | "tool", exact match filter.
- `content_hash`: STRING, STORED — the SHA-256 of `session_id + all messages` for skip detection.

STRING vs TEXT for `session_id`: use `schema_builder.add_text_field("session_id", STORED | STRING)`. STRING means "indexed as-is, no tokenization" — correct for UUIDs and session IDs. TEXT would tokenize "session-abc-123" into ["session", "abc", "123"], breaking exact lookup.

```rust
// In build_schema():
let f_session_id = schema_builder.add_text_field("session_id", STORED | STRING);
let f_turn_index = schema_builder.add_u64_field("turn_index", STORED | FAST);
let f_timestamp_unix = schema_builder.add_u64_field("timestamp_unix", STORED | FAST);
let f_role = schema_builder.add_text_field("role", STRING);
let f_content_hash = schema_builder.add_text_field("content_hash", STORED | STRING);
```

Approximate lines: ~20. Addresses AC-4.2.

**Task 4.3 — Incremental writer + `add_transcript()`**

Hold `IndexWriter` as `Option<IndexWriter>` in the struct. Acquire lazily. Call `commit()` after each session batch (not per-document).

```rust
pub struct MemoryTantivyIndex {
    index: Index,
    reader: tantivy::IndexReader,
    writer: Mutex<Option<IndexWriter>>,  // Mutex because IndexWriter is not Send+Sync
    // ... fields
}

impl MemoryTantivyIndex {
    pub fn add_transcript(&self, doc: TranscriptDoc) -> Result<(), tantivy::TantivyError> {
        let mut guard = self.writer.lock().unwrap();
        let writer = guard.get_or_insert_with(|| {
            self.index.writer(15_000_000).expect("writer alloc")
        });
        writer.add_document(doc!(
            self.f_session_id => doc.session_id.as_str(),
            self.f_turn_index => doc.turn_index,
            self.f_timestamp_unix => doc.timestamp_unix,
            self.f_role => doc.role.as_str(),
            self.f_body => doc.body.as_str(),
            self.f_source_type => "transcript",
        ))?;
        Ok(())
    }

    pub fn commit(&self) -> Result<(), tantivy::TantivyError> {
        let mut guard = self.writer.lock().unwrap();
        if let Some(writer) = guard.as_mut() {
            writer.commit()?;
        }
        Ok(())
    }
}
```

Approximate lines: ~35. Addresses AC-4.3.

**Task 4.4 — Hash-based skip detection (Pattern 7 from qmd)**

Use a sidecar file `{index_dir}/session_hashes.json` — simpler than querying Tantivy for the hash, avoids searcher overhead for a simple lookup.

```rust
// Sidecar: HashMap<session_id, content_hash>
fn load_session_hashes(index_dir: &Path) -> HashMap<String, String> {
    let p = index_dir.join("session_hashes.json");
    std::fs::read_to_string(&p)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_session_hashes(index_dir: &Path, map: &HashMap<String, String>) {
    let p = index_dir.join("session_hashes.json");
    if let Ok(json) = serde_json::to_string(map) {
        let _ = std::fs::write(&p, json);  // best-effort, not critical
    }
}

pub fn contains_session_with_hash(&self, session_id: &str, hash: &str) -> bool {
    let hashes = load_session_hashes(&self.index_dir);
    hashes.get(session_id).map(|h| h == hash).unwrap_or(false)
}

pub fn record_session_hash(&self, session_id: &str, hash: &str) {
    let mut hashes = load_session_hashes(&self.index_dir);
    hashes.insert(session_id.to_string(), hash.to_string());
    save_session_hashes(&self.index_dir, &hashes);
}
```

Approximate lines: ~35. Addresses AC-4.4.

**Task 4.5 — Corruption recovery (Pattern 5 from fff.nvim)**

```rust
pub fn open_or_create_with_recovery(index_dir: &Path) -> Result<Self, tantivy::TantivyError> {
    match Self::open_or_create(index_dir) {
        Ok(idx) => Ok(idx),
        Err(tantivy::TantivyError::SchemaError(_)) | Err(_) => {
            // Rename corrupt dir, start fresh
            let corrupt = index_dir.with_extension(
                format!("corrupt-{}", std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default().as_secs())
            );
            let _ = std::fs::rename(index_dir, &corrupt);
            tracing::warn!(corrupt = ?corrupt, "Tantivy index corrupt, recreating");
            Self::open_or_create(index_dir)
        }
    }
}
```

Approximate lines: ~20. Addresses PLAN_AUTO_EVOLUTION_SOTA risk "Tantivy persistence corrompe em crash".

**Task 4.6 — Recency-weighted scoring for `search_transcripts`**

The plan's AC-4.7 requires BM25 cross-session retrieval. To add recency weighting, apply a score multiplier after TopDocs collection:

```rust
pub fn search_transcripts(&self, query: &str, limit: usize)
    -> Result<Vec<TranscriptHit>, tantivy::TantivyError>
{
    // ... BooleanQuery with source_type="transcript" Must filter + body tokens Should
    let top = searcher.search(&bool_query, &TopDocs::with_limit(limit * 2))?;

    let now_unix = SystemTime::now()
        .duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();

    let mut hits = Vec::with_capacity(top.len());
    for (bm25_score, addr) in top {
        let doc = searcher.doc(addr)?;
        let ts = doc.get_first(self.f_timestamp_unix)
            .and_then(|v| v.as_u64()).unwrap_or(0);
        // Recency decay: e^(-lambda * days_ago), lambda = 0.0693 (10-day half-life)
        // Same constant used in fff.nvim frecency.rs line 16
        let days_ago = (now_unix.saturating_sub(ts) as f64) / 86400.0;
        let recency = (-0.0693 * days_ago).exp();
        let final_score = bm25_score as f64 * (0.7 + 0.3 * recency);  // 70% BM25, 30% recency
        // ... collect hit
        hits.push(TranscriptHit { ..., score: final_score });
    }
    hits.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap());
    hits.truncate(limit);
    Ok(hits)
}
```

The 0.0693 decay constant (ln(2)/10 for 10-day half-life) is directly borrowed from fff.nvim `frecency.rs` line 16. Approximate lines: ~30.

---

### Summary of Key Technical Decisions

1. **`MmapDirectory::open` requires pre-existing directory**: always call `create_dir_all` first. This is not obvious from the Tantivy docs but explicit in the source (mmap_directory.rs line 242).

2. **Writer is not thread-safe**: `IndexWriter` must be held behind `Mutex<Option<IndexWriter>>`. The `Option` allows lazy initialization.

3. **Reader should be cached**: opening a fresh `IndexReader` on every search is unnecessary. Store it in the struct, use `ReloadPolicy::OnCommitWithDelay` to pick up new commits automatically.

4. **Hash skip avoids re-indexing**: store `session_id → sha256` in a sidecar file, not inside Tantivy itself (avoids query round-trip for a simple equality check).

5. **`session_id` must be STRING, not TEXT**: TEXT would tokenize UUIDs/slugs, breaking exact match. This is a schema correctness issue, not a performance one.

6. **Commit batch, not per-document**: qmd and Tantivy's own patterns both confirm: add N documents, then commit once. Per-document commits would cause N segment flushes.

7. **Recency decay reuses fff.nvim's 0.0693 constant**: 10-day half-life is appropriate for AI session transcripts (same reasoning as fff.nvim's AI mode `AI_MAX_HISTORY_DAYS = 7.0`).

8. **Tokenizer registration is not persisted**: must re-register `MEMORY_TOKENIZER` on every `Index::open_or_create`, even when opening an existing index. Tantivy tokenizers are in-memory only.

