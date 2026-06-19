use crate::encoding::{pack_cursor_key, pack_sentinel_key};
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

    // Parks until the sentinel key changes (i.e. an append occurred).
    // Always returns — errors are treated as spurious wakes so the caller
    // can run catch_up regardless. Watch commit errors are intentionally
    // ignored: a failed watch still wakes the caller, which is correct
    // (catch_up is idempotent and finds 0 new events on a spurious wake).
    pub async fn wait_for_sentinel_change(&self) {
        let sentinel_key = pack_sentinel_key(&self.namespace);
        if let Ok(tr) = self.db.create_trx() {
            let watch = tr.watch(&sentinel_key);
            let _ = tr.commit().await;
            let _ = watch.await;
        }
    }
}
