use std::future::Future;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use argyph_embed::config::EmbedConfig;
use argyph_embed::{self, Embedder, Provider};
use camino::Utf8PathBuf;
use tokio::sync::RwLock;
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use argyph_store::SqliteStore;
use argyph_store::Store;

use crate::config::Config;
use crate::error::Result;
use crate::index::Index;
use crate::tiers::{self, Tier2Progress, TierState};

pub struct Supervisor {
    #[allow(dead_code)]
    config: Arc<Config>,
    root: Utf8PathBuf,
    index: Arc<Index>,
    tier_state: Arc<RwLock<TierState>>,
    tasks: Mutex<JoinSet<()>>,
    shutdown: CancellationToken,
    store: Arc<dyn Store>,
    watcher_active: bool,
    embedder: Arc<OnceLock<Arc<dyn Embedder>>>,
}

impl Supervisor {
    #[tracing::instrument(skip(config), fields(root = %root.as_str()))]
    pub async fn boot(root: Utf8PathBuf, config: Config) -> Result<Self> {
        tracing::info!("booting supervisor");

        let store: Arc<dyn Store> = {
            let sqlite = SqliteStore::open_at(&root)?;
            Arc::new(sqlite)
        };

        let (embedder, tier2_concurrency) = build_embedder();
        let embedder_for_t2 = Arc::clone(&embedder);

        let index = Arc::new(Index::new(Arc::clone(&store), Arc::clone(&embedder)));
        let tier_state = Arc::new(RwLock::new(TierState::Offline));

        let entries = tiers::run_tier0(&root, &*store).await?;
        let files_indexed = entries.len();

        *tier_state.write().await = TierState::Tier0 { files_indexed };
        tracing::info!(files_indexed, "Tier 0 ready");

        let tier_state_clone = Arc::clone(&tier_state);
        let root_clone = root.clone();
        let store_clone = Arc::clone(&store);

        let sup = Self {
            config: Arc::new(config),
            root,
            index,
            tier_state,
            tasks: Mutex::new(JoinSet::new()),
            shutdown: CancellationToken::new(),
            store,
            watcher_active: false,
            embedder,
        };

        // Channel to signal Tier 2 start after Tier 1 finishes
        let (tier2_start_tx, tier2_start_rx) = tokio::sync::oneshot::channel::<()>();
        let (tier1_5_start_tx, tier1_5_start_rx) = tokio::sync::oneshot::channel::<()>();

        // Tier 1 — parse symbols, create chunks
        let root_for_t1 = sup.root.clone();
        let store_for_t1 = Arc::clone(&store_clone);
        let tier_for_t1 = Arc::clone(&tier_state_clone);
        sup.spawn(async move {
            match tiers::run_tier1(&root_for_t1, &*store_for_t1).await {
                Ok(symbol_count) => {
                    *tier_for_t1.write().await = TierState::Tier1 {
                        symbols_indexed: symbol_count as usize,
                    };
                    tracing::info!(symbol_count, "Tier 1 ready");
                }
                Err(e) => {
                    tracing::error!(error = %e, "Tier 1 failed");
                }
            }
            // Signal Tier 2 that chunks exist (or Tier 1 failed — Tier 2 exits quickly either way)
            let _ = tier2_start_tx.send(());
            let _ = tier1_5_start_tx.send(());
        });

        // Tier 1.5 — structural indexing (starts after Tier 1, parallel with Tier 2)
        {
            let root_for_t1_5 = sup.root.clone();
            let store_for_t1_5 = Arc::clone(&store_clone);
            let tier_for_t1_5 = Arc::clone(&tier_state_clone);
            let max_bytes = sup.config.locate.max_file_bytes;
            sup.spawn(async move {
                let _ = tier1_5_start_rx.await;
                match tiers::run_tier1_5(&*store_for_t1_5, &root_for_t1_5, max_bytes).await {
                    Ok(count) => {
                        let mut s = tier_for_t1_5.write().await;
                        if matches!(*s, TierState::Tier1 { .. })
                            || matches!(*s, TierState::Tier1_5 { .. })
                        {
                            *s = TierState::Tier1_5 {
                                structural_files: count,
                            };
                        }
                        tracing::info!(structural_files = count, "Tier 1.5 ready");
                    }
                    Err(e) => {
                        tracing::warn!("Tier 1.5 failed: {e}");
                    }
                }
            });
        }

        // Tier 2 — background semantic embedding (waits for Tier 1, then polls)
        {
            let store_for_t2 = Arc::clone(&store_clone);
            let tier_for_t2 = Arc::clone(&tier_state_clone);
            let (progress_tx, mut progress_rx) =
                tokio::sync::mpsc::unbounded_channel::<Tier2Progress>();

            // Progress-tracking task — reads progress events, updates tier_state
            let tier_prog = Arc::clone(&tier_for_t2);
            sup.spawn(async move {
                while let Some(prog) = progress_rx.recv().await {
                    *tier_prog.write().await = TierState::Tier2 {
                        embedded: prog.embedded,
                        total: prog.total,
                    };
                }
            });

            // Tier 2 embedding task — waits for Tier 1, then runs embedding loop
            sup.spawn(async move {
                let _ = tier2_start_rx.await;

                let Some(embedder) = embedder_for_t2.get().cloned() else {
                    return;
                };

                tracing::info!("Tier 2 starting semantic embedding");
                match tiers::run_tier2(store_for_t2, embedder, progress_tx, tier2_concurrency).await
                {
                    Ok(()) => {
                        *tier_for_t2.write().await = TierState::Ready;
                        tracing::info!("Tier 2 ready — all chunks embedded");
                    }
                    Err(e) => {
                        tracing::error!(%e, "Tier 2 embedding failed");
                    }
                }
            });
        }

        let mut sup = sup;
        let watcher = crate::watcher::create_watcher(&root_clone, Duration::from_millis(500));

        let orch = crate::watcher::WatcherOrchestrator::new(
            root_clone.clone(),
            watcher,
            store_clone,
            tier_state_clone,
        );
        sup.spawn(async move {
            orch.run().await;
        });
        sup.watcher_active = true;

        Ok(sup)
    }

