use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::time::SystemTime;

use camino::{Utf8Path, Utf8PathBuf};
use rusqlite::{params, Connection, OptionalExtension, Row};

use argyph_fs::{Blake3Hash, FileEntry, Language};
use argyph_graph::edge::{Confidence, Edge, EdgeKind};
use argyph_graph::graph::SymbolOutline;
use argyph_graph::selector::SymbolSelector;
use argyph_parse::types::{ByteRange, Chunk, ChunkId, ChunkKind, Symbol, SymbolId, SymbolKind};

use crate::error::Result;
use crate::migration;
use crate::search::{HitSource, HybridSearchResult, SearchFilter, SearchHit, VectorEntry};
use crate::MemoryEntry;
use crate::Store;
use crate::StructuralNodeRecord;

pub struct SqliteStore {
    pub(crate) conn: Mutex<Connection>,
}

impl SqliteStore {
    pub fn open_at(root: &Utf8Path) -> Result<Self> {
        let dir = root.join(".argyph");
        std::fs::create_dir_all(dir.as_std_path())?;

        let db_path = dir.join("meta.sqlite");
        let conn = Connection::open(db_path.as_std_path())?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;

        migration::run(&conn)?;

        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    pub fn open_in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        conn.execute_batch("PRAGMA foreign_keys=ON;")?;
        migration::run(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }
}

#[async_trait::async_trait]
impl Store for SqliteStore {
    #[allow(clippy::expect_used)]
    async fn upsert_files(&self, files: &[FileEntry]) -> Result<()> {
        let mut conn = self.conn.lock().expect("mutex poisoned");
        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare_cached(
                "INSERT INTO files (path, hash, language, size, last_seen)
                 VALUES (?1, ?2, ?3, ?4, datetime('now'))
                 ON CONFLICT(path) DO UPDATE SET
                     hash     = excluded.hash,
                     language = excluded.language,
                     size     = excluded.size,
                     last_seen = datetime('now')",
            )?;

            for entry in files {
                let lang_val = entry.language.map(language_to_str);
                stmt.execute(params![
                    entry.path.as_str(),
                    entry.hash.as_bytes().as_slice(),
                    lang_val,
                    entry.size as i64,
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    #[allow(clippy::expect_used)]
    async fn get_file(&self, path: &Utf8Path) -> Result<Option<FileEntry>> {
        let conn = self.conn.lock().expect("mutex poisoned");
        let mut stmt =
            conn.prepare_cached("SELECT path, hash, language, size FROM files WHERE path = ?1")?;
        let mut rows = stmt.query_map(params![path.as_str()], |row| {
            let path_str: String = row.get(0)?;
            let hash_blob: Vec<u8> = row.get(1)?;
            let language: Option<String> = row.get(2)?;
            let size: i64 = row.get(3)?;
            Ok((path_str, hash_blob, language, size))
        })?;

        match rows.next() {
            Some(Ok((path_str, hash_blob, lang_str, size))) => {
                let path = Utf8PathBuf::from(&path_str);
                let hash = blob_to_hash(&hash_blob);
                let language = lang_str.and_then(|s| str_to_language(&s));
                Ok(Some(FileEntry {
                    path,
                    hash,
                    language,
                    size: size as u64,
                    modified: SystemTime::UNIX_EPOCH,
                }))
            }
            Some(Err(e)) => Err(e.into()),
            None => Ok(None),
        }
    }

    #[allow(clippy::expect_used)]
    async fn list_files(&self) -> Result<Vec<FileEntry>> {
        let conn = self.conn.lock().expect("mutex poisoned");
        let mut stmt =
            conn.prepare_cached("SELECT path, hash, language, size FROM files ORDER BY path")?;
        let rows = stmt.query_map([], |row| {
            let path_str: String = row.get(0)?;
            let hash_blob: Vec<u8> = row.get(1)?;
            let language: Option<String> = row.get(2)?;
            let size: i64 = row.get(3)?;
            Ok((path_str, hash_blob, language, size))
        })?;

        let mut entries = Vec::new();
        for row in rows {
            let (path_str, hash_blob, lang_str, size) = row?;
            let path = Utf8PathBuf::from(&path_str);
            let hash = blob_to_hash(&hash_blob);
            let language = lang_str.and_then(|s| str_to_language(&s));
            entries.push(FileEntry {
                path,
                hash,
                language,
                size: size as u64,
                modified: SystemTime::UNIX_EPOCH,
            });
        }
        Ok(entries)
    }

    #[allow(clippy::expect_used)]
    async fn delete_file(&self, path: &Utf8Path) -> Result<()> {
        let conn = self.conn.lock().expect("mutex poisoned");
        conn.execute("DELETE FROM files WHERE path = ?1", params![path.as_str()])?;
        Ok(())
    }

    #[allow(clippy::expect_used)]
    async fn get_file_id(&self, path: &Utf8Path) -> Result<Option<i64>> {
        let conn = self.conn.lock().expect("mutex poisoned");
        let mut stmt = conn.prepare_cached("SELECT rowid FROM files WHERE path = ?1")?;
        let result = stmt
            .query_row(params![path.as_str()], |r| r.get(0))
            .optional()?;
        Ok(result)
    }

    #[allow(clippy::expect_used)]
    async fn get_file_by_id(&self, id: i64) -> Result<Option<FileEntry>> {
        let conn = self.conn.lock().expect("mutex poisoned");
        let mut stmt =
            conn.prepare_cached("SELECT path, hash, language, size FROM files WHERE rowid = ?1")?;
        let result = stmt
            .query_row(params![id], |row| {
                let path_str: String = row.get(0)?;
                let hash_blob: Vec<u8> = row.get(1)?;
                let language: Option<String> = row.get(2)?;
                let size: i64 = row.get(3)?;
                Ok((path_str, hash_blob, language, size))
            })
            .optional()?;
        match result {
            Some((path_str, hash_blob, lang_str, size)) => {
                let path = Utf8PathBuf::from(&path_str);
                let hash = blob_to_hash(&hash_blob);
                let language = lang_str.and_then(|s| str_to_language(&s));
                Ok(Some(FileEntry {
                    path,
                    hash,
                    language,
                    size: size as u64,
                    modified: SystemTime::UNIX_EPOCH,
                }))
            }
            None => Ok(None),
        }
    }

    #[allow(clippy::expect_used)]
    async fn upsert_symbols(&self, symbols: &[Symbol]) -> Result<()> {
        let mut conn = self.conn.lock().expect("mutex poisoned");
        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare_cached(
                "INSERT OR REPLACE INTO symbols
                    (id, name, kind, file, range_start, range_end, signature, parent_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            )?;

            for sym in symbols {
                stmt.execute(params![
                    sym.id.as_str(),
                    sym.name.as_str(),
                    symbol_kind_to_str(sym.kind),
                    sym.file.as_str(),
                    sym.range.start as i64,
                    sym.range.end as i64,
                    sym.signature.as_deref(),
                    sym.parent.as_ref().map(|p| p.as_str()),
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    #[allow(clippy::expect_used)]
    async fn upsert_chunks(&self, chunks: &[Chunk]) -> Result<()> {
        let mut conn = self.conn.lock().expect("mutex poisoned");
        let tx = conn.transaction()?;
        {
            let mut stmt = tx.prepare_cached(
                "INSERT OR REPLACE INTO chunks
                    (id, file, range_start, range_end, text, kind, language)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            )?;

            for ch in chunks {
                let cid = hex_encode(ch.id.as_bytes());
                stmt.execute(params![
                    cid,
                    ch.file.as_str(),
                    ch.range.start as i64,
                    ch.range.end as i64,
                    ch.text.as_str(),
                    chunk_kind_to_str(ch.kind),
                    language_to_str(ch.language),
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    #[allow(clippy::expect_used)]
    async fn upsert_edges(&self, edges: &[Edge]) -> Result<()> {
        if edges.is_empty() {
            return Ok(());
        }

        let file_prefixes: HashSet<String> = edges
            .iter()
            .filter_map(|e| e.from.as_str().split("::").next())
            .map(String::from)
            .collect();

        let mut conn = self.conn.lock().expect("mutex poisoned");
        let tx = conn.transaction()?;
        {
            let mut del_stmt =
                tx.prepare_cached("DELETE FROM edges WHERE from_id LIKE ?1 || '::%'")?;
            for prefix in &file_prefixes {
                del_stmt.execute(params![prefix.as_str()])?;
            }

            let mut ins_stmt = tx.prepare_cached(
                "INSERT OR IGNORE INTO edges (from_id, to_id, kind, confidence)
                 VALUES (?1, ?2, ?3, ?4)",
            )?;

            for edge in edges {
                ins_stmt.execute(params![
                    edge.from.as_str(),
                    edge.to.as_str(),
                    edge_kind_to_str(edge.kind),
                    confidence_to_str(edge.confidence),
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    #[allow(clippy::expect_used)]
    async fn replace_edges_for_file(&self, file: &Utf8Path, edges: &[Edge]) -> Result<()> {
        let prefix = file.as_str();
        let mut conn = self.conn.lock().expect("mutex poisoned");
        let tx = conn.transaction()?;
        {
            tx.execute(
                "DELETE FROM edges WHERE from_id LIKE ?1 || '::%' OR to_id LIKE ?1 || '::%'",
                params![prefix],
            )?;

            if !edges.is_empty() {
                let mut ins_stmt = tx.prepare_cached(
                    "INSERT OR IGNORE INTO edges (from_id, to_id, kind, confidence)
                     VALUES (?1, ?2, ?3, ?4)",
                )?;
                for edge in edges {
                    ins_stmt.execute(params![
                        edge.from.as_str(),
                        edge.to.as_str(),
                        edge_kind_to_str(edge.kind),
                        confidence_to_str(edge.confidence),
                    ])?;
                }
            }
        }
        tx.commit()?;
        Ok(())
    }

    #[allow(clippy::expect_used)]
    async fn find_symbol(&self, name: &str, file: Option<&Utf8Path>) -> Result<Vec<Symbol>> {
        let conn = self.conn.lock().expect("mutex poisoned");

        let (sql, params_vec): (String, Vec<Box<dyn rusqlite::types::ToSql>>) =
            if let Some(f) = file {
                (
                    "SELECT id, name, kind, file, range_start, range_end, signature, parent_id
                     FROM symbols WHERE name = ?1 AND file = ?2"
                        .to_string(),
                    vec![Box::new(name.to_string()), Box::new(f.as_str().to_string())],
                )
            } else {
                (
                    "SELECT id, name, kind, file, range_start, range_end, signature, parent_id
                     FROM symbols WHERE name = ?1"
                        .to_string(),
                    vec![Box::new(name.to_string())],
                )
            };

        let params_refs: Vec<&dyn rusqlite::types::ToSql> =
            params_vec.iter().map(|p| p.as_ref()).collect();
        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params_refs.as_slice(), row_to_symbol)?;

        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    #[allow(clippy::expect_used)]
    async fn find_references(&self, sel: &SymbolSelector) -> Result<Vec<Edge>> {
        let conn = self.conn.lock().expect("mutex poisoned");
        query_edges_incoming(&conn, sel, EdgeKind::References)
    }

    #[allow(clippy::expect_used)]
    async fn neighbors(&self, sel: &SymbolSelector, kind: EdgeKind) -> Result<Vec<Edge>> {
        let conn = self.conn.lock().expect("mutex poisoned");
        query_edges_outgoing(&conn, sel, kind)
    }

    #[allow(clippy::expect_used)]
    async fn get_callers(&self, sel: &SymbolSelector) -> Result<Vec<Edge>> {
        let conn = self.conn.lock().expect("mutex poisoned");
        query_edges_incoming(&conn, sel, EdgeKind::Calls)
    }

    #[allow(clippy::expect_used)]
    async fn get_callees(&self, sel: &SymbolSelector) -> Result<Vec<Edge>> {
        let conn = self.conn.lock().expect("mutex poisoned");
        query_edges_outgoing(&conn, sel, EdgeKind::Calls)
    }

    #[allow(clippy::expect_used)]
    async fn get_imports(&self, file: &Utf8Path) -> Result<Vec<Edge>> {
        let conn = self.conn.lock().expect("mutex poisoned");
        let prefix = format!("{}::%", file.as_str());
        let mut stmt = conn.prepare_cached(
            "SELECT from_id, to_id, kind, confidence FROM edges
             WHERE kind = 'Imports' AND from_id LIKE ?1",
        )?;
        let rows = stmt.query_map(params![prefix.as_str()], row_to_edge)?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    #[allow(clippy::expect_used)]
    async fn get_symbol_outline(&self, file: &Utf8Path) -> Result<Vec<SymbolOutline>> {
        let conn = self.conn.lock().expect("mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT id, name, kind, signature, parent_id, range_start, range_end
             FROM symbols WHERE file = ?1 ORDER BY name",
        )?;
        let rows = stmt.query_map(params![file.as_str()], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, Option<String>>(4)?,
                row.get::<_, u64>(5)?,
                row.get::<_, u64>(6)?,
            ))
        })?;

        let all: Vec<(
            String,
            String,
            String,
            Option<String>,
            Option<String>,
            u64,
            u64,
        )> = rows.filter_map(|r| r.ok()).collect();

        let file_ids: HashSet<&str> = all.iter().map(|(id, ..)| id.as_str()).collect();

        let mut map: HashMap<String, SymbolOutline> = HashMap::new();
        for (id, name, kind, sig, _parent_id, start, end) in &all {
            map.entry(id.clone()).or_insert(SymbolOutline {
                name: name.clone(),
                kind: kind.clone(),
                signature: sig.clone(),
                range: (*start, *end),
                children: Vec::new(),
            });
        }

        let mut root_ids: Vec<String> = Vec::new();
        for (id, _name, _kind, _sig, parent_id, _start, _end) in &all {
            let has_in_file_parent = parent_id
                .as_ref()
                .is_some_and(|pid| file_ids.contains(pid.as_str()));
            if !has_in_file_parent {
                root_ids.push(id.clone());
            }
        }

        for (id, _name, _kind, _sig, parent_id, _start, _end) in &all {
            if let Some(pid) = parent_id {
                if file_ids.contains(pid.as_str()) {
                    if let Some(child) = map.remove(id) {
                        if let Some(parent) = map.get_mut(pid) {
                            parent.children.push(child);
                        }
                    }
                }
            }
        }

        let mut roots = Vec::new();
        for root_id in &root_ids {
            if let Some(outline) = map.remove(root_id) {
                roots.push(outline);
            }
        }
        roots.extend(map.into_values());

        Ok(roots)
    }

    #[allow(clippy::expect_used)]
    async fn upsert_vectors(&self, vectors: &[VectorEntry]) -> Result<()> {
        if vectors.is_empty() {
            return Ok(());
        }

        let conn = self.conn.lock().expect("mutex poisoned");
        let mut stmt = conn.prepare_cached(
            "INSERT OR REPLACE INTO vectors (chunk_id, vector, model, dimension)
             VALUES (?1, ?2, ?3, ?4)",
        )?;

        for entry in vectors {
            let blob = vector_to_blob(&entry.vector);
            stmt.execute(params![
                entry.chunk_id.as_str(),
                blob,
                entry.model.as_str(),
                entry.dimension as i64,
            ])?;
        }

        Ok(())
    }

    #[allow(clippy::expect_used)]
    async fn search_vectors(
        &self,
        query_vec: &[f32],
        k: usize,
        filter: &SearchFilter,
    ) -> Result<Vec<(String, f32)>> {
        let conn = self.conn.lock().expect("mutex poisoned");

        let (sql, params_vec): (String, Vec<Box<dyn rusqlite::types::ToSql>>) =
            build_vector_search_sql(query_vec.len(), filter);

        let params_refs: Vec<&dyn rusqlite::types::ToSql> =
            params_vec.iter().map(|p| p.as_ref()).collect();

        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params_refs.as_slice(), |row| {
            let chunk_id: String = row.get(0)?;
            let vector_blob: Vec<u8> = row.get(1)?;
            let file: String = row.get(2)?;
            let text: String = row.get(3)?;
            Ok((chunk_id, vector_blob, file, text))
        })?;

        let query_norm_inv = 1.0 / norm(query_vec).max(f32::MIN_POSITIVE);

        let mut results: Vec<_> = rows
            .filter_map(|r| r.ok())
            .filter_map(|(cid, blob, _file, _text)| {
                let stored_vec = blob_to_vector(&blob)?;
                let stored_norm_inv = 1.0 / norm(&stored_vec).max(f32::MIN_POSITIVE);
                let cosine = dot(query_vec, &stored_vec) * query_norm_inv * stored_norm_inv;
                Some((cid, cosine))
            })
            .collect();

        results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        results.truncate(k);

        Ok(results)
    }

    #[allow(clippy::expect_used)]
    async fn search_text_bm25(
        &self,
        query: &str,
        limit: usize,
        filter: &SearchFilter,
    ) -> Result<Vec<(String, f32)>> {
        let conn = self.conn.lock().expect("mutex poisoned");

        let (sql, filter_params): (String, Vec<Box<dyn rusqlite::types::ToSql>>) =
            build_bm25_sql(limit, filter);

        let mut all_params: Vec<Box<dyn rusqlite::types::ToSql>> =
            vec![Box::new(query.to_string())];
        all_params.extend(filter_params);
        let params_refs: Vec<&dyn rusqlite::types::ToSql> =
            all_params.iter().map(|p| p.as_ref()).collect();

        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt.query_map(params_refs.as_slice(), |row| {
            let chunk_id: String = row.get(0)?;
            let score: f64 = row.get(1)?;
            Ok((chunk_id, score as f32))
        })?;

        let mut results = Vec::with_capacity(limit);
        for row in rows {
            match row {
                Ok((id, score)) => results.push((id, score)),
                Err(e) => return Err(e.into()),
            }
        }

        Ok(results)
    }

    #[allow(clippy::expect_used)]
    async fn search_hybrid(
        &self,
        query: &str,
        query_vec: &[f32],
        k: usize,
        filter: &SearchFilter,
    ) -> Result<HybridSearchResult> {
        let bm25_limit = k.max(100);
        let _vec_limit = k.max(100);

        let (bm25_hits, vector_hits, total_embedded, total_chunks) = {
            let conn = self.conn.lock().expect("mutex poisoned");

            let total_chunks: usize = conn
                .query_row("SELECT COUNT(*) FROM chunks", [], |r| r.get::<_, i64>(0))
                .map(|n| n as usize)
                .unwrap_or(0);

            let total_embedded: usize = conn
                .query_row("SELECT COUNT(*) FROM vectors", [], |r| r.get::<_, i64>(0))
                .map(|n| n as usize)
                .unwrap_or(0);

            let (bm25_sql, bm25_filter_params): (String, Vec<Box<dyn rusqlite::types::ToSql>>) =
                build_bm25_sql(bm25_limit, filter);
            let mut bm25_all_params: Vec<Box<dyn rusqlite::types::ToSql>> =
                vec![Box::new(query.to_string())];
            bm25_all_params.extend(bm25_filter_params);
            let bm25_refs: Vec<&dyn rusqlite::types::ToSql> =
                bm25_all_params.iter().map(|p| p.as_ref()).collect();

            let mut bm25_stmt = conn.prepare(&bm25_sql)?;
            let bm25_rows = bm25_stmt.query_map(bm25_refs.as_slice(), |row| {
                let chunk_id: String = row.get(0)?;
                let score: f64 = row.get(1)?;
                Ok((chunk_id, score as f32))
            })?;
            let bm25_hits: Vec<(String, f32)> = bm25_rows.filter_map(|r| r.ok()).collect();

            let (vec_sql, vec_params): (String, Vec<Box<dyn rusqlite::types::ToSql>>) =
                build_vector_search_sql(query_vec.len(), filter);
            let vec_refs: Vec<&dyn rusqlite::types::ToSql> =
                vec_params.iter().map(|p| p.as_ref()).collect();

            let mut vec_stmt = conn.prepare(&vec_sql)?;
            let vec_rows = vec_stmt.query_map(vec_refs.as_slice(), |row| {
                let chunk_id: String = row.get(0)?;
                let vector_blob: Vec<u8> = row.get(1)?;
                Ok((chunk_id, vector_blob))
            })?;

            let query_norm_inv = 1.0 / norm(query_vec).max(f32::MIN_POSITIVE);
            let vector_hits: Vec<(String, f32)> = vec_rows
                .filter_map(|r| r.ok())
                .filter_map(|(cid, blob)| {
                    let stored_vec = blob_to_vector(&blob)?;
                    let stored_norm_inv = 1.0 / norm(&stored_vec).max(f32::MIN_POSITIVE);
                    let cosine = dot(query_vec, &stored_vec) * query_norm_inv * stored_norm_inv;
                    Some((cid, cosine))
                })
                .collect();

            (bm25_hits, vector_hits, total_embedded, total_chunks)
        };

        let fused = crate::search::reciprocal_rank_fusion(&bm25_hits, &vector_hits, k);

        let conn = self.conn.lock().expect("mutex poisoned");
        let mut hits = Vec::with_capacity(fused.len());
        for (chunk_id, score) in &fused {
            let (text, file, byte_start, byte_end) = chunk_info(&conn, chunk_id)?;
            let line_range = text_bytes_to_line_range(&text, byte_start, byte_end);
            let source = if in_both(&bm25_hits, &vector_hits, chunk_id) {
                HitSource::Hybrid
            } else if bm25_hits.iter().any(|(id, _)| id == chunk_id) {
                HitSource::Bm25
            } else {
                HitSource::Vector
            };

            hits.push(SearchHit {
                chunk_id: chunk_id.clone(),
                chunk_text: text,
                file,
                byte_range: (byte_start, byte_end),
                line_range,
                score: *score,
                source,
            });
        }

        crate::rerank::rerank_hits(
            &mut hits,
            &crate::rerank::FocusContext::default(),
            crate::rerank::Weights::default(),
        );

        Ok(HybridSearchResult {
            hits,
            total_embedded,
            total_chunks,
        })
    }

    #[allow(clippy::expect_used)]
    async fn missing_vectors(&self, model: &str) -> Result<Vec<String>> {
        let conn = self.conn.lock().expect("mutex poisoned");
        let mut stmt = conn.prepare(
            "SELECT c.id FROM chunks c
             WHERE c.id NOT IN (SELECT v.chunk_id FROM vectors v WHERE v.model = ?1)",
        )?;
        let rows = stmt.query_map(params![model], |row| row.get::<_, String>(0))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    #[allow(clippy::expect_used)]
    async fn get_chunk_texts(&self, chunk_ids: &[String]) -> Result<Vec<(String, String)>> {
        if chunk_ids.is_empty() {
            return Ok(Vec::new());
        }
        let conn = self.conn.lock().expect("mutex poisoned");
        let placeholders: Vec<String> = (1..=chunk_ids.len()).map(|i| format!("?{i}")).collect();
        let sql = format!(
            "SELECT id, text FROM chunks WHERE id IN ({})",
            placeholders.join(",")
        );
        let mut stmt = conn.prepare(&sql)?;
        let params: Vec<&dyn rusqlite::types::ToSql> = chunk_ids
            .iter()
            .map(|s| s as &dyn rusqlite::types::ToSql)
            .collect();
        let rows = stmt.query_map(params.as_slice(), |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut out = Vec::with_capacity(chunk_ids.len());
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    // ---- Structural nodes ----

    #[allow(clippy::expect_used)]
    async fn upsert_structural_nodes(
        &self,
        file_id: i64,
        nodes: &[StructuralNodeRecord],
    ) -> Result<()> {
        if nodes.is_empty() {
            return Ok(());
        }
        let mut conn = self.conn.lock().expect("mutex poisoned");
        let tx = conn.transaction()?;
        {
            tx.execute(
                "DELETE FROM structural_nodes WHERE file_id = ?1",
                params![file_id],
            )?;
            let mut stmt = tx.prepare_cached(
                "INSERT INTO structural_nodes (id, file_id, kind, label, path_joined, path_json, byte_start, byte_end, line_start, line_end, parent_id, depth)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            )?;
            for n in nodes {
                let path_json = serde_json::to_string(&n.path).unwrap_or_default();
                stmt.execute(params![
                    n.id,
                    n.file_id,
                    n.kind.as_str(),
                    n.label.as_str(),
                    n.path_joined.as_str(),
                    path_json.as_str(),
                    n.byte_range.0 as i64,
                    n.byte_range.1 as i64,
                    n.line_range.0 as i64,
                    n.line_range.1 as i64,
                    n.parent_id,
                    n.depth as i64,
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    #[allow(clippy::expect_used)]
    async fn get_structural_node_by_path(
        &self,
        file_id: Option<i64>,
        path_joined: &str,
    ) -> Result<Option<StructuralNodeRecord>> {
        let conn = self.conn.lock().expect("mutex poisoned");
        let result = if let Some(fid) = file_id {
            let mut stmt = conn.prepare_cached(
                "SELECT id, file_id, kind, label, path_joined, path_json, byte_start, byte_end, line_start, line_end, parent_id, depth
                 FROM structural_nodes WHERE file_id = ?1 AND path_joined = ?2",
            )?;
            stmt.query_row(params![fid, path_joined], row_to_structural_node)
                .optional()?
        } else {
            let mut stmt = conn.prepare_cached(
                "SELECT id, file_id, kind, label, path_joined, path_json, byte_start, byte_end, line_start, line_end, parent_id, depth
                 FROM structural_nodes WHERE path_joined = ?1 ORDER BY file_id, depth LIMIT 1",
            )?;
            stmt.query_row(params![path_joined], row_to_structural_node)
                .optional()?
        };
        Ok(result)
    }

    #[allow(clippy::expect_used)]
    async fn fts_search_structural(
        &self,
        query: &str,
        file_ids: Option<&[i64]>,
        limit: usize,
    ) -> Result<Vec<StructuralNodeRecord>> {
        let conn = self.conn.lock().expect("mutex poisoned");
        let mut stmt = conn.prepare_cached(
            "SELECT sn.id, sn.file_id, sn.kind, sn.label, sn.path_joined, sn.path_json, sn.byte_start, sn.byte_end, sn.line_start, sn.line_end, sn.parent_id, sn.depth
             FROM structural_fts fts
             JOIN structural_nodes sn ON sn.id = fts.rowid
             WHERE structural_fts MATCH ?1
             ORDER BY rank
             LIMIT ?2",
        )?;
        let rows = stmt.query_map(params![query, limit as i64], row_to_structural_node)?;
        let mut out: Vec<StructuralNodeRecord> = Vec::new();
        for rec in rows.flatten() {
            match file_ids {
                Some(ids) if !ids.contains(&rec.file_id) => continue,
                _ => out.push(rec),
            }
        }
        Ok(out)
    }

    #[allow(clippy::expect_used)]
    async fn enclosing_structural_node(
        &self,
        file_id: i64,
        byte_offset: u32,
    ) -> Result<Option<StructuralNodeRecord>> {
        let conn = self.conn.lock().expect("mutex poisoned");
        let mut stmt = conn.prepare_cached(
            "SELECT id, file_id, kind, label, path_joined, path_json, byte_start, byte_end, line_start, line_end, parent_id, depth
             FROM structural_nodes
             WHERE file_id = ?1 AND byte_start <= ?2 AND byte_end >= ?2
             ORDER BY (byte_end - byte_start) ASC
             LIMIT 1",
        )?;
        let result = stmt
            .query_row(params![file_id, byte_offset as i64], row_to_structural_node)
            .optional()?;
        Ok(result)
    }

    #[allow(clippy::expect_used)]
    async fn structural_node_by_id(&self, id: i64) -> Result<Option<StructuralNodeRecord>> {
        let conn = self.conn.lock().expect("mutex poisoned");
        let mut stmt = conn.prepare_cached(
            "SELECT id, file_id, kind, label, path_joined, path_json, byte_start, byte_end, line_start, line_end, parent_id, depth
             FROM structural_nodes WHERE id = ?1",
        )?;
        let result = stmt
            .query_row(params![id], row_to_structural_node)
            .optional()?;
        Ok(result)
    }

    // ---- Memory operations ----

    #[allow(clippy::expect_used)]
    async fn save_memory(
        &self,
        scope: &str,
        content: &str,
        metadata: &HashMap<String, String>,
    ) -> Result<String> {
        let id = uuid::Uuid::new_v4().to_string();
        let metadata_json = serde_json::to_string(metadata).unwrap_or_else(|_| "{}".to_string());
        let conn = self.conn.lock().expect("mutex poisoned");
        conn.execute(
            "INSERT INTO memories (id, scope, content, metadata) VALUES (?1, ?2, ?3, ?4)",
            params![id, scope, content, metadata_json],
        )?;
        Ok(id)
    }

    #[allow(clippy::expect_used)]
    async fn search_memories(
        &self,
        query: &str,
        scope: Option<&str>,
        k: usize,
    ) -> Result<Vec<MemoryEntry>> {
        let conn = self.conn.lock().expect("mutex poisoned");

        let sql = if scope.is_some() {
            "SELECT m.id, m.scope, m.content, m.metadata, m.created_at
             FROM memories_fts fts
             JOIN memories m ON m.rowid = fts.rowid
             WHERE memories_fts MATCH ?1 AND m.scope = ?2
             ORDER BY rank
             LIMIT ?3"
        } else {
            "SELECT m.id, m.scope, m.content, m.metadata, m.created_at
             FROM memories_fts fts
             JOIN memories m ON m.rowid = fts.rowid
             WHERE memories_fts MATCH ?1
             ORDER BY rank
             LIMIT ?2"
        };

        let mut entries = Vec::new();
        if let Some(s) = scope {
            let mut stmt = conn.prepare_cached(sql)?;
            let rows = stmt.query_map(params![query, s, k as i64], row_to_memory_entry)?;
            for row in rows {
                entries.push(row?);
            }
        } else {
            let mut stmt = conn.prepare_cached(sql)?;
            let rows = stmt.query_map(params![query, k as i64], row_to_memory_entry)?;
            for row in rows {
                entries.push(row?);
            }
        }

        Ok(entries)
    }

    #[allow(clippy::expect_used)]
    async fn list_memories(&self, scope: &str) -> Result<Vec<MemoryEntry>> {
        let conn = self.conn.lock().expect("mutex poisoned");
        let mut stmt = conn.prepare_cached(
            "SELECT id, scope, content, metadata, created_at FROM memories WHERE scope = ?1 ORDER BY created_at DESC",
        )?;
        let rows = stmt.query_map(params![scope], row_to_memory_entry)?;
        let mut entries = Vec::new();
        for row in rows {
            entries.push(row?);
        }
        Ok(entries)
    }

    #[allow(clippy::expect_used)]
    async fn forget_memory(&self, id: &str) -> Result<()> {
        let conn = self.conn.lock().expect("mutex poisoned");
        conn.execute("DELETE FROM memories WHERE id = ?1", params![id])?;
        Ok(())
    }
}

// ---------------------------------------------------------------
// Structural node helper
// ---------------------------------------------------------------

fn row_to_structural_node(row: &Row) -> rusqlite::Result<StructuralNodeRecord> {
    let path_json: String = row.get(5)?;
    let path: Vec<String> = serde_json::from_str(&path_json).unwrap_or_default();
    Ok(StructuralNodeRecord {
        id: row.get(0)?,
        file_id: row.get(1)?,
        kind: row.get(2)?,
        label: row.get(3)?,
        path_joined: row.get(4)?,
        path,
        byte_range: (row.get::<_, i64>(6)? as u32, row.get::<_, i64>(7)? as u32),
        line_range: (row.get::<_, i64>(8)? as u32, row.get::<_, i64>(9)? as u32),
        parent_id: row.get(10)?,
        depth: row.get::<_, i64>(11)? as u16,
    })
}

fn row_to_memory_entry(row: &Row) -> rusqlite::Result<MemoryEntry> {
    let id: String = row.get(0)?;
    let scope: String = row.get(1)?;
    let content: String = row.get(2)?;
    let metadata_json: String = row.get(3)?;
    let created_at: String = row.get(4)?;
    let metadata: HashMap<String, String> =
        serde_json::from_str(&metadata_json).unwrap_or_default();
    Ok(MemoryEntry {
        id,
        scope,
        content,
        metadata,
        created_at,
    })
}

// ---------------------------------------------------------------
// Vector helpers
// ---------------------------------------------------------------

fn vector_to_blob(vec: &[f32]) -> Vec<u8> {
    vec.iter().flat_map(|f| f32::to_le_bytes(*f)).collect()
}

fn blob_to_vector(blob: &[u8]) -> Option<Vec<f32>> {
    let chunks = blob.chunks_exact(4);
    let vec: Vec<f32> = chunks
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect();
    if vec.is_empty() && !blob.is_empty() {
        return None;
    }
    Some(vec)
}

fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}

fn norm(v: &[f32]) -> f32 {
    v.iter().map(|x| x * x).sum::<f32>().sqrt()
}

// ---------------------------------------------------------------
// Symbol helper functions
// ---------------------------------------------------------------

fn symbol_kind_to_str(k: SymbolKind) -> &'static str {
    k.as_str()
}

fn symbol_kind_from_str(s: &str) -> Option<SymbolKind> {
    match s {
        "function" => Some(SymbolKind::Function),
        "method" => Some(SymbolKind::Method),
        "struct" => Some(SymbolKind::Struct),
        "enum" => Some(SymbolKind::Enum),
        "trait" => Some(SymbolKind::Trait),
        "impl" => Some(SymbolKind::Impl),
        "class" => Some(SymbolKind::Class),
        "module" => Some(SymbolKind::Module),
        "variable" => Some(SymbolKind::Variable),
        "type_alias" => Some(SymbolKind::TypeAlias),
        "constant" => Some(SymbolKind::Constant),
        "interface" => Some(SymbolKind::Interface),
        "macro" => Some(SymbolKind::Macro),
        "static" => Some(SymbolKind::Static),
        _ => None,
    }
}

fn row_to_symbol(row: &Row) -> rusqlite::Result<Symbol> {
    let name: String = row.get(1)?;
    let kind_str: String = row.get(2)?;
    let file_str: String = row.get(3)?;
    let range_start: i64 = row.get(4)?;
    let range_end: i64 = row.get(5)?;
    let signature: Option<String> = row.get(6)?;
    let parent_id: Option<String> = row.get(7)?;

    let file = Utf8PathBuf::from(&file_str);
    let range = ByteRange::new(range_start as usize, range_end as usize);
    let kind = symbol_kind_from_str(&kind_str).unwrap_or(SymbolKind::Function);
    let id = SymbolId::new(&file, &name, range.start);
    let parent = parent_id
        .as_deref()
        .and_then(parse_symbol_id)
        .map(|(f, n, s)| SymbolId::new(&f, &n, s));

    Ok(Symbol {
        id,
        name,
        kind,
        file,
        range,
        signature,
        parent,
    })
}

// ---------------------------------------------------------------
// Chunk helper functions
// ---------------------------------------------------------------

fn chunk_kind_to_str(k: ChunkKind) -> &'static str {
    match k {
        ChunkKind::FunctionBody => "FunctionBody",
        ChunkKind::TypeDef => "TypeDef",
        ChunkKind::TopLevel => "TopLevel",
        ChunkKind::Fallback => "Fallback",
    }
}

fn _chunk_kind_from_str(s: &str) -> Option<ChunkKind> {
    match s {
        "FunctionBody" => Some(ChunkKind::FunctionBody),
        "TypeDef" => Some(ChunkKind::TypeDef),
        "TopLevel" => Some(ChunkKind::TopLevel),
        "Fallback" => Some(ChunkKind::Fallback),
        _ => None,
    }
}

fn hex_encode(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[allow(dead_code)]
fn hex_decode(s: &str) -> Option<[u8; 32]> {
    if s.len() != 64 {
        return None;
    }
    let mut bytes = [0u8; 32];
    for (i, chunk) in s.as_bytes().chunks(2).enumerate() {
        let hi = hex_nibble(chunk[0])?;
        let lo = hex_nibble(chunk[1])?;
        bytes[i] = (hi << 4) | lo;
    }
    Some(bytes)
}

#[allow(dead_code)]
fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

#[allow(dead_code)]
fn _row_to_chunk(row: &Row, _language: Option<Language>) -> rusqlite::Result<Chunk> {
    let id_hex: String = row.get(0)?;
    let file_str: String = row.get(1)?;
    let range_start: i64 = row.get(2)?;
    let range_end: i64 = row.get(3)?;
    let text: String = row.get(4)?;
    let kind_str: String = row.get(5)?;
    let lang_str: String = row.get(6)?;

    let file = Utf8PathBuf::from(&file_str);
    let range = ByteRange::new(range_start as usize, range_end as usize);
    let id = hex_decode(&id_hex)
        .map(ChunkId)
        .unwrap_or(ChunkId([0u8; 32]));
    let kind = _chunk_kind_from_str(&kind_str).unwrap_or(ChunkKind::TopLevel);
    let language = str_to_language(&lang_str).unwrap_or(Language::Markdown);

    Ok(Chunk {
        id,
        file,
        range,
        text,
        kind,
        language,
    })
}

// ---------------------------------------------------------------
// Edge helper functions
// ---------------------------------------------------------------

fn edge_kind_to_str(k: EdgeKind) -> &'static str {
    match k {
        EdgeKind::Defines => "Defines",
        EdgeKind::References => "References",
        EdgeKind::Calls => "Calls",
        EdgeKind::Imports => "Imports",
        EdgeKind::ImportedBy => "ImportedBy",
        EdgeKind::Implements => "Implements",
        EdgeKind::Inherits => "Inherits",
    }
}

fn edge_kind_from_str(s: &str) -> Option<EdgeKind> {
    match s {
        "Defines" => Some(EdgeKind::Defines),
        "References" => Some(EdgeKind::References),
        "Calls" => Some(EdgeKind::Calls),
        "Imports" => Some(EdgeKind::Imports),
        "ImportedBy" => Some(EdgeKind::ImportedBy),
        "Implements" => Some(EdgeKind::Implements),
        "Inherits" => Some(EdgeKind::Inherits),
        _ => None,
    }
}

fn confidence_to_str(c: Confidence) -> &'static str {
    match c {
        Confidence::Resolved => "Resolved",
        Confidence::Heuristic => "Heuristic",
        Confidence::Ambiguous => "Ambiguous",
    }
}

fn confidence_from_str(s: &str) -> Option<Confidence> {
    match s {
        "Resolved" => Some(Confidence::Resolved),
        "Heuristic" => Some(Confidence::Heuristic),
        "Ambiguous" => Some(Confidence::Ambiguous),
        _ => None,
    }
}

fn parse_symbol_id(s: &str) -> Option<(Utf8PathBuf, String, usize)> {
    let mut parts = s.split("::");
    let file = parts.next()?;
    let name = parts.next()?;
    let start_str = parts.next()?;
    let start: usize = start_str.parse().ok()?;
    Some((Utf8PathBuf::from(file), name.to_string(), start))
}

fn row_to_edge(row: &Row) -> rusqlite::Result<Edge> {
    let from_str: String = row.get(0)?;
    let to_str: String = row.get(1)?;
    let kind_str: String = row.get(2)?;
    let conf_str: String = row.get(3)?;

    let kind = edge_kind_from_str(&kind_str).unwrap_or(EdgeKind::References);
    let confidence = confidence_from_str(&conf_str).unwrap_or(Confidence::Heuristic);

    let from = parse_symbol_id(&from_str)
        .map(|(f, n, s)| SymbolId::new(&f, &n, s))
        .unwrap_or_else(|| SymbolId::new(&Utf8PathBuf::from("?"), "?", 0));

    let to = parse_symbol_id(&to_str)
        .map(|(f, n, s)| SymbolId::new(&f, &n, s))
        .unwrap_or_else(|| SymbolId::new(&Utf8PathBuf::from("?"), "?", 0));

    Ok(Edge {
        from,
        to,
        kind,
        confidence,
    })
}

// ---------------------------------------------------------------
// Selector-based edge queries
// ---------------------------------------------------------------

enum SelectorPattern {
    Exact(String),
    Prefix(String),
    Contains(String),
}

fn selector_to_pattern(sel: &SymbolSelector) -> SelectorPattern {
    match sel {
        SymbolSelector::ById(id) => SelectorPattern::Exact(id.as_str().to_string()),
        SymbolSelector::ByName { file, name } => {
            SelectorPattern::Prefix(format!("{file}::{name}::"))
        }
        SymbolSelector::Qualified(qn) => SelectorPattern::Contains(qn.clone()),
    }
}

/// Query edges where `to_id` matches the selector (incoming edges).
fn query_edges_incoming(
    conn: &Connection,
    sel: &SymbolSelector,
    kind: EdgeKind,
) -> Result<Vec<Edge>> {
    let kind_str = edge_kind_to_str(kind);
    let pattern = selector_to_pattern(sel);

    let (sql, param_str): (&str, String) = match &pattern {
        SelectorPattern::Exact(id) => (
            "SELECT from_id, to_id, kind, confidence FROM edges
             WHERE kind = ?1 AND to_id = ?2",
            id.clone(),
        ),
        SelectorPattern::Prefix(prefix) => (
            "SELECT from_id, to_id, kind, confidence FROM edges
             WHERE kind = ?1 AND to_id LIKE ?2",
            format!("{prefix}%"),
        ),
        SelectorPattern::Contains(qn) => (
            "SELECT from_id, to_id, kind, confidence FROM edges
             WHERE kind = ?1 AND to_id LIKE '%' || ?2 || '%'",
            qn.clone(),
        ),
    };

    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(params![kind_str, param_str.as_str()], row_to_edge)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

/// Query edges where `from_id` matches the selector (outgoing edges).
fn query_edges_outgoing(
    conn: &Connection,
    sel: &SymbolSelector,
    kind: EdgeKind,
) -> Result<Vec<Edge>> {
    let kind_str = edge_kind_to_str(kind);
    let pattern = selector_to_pattern(sel);

    let (sql, param_str): (&str, String) = match &pattern {
        SelectorPattern::Exact(id) => (
            "SELECT from_id, to_id, kind, confidence FROM edges
             WHERE kind = ?1 AND from_id = ?2",
            id.clone(),
        ),
        SelectorPattern::Prefix(prefix) => (
            "SELECT from_id, to_id, kind, confidence FROM edges
             WHERE kind = ?1 AND from_id LIKE ?2",
            format!("{prefix}%"),
        ),
        SelectorPattern::Contains(qn) => (
            "SELECT from_id, to_id, kind, confidence FROM edges
             WHERE kind = ?1 AND from_id LIKE '%' || ?2 || '%'",
            qn.clone(),
        ),
    };

    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(params![kind_str, param_str.as_str()], row_to_edge)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

// ---------------------------------------------------------------
// General helpers
// ---------------------------------------------------------------

fn blob_to_hash(blob: &[u8]) -> Blake3Hash {
    let mut bytes = [0u8; 32];
    let len = blob.len().min(32);
    bytes[..len].copy_from_slice(&blob[..len]);
    Blake3Hash::from(bytes)
}

fn language_to_str(lang: Language) -> &'static str {
    match lang {
        Language::Rust => "Rust",
        Language::TypeScript => "TypeScript",
        Language::Python => "Python",
        Language::JavaScript => "JavaScript",
        Language::Markdown => "Markdown",
        Language::Json => "Json",
        Language::Yaml => "Yaml",
        Language::Toml => "Toml",
        Language::Csv => "Csv",
    }
}

fn str_to_language(s: &str) -> Option<Language> {
    match s {
        "Rust" => Some(Language::Rust),
        "TypeScript" => Some(Language::TypeScript),
        "Python" => Some(Language::Python),
        "JavaScript" => Some(Language::JavaScript),
        "Markdown" => Some(Language::Markdown),
        "Json" => Some(Language::Json),
        "Yaml" => Some(Language::Yaml),
        "Toml" => Some(Language::Toml),
        "Csv" => Some(Language::Csv),
        _ => None,
    }
}

// ---------------------------------------------------------------
// Search / vector query builders
// ---------------------------------------------------------------

fn build_bm25_sql(
    limit: usize,
    filter: &SearchFilter,
) -> (String, Vec<Box<dyn rusqlite::types::ToSql>>) {
    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    let mut conditions = Vec::new();

    if let Some(ref lang) = filter.language {
        conditions.push(format!("c.language = ?{}", params.len() + 2));
        params.push(Box::new(lang.clone()));
    }
    if let Some(ref glob) = filter.paths_glob {
        conditions.push(format!("c.file GLOB ?{}", params.len() + 2));
        params.push(Box::new(glob.clone()));
    }
    if let Some(ref glob) = filter.exclude_glob {
        conditions.push(format!("c.file NOT GLOB ?{}", params.len() + 2));
        params.push(Box::new(glob.clone()));
    }

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!(" AND {}", conditions.join(" AND "))
    };

    let sql = format!(
        "SELECT c.id, rank \
         FROM chunks_fts \
         JOIN chunks c ON c.rowid = chunks_fts.rowid \
         WHERE chunks_fts MATCH ?1 \
         {where_clause} \
         ORDER BY rank \
         LIMIT {limit}"
    );

    (sql, params)
}

fn build_vector_search_sql(
    _dimension: usize,
    filter: &SearchFilter,
) -> (String, Vec<Box<dyn rusqlite::types::ToSql>>) {
    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
    let mut conditions = Vec::new();

    if let Some(ref lang) = filter.language {
        conditions.push(format!("c.language = ?{}", params.len() + 1));
        params.push(Box::new(lang.clone()));
    }
    if let Some(ref glob) = filter.paths_glob {
        conditions.push(format!("c.file GLOB ?{}", params.len() + 1));
        params.push(Box::new(glob.clone()));
    }
    if let Some(ref glob) = filter.exclude_glob {
        conditions.push(format!("c.file NOT GLOB ?{}", params.len() + 1));
        params.push(Box::new(glob.clone()));
    }

    let where_clause = if conditions.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", conditions.join(" AND "))
    };

    let sql = format!(
        "SELECT v.chunk_id, v.vector, c.file, c.text \
         FROM vectors v \
         JOIN chunks c ON c.id = v.chunk_id \
         {where_clause}"
    );

    (sql, params)
}

fn chunk_info(conn: &Connection, chunk_id: &str) -> Result<(String, String, u32, u32)> {
    let mut stmt =
        conn.prepare_cached("SELECT text, file, range_start, range_end FROM chunks WHERE id = ?1")?;
    stmt.query_row(params![chunk_id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, i64>(2)? as u32,
            row.get::<_, i64>(3)? as u32,
        ))
    })
    .map_err(|e| e.into())
}

fn in_both(bm25_hits: &[(String, f32)], vector_hits: &[(String, f32)], chunk_id: &str) -> bool {
    bm25_hits.iter().any(|(id, _)| id == chunk_id)
        && vector_hits.iter().any(|(id, _)| id == chunk_id)
}

fn text_bytes_to_line_range(text: &str, byte_start: u32, byte_end: u32) -> (u32, u32) {
    let newline_count_before = text[..(byte_start as usize).min(text.len())]
        .chars()
        .filter(|&c| c == '\n')
        .count() as u32;
    let newline_count_total = text.chars().filter(|&c| c == '\n').count() as u32;
    let start_line = newline_count_before + 1;
    let end_line = if byte_end > byte_start {
        newline_count_total + 1
    } else {
        start_line
    };
    (start_line, end_line.max(start_line))
}
