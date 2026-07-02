//! # dcb
//!
//! A [DCB-compliant](https://dcb.events) event store backed by [FoundationDB](https://www.foundationdb.org).
//!
//! Events are appended atomically, assigned a globally ordered [`Versionstamp`] position,
//! and indexed for fast retrieval by type, tags, or both.
//! Optimistic concurrency is enforced through [`AppendCondition`]s that fail the write
//! if matching events appeared since the caller last read.
//!
//! ## Quick start
//!
//! ```rust,no_run
//! use dcb_layer::{Event, FdbStore, Query, QueryItem};
//!
//! #[tokio::main]
//! async fn main() {
//!     // 1. Open a FoundationDB database (see foundationdb-rs for setup).
//!     let _network = unsafe { foundationdb::boot() };
//!     let db = foundationdb::Database::default().unwrap();
//!
//!     // 2. Create a namespaced store — namespace isolates data within one FDB cluster.
//!     let store = FdbStore::new(db, "my-app");
//!
//!     // 3. Append events (no condition = unconditional write).
//!     let event = Event::new(
//!         "OrderPlaced",
//!         ["order:42", "customer:7"],
//!         b"{\"total\":99}".as_slice(),
//!     );
//!     let position = store.append(vec![event], vec![]).await.unwrap();
//!
//!     // 4. Read events back — filter by type, tags, or both.
//!     let results = store
//!         .read(
//!             Query {
//!                 items: vec![QueryItem {
//!                     types: vec!["OrderPlaced".into()],
//!                     tags: vec!["customer:7".into()],
//!                 }],
//!             },
//!             None,
//!         )
//!         .await
//!         .unwrap();
//!
//!     println!("Found {} event(s), first at {:?}", results.len(), results[0].position);
//! }
//! ```
//!
//! ## Core concepts
//!
//! ### Event
//!
//! An [`Event`] carries three fields, constructed via [`Event::new`]:
//!
//! | Field | Accepts | Notes |
//! |-------|---------|-------|
//! | `type_name` | `&str` / `String` | Required, non-empty (e.g. `"OrderPlaced"`) |
//! | `tags` | any iterable of strings | Up to 10 tags; `"_"` is reserved |
//! | `data` | `&[u8]` / `Vec<u8>` | Opaque bytes — encode however you like |
//!
//! Tags are free-form strings. A common convention is `"entity:id"` (e.g. `"order:42"`),
//! but the store treats them as opaque labels.
//!
//! ### Query
//!
//! A [`Query`] is a list of [`QueryItem`]s joined by **OR**.
//! Each item matches events that satisfy **all** of its constraints:
//!
//! - `types` — event's `type_name` must be one of these (empty = any type)
//! - `tags` — event must carry **all** of these tags (empty = any tags)
//!
//! Every item must specify at least one type or one tag.
//!
//! ```rust
//! use dcb_layer::{Query, QueryItem};
//!
//! // Match OrderPlaced OR OrderCancelled events tagged with order:42
//! let query = Query {
//!     items: vec![
//!         QueryItem { types: vec!["OrderPlaced".into()], tags: vec!["order:42".into()] },
//!         QueryItem { types: vec!["OrderCancelled".into()], tags: vec!["order:42".into()] },
//!     ],
//! };
//! ```
//!
//! ### AppendCondition
//!
//! An [`AppendCondition`] guards a write with an optimistic-concurrency check:
//! the `append` call fails with [`Error::AppendConditionFailed`] if any event
//! matching `query` exists **after** the `after` position (or at all, when `after` is `None`).
//!
//! The typical pattern:
//!
//! 1. Read events matching your query → note the last position seen.
//! 2. Build your decision model from those events.
//! 3. Append new events with `after = last_position` to guard against concurrent writes.
//!
//! ```rust,no_run
//! use dcb_layer::{AppendCondition, Event, FdbStore, Query, QueryItem};
//!
//! # async fn example(store: FdbStore) {
//! // 1. Read events and note the last position seen.
//! let results = store.read(
//!     Query { items: vec![QueryItem { types: vec!["OrderPlaced".into()], tags: vec!["order:42".into()] }] },
//!     None,
//! ).await.unwrap();
//! let last_position = results.last().map(|e| e.position);
//!
//! // 2. Append new events — fail if a concurrent write snuck in after last_position.
//! let condition = AppendCondition {
//!     query: Query {
//!         items: vec![QueryItem {
//!             types: vec!["OrderPlaced".into()],
//!             tags: vec!["order:42".into()],
//!         }],
//!     },
//!     after: last_position,
//! };
//! store.append(vec![Event::new("OrderPlaced", ["order:42"], b"".as_slice())], vec![condition]).await.unwrap();
//! # }
//! ```
//!
//! ### ReadOptions
//!
//! [`ReadOptions`] controls pagination and ordering:
//!
//! | Field | Default | Meaning |
//! |-------|---------|---------|
//! | `limit` | `0` (unlimited) | Maximum events to return |
//! | `after` | `None` | Only return events after this position |
//! | `reverse` | `false` | Newest-first when `true` |
//!
//! ## Limits
//!
//! | Constraint | Value |
//! |------------|-------|
//! | Max events per `append` call | 65 535 |
//! | Max tags per event | 10 |
//! | Reserved tag value | `"_"` |
//!
//! ### Practical sizing
//!
//! Each event writes one index key per **subset** of its tags (2^n keys for
//! n tags — up to 1 024 for 10 tags). All writes of an `append` call share one
//! FDB transaction, which is capped at 10 MB of affected data and 5 seconds —
//! keep batches small when events carry many tags.
//!
//! Reads run in a single FDB transaction too: an unbounded [`FdbStore::read`]
//! or [`FdbStore::read_all`] over a large store fails with
//! `transaction_too_old` (FDB error 1007) after a bounded number of retries.
//! Paginate with [`ReadOptions`] `limit` + `after`.
//!
//! Subscription cursors ([`FdbStore::set_cursor`]) are last-writer-wins; run
//! one consumer per cursor name or coordinate externally.
//!
//! ## FoundationDB setup
//!
//! This crate wraps the [`foundationdb`](https://docs.rs/foundationdb) crate.
//! You are responsible for:
//! - installing and running an FDB cluster
//! - calling `unsafe { foundationdb::boot() }` once at startup (keeps the network thread alive)
//! - opening a [`foundationdb::Database`] and passing it to [`FdbStore::new`]
//!
//! Refer to the [foundationdb-rs documentation](https://docs.rs/foundationdb) and
//! the [FoundationDB developer guide](https://apple.github.io/foundationdb/getting-started-linux.html)
//! for cluster setup and client configuration.
//!
//! ### FDB API version
//!
//! The default Cargo feature targets FDB 7.4. To use FDB 7.3, disable the default features:
//!
//! ```toml
//! dcb-layer = { version = "...", default-features = false, features = ["fdb-7_3"] }
//! ```
//!
//! The feature selects the C API headers compiled against; a newer `libfdb_c` is backward compatible.

mod error;
mod types;
mod encoding;
mod append;
mod query;
mod read;
mod subscribe;

pub use error::Error;
pub use types::{Event, StoredEvent, QueryItem, Query, AppendCondition, ReadOptions, FdbStore, Versionstamp};