    pub async fn run(&self) -> Result<()> {
        tracing::info!("supervisor running");
        self.shutdown.cancelled().await;
        tracing::info!("supervisor shutdown signal received");
        Ok(())
    }

    pub fn watcher_active(&self) -> bool {
        self.watcher_active
    }

    pub fn index(&self) -> Arc<Index> {
        Arc::clone(&self.index)
    }

    pub fn store(&self) -> Arc<dyn Store> {
        Arc::clone(&self.store)
    }

    pub fn embedder(&self) -> Option<Arc<dyn Embedder>> {
        self.embedder.get().cloned()
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn root(&self) -> &Utf8PathBuf {
        &self.root
    }

    pub async fn get_tier_state(&self) -> TierState {
        *self.tier_state.read().await
    }

    #[allow(clippy::expect_used)]
    pub async fn shutdown(self) -> Result<()> {
        tracing::info!("supervisor shutting down");
        self.shutdown.cancel();

        let mut tasks = self.tasks.into_inner().unwrap_or_else(|e| e.into_inner());
        while let Some(result) = tasks.join_next().await {
            if let Err(e) = result {
                tracing::warn!(error = %e, "task panicked during shutdown");
            }
        }

        self.store.close().await?;

        tracing::info!("supervisor shut down");
        Ok(())
    }

    #[allow(clippy::expect_used)]
    pub fn spawn<F, T>(&self, fut: F)
    where
        F: Future<Output = T> + Send + 'static,
        T: Send + 'static,
    {
        let token = self.shutdown.child_token();
        let mut tasks = self.tasks.lock().expect("mutex poisoned");
        tasks.spawn(async move {
            tokio::select! {
                _ = fut => {},
                _ = token.cancelled() => {},
            }
        });
    }
}

fn build_embedder() -> (Arc<OnceLock<Arc<dyn Embedder>>>, usize) {
    let provider = if std::env::var("OPENAI_API_KEY").is_ok() {
        Provider::OpenAi
    } else {
        Provider::Local
    };
    let tier2_concurrency = provider.default_concurrency();
    let config = EmbedConfig::for_provider(&provider);
    let slot = Arc::new(OnceLock::new());
    let slot_clone = Arc::clone(&slot);
    tokio::task::spawn(async move {
        let result =
            tokio::task::spawn_blocking(move || argyph_embed::build(provider, config)).await;
        match result {
            Ok(Ok(e)) => {
                let _ = slot_clone.set(e);
            }
            Ok(Err(err)) => {
                tracing::warn!(%err, "embedding unavailable — semantic search disabled");
            }
            Err(join_err) => {
                tracing::warn!(%join_err, "embedder build panicked — semantic search disabled");
            }
        }
    });
    (slot, tier2_concurrency)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use argyph_fs::{ChangeKind, ChangedPath};
    use std::time::Duration;

    struct TestFixture {
        _dir: tempfile::TempDir,
        root: Utf8PathBuf,
    }

    fn temp_fixture() -> TestFixture {
        let dir = tempfile::tempdir().unwrap();
        let src = std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../examples/tiny-rust-app"
        ));
        let dst = dir.path().join("repo");
        copy_dir_all(src, &dst).unwrap();
        let root = Utf8PathBuf::from_path_buf(dst).unwrap();
        TestFixture { _dir: dir, root }
    }

    #[allow(clippy::unwrap_used)]
    fn copy_dir_all(src: &std::path::Path, dst: &std::path::Path) -> std::io::Result<()> {
        std::fs::create_dir_all(dst)?;
        for entry in std::fs::read_dir(src)? {
            let entry = entry?;
            let ty = entry.file_type()?;
            let src_path = entry.path();
            let dst_path = dst.join(entry.file_name());
            if ty.is_dir() {
                copy_dir_all(&src_path, &dst_path)?;
            } else if ty.is_symlink() {
                let target = std::fs::read_link(&src_path)?;
                #[cfg(unix)]
                {
                    std::os::unix::fs::symlink(&target, &dst_path)?;
                }
                #[cfg(windows)]
                {
                    // Windows symlinks need an admin token in CI; for the
                    // test fixture, treat the symlink target as a regular
                    // file copy. The fixture tree under examples/ does not
                    // currently contain any symlinks, so this branch is
                    // defensive rather than load-bearing.
                    if target.is_dir() {
                        std::os::windows::fs::symlink_dir(&target, &dst_path)
                            .or_else(|_| std::fs::copy(&src_path, &dst_path).map(|_| ()))?;
                    } else {
                        std::os::windows::fs::symlink_file(&target, &dst_path)
                            .or_else(|_| std::fs::copy(&src_path, &dst_path).map(|_| ()))?;
                    }
                }
            } else {
                std::fs::copy(&src_path, &dst_path)?;
            }
        }
        Ok(())
    }

    #[tokio::test]
    async fn boot_reaches_tier0_in_under_1_second() {
        let fixture = temp_fixture();
        let root = fixture.root;
        let config = Config::default();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(1);

        let sup = Supervisor::boot(root, config).await.unwrap();

        let elapsed = deadline.elapsed();
        assert!(
            elapsed < Duration::from_secs(1),
            "boot took {elapsed:?}, expected <1s"
        );

        let state = sup.get_tier_state().await;
        assert!(state.is_ready(), "expected Tier 0 ready, got {state:?}");

        let status = sup.index().status().await.unwrap();
        assert!(status.file_count > 0, "expected at least one indexed file");

        sup.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn boot_sets_tier_state_fields() {
        let fixture = temp_fixture();
        let config = Config::default();
        let sup = Supervisor::boot(fixture.root, config).await.unwrap();

        let state = sup.get_tier_state().await;
        match state {
            TierState::Tier0 { files_indexed } => {
                assert!(files_indexed > 0, "expected at least 1 file indexed");
            }
            other => panic!("expected Tier 0, got {other:?}"),
        }

        sup.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn shutdown_cleans_up_without_panicking() {
        let fixture = temp_fixture();
        let sup = Supervisor::boot(fixture.root, Config::default())
            .await
            .unwrap();
        sup.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn spawn_registers_cancellation_aware_task() {
        let fixture = temp_fixture();
        let sup = Supervisor::boot(fixture.root, Config::default())
            .await
            .unwrap();

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        sup.spawn(async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            let _ = tx.send(42);
        });

        let val = tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .unwrap();
        assert_eq!(val, Some(42));

        sup.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn incremental_reindex_picks_up_new_file() {
        let fixture = temp_fixture();
        let root = fixture.root.clone();
        let sup = Supervisor::boot(root.clone(), Config::default())
            .await
            .unwrap();

        let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
        let mut tier1_ready = false;
        while tokio::time::Instant::now() < deadline {
            let state = sup.get_tier_state().await;
            if matches!(state, TierState::Tier1 { .. } | TierState::Tier1_5 { .. }) {
                tier1_ready = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        assert!(
            tier1_ready,
            "Tier 1 / Tier 1.5 did not become ready within 30s"
        );

        let new_file_path = camino::Utf8PathBuf::from("src/new_module.rs");
        let new_file_abs = root.join(new_file_path.as_str());
        std::fs::write(
            new_file_abs.as_str(),
            "pub fn watcher_test_symbol() -> u32 { 42 }\n",
        )
        .unwrap();

        let changes = vec![ChangedPath {
            path: new_file_path.clone(),
            kind: ChangeKind::Created,
        }];

        let start = std::time::Instant::now();
        sup.index()
            .reindex(&root, &changes)
            .await
            .expect("reindex should succeed");
        let elapsed = start.elapsed();
        assert!(
            elapsed < Duration::from_millis(3000),
            "reindex took {elapsed:?}, expected <3s"
        );

        let found = sup
            .index()
            .find_symbol("watcher_test_symbol", None)
            .await
            .expect("find_symbol should succeed");
        assert!(
            !found.is_empty(),
            "newly created watcher_test_symbol not found after reindex"
        );
        assert_eq!(
            found[0].file.as_str(),
            "src/new_module.rs",
            "symbol should be associated with the new file"
        );

        sup.shutdown().await.unwrap();
    }
}
