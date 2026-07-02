use std::future::Future;

use foundationdb::FdbError;
use futures::FutureExt;

use crate::encoding::{pack_cursor_key, pack_sentinel_shard_key, SENTINEL_SHARDS};
use crate::error::Error;
use crate::types::{FdbStore, Versionstamp};

impl FdbStore {
    pub async fn get_cursor(&self, name: &str) -> Result<Option<Versionstamp>, Error> {
        let key = pack_cursor_key(&self.namespace, name);
        let tr = self.db.create_trx().map_err(Error::Fdb)?;
        match tr.get(&key, false).await.map_err(Error::Fdb)? {
            None => Ok(None),
            Some(val) => {
                if val.len() != 12 {
                    return Err(Error::TupleDecode(format!(
                        "cursor value has {} bytes, expected 12",
                        val.len()
                    )));
                }
                let mut vs = [0u8; 12];
                vs.copy_from_slice(&val);
                Ok(Some(vs))
            }
        }
    }

    pub async fn set_cursor(&self, name: &str, position: Versionstamp) -> Result<(), Error> {
        let key = pack_cursor_key(&self.namespace, name);
        let mut tr = self.db.create_trx().map_err(Error::Fdb)?;
        loop {
            tr.set(&key, &position);
            match tr.commit().await {
                Ok(_) => return Ok(()),
                Err(e) => tr = e.on_error().await.map_err(Error::Fdb)?,
            }
        }
    }

    /// Register a watch on every sentinel shard for this namespace and return a
    /// single future that resolves when the first one fires.
    ///
    /// Each append touches exactly one shard (round-robin), so arming all
    /// `SENTINEL_SHARDS` in one transaction preserves the guarantee that any
    /// append committing after this returns `Ok` resolves the returned future.
    /// The watches are durably registered when this function returns `Ok` — the
    /// caller can then run a catch-up read; registering BEFORE catching up
    /// closes the wake-loss race.
    ///
    /// The returned future resolves with `Err` if any watch fails (e.g.
    /// `too_many_watches`); callers should back off before re-arming to avoid a
    /// hot loop.
    pub async fn register_sentinel_watch(
        &self,
    ) -> Result<impl Future<Output = Result<(), FdbError>> + Send + Unpin, Error> {
        let tr = self.db.create_trx().map_err(Error::Fdb)?;
        let watches: Vec<_> = (0..SENTINEL_SHARDS)
            .map(|shard| tr.watch(&pack_sentinel_shard_key(&self.namespace, shard)))
            .collect();
        tr.commit().await.map_err(|e| Error::Fdb(e.into()))?;
        // Resolve when the first shard fires; an append always lands on one of them.
        Ok(futures::future::select_all(watches).map(|(res, _, _)| res))
    }

    // Parks until the sentinel key changes (i.e. an append occurred).
    // Always returns — errors are treated as spurious wakes so the caller
    // can run catch_up regardless (catch_up is idempotent and finds 0 new
    // events on a spurious wake). Prefer `register_sentinel_watch` when the
    // caller must know the watch is armed before catching up, or needs to
    // distinguish errors to back off.
    pub async fn wait_for_sentinel_change(&self) {
        if let Ok(watch) = self.register_sentinel_watch().await {
            let _ = watch.await;
        }
    }
}
