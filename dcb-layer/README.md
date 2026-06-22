# dcb-layer

A [DCB-compliant](https://dcb.events) event store backed by [FoundationDB](https://www.foundationdb.org).

Events are appended atomically, assigned a globally ordered position, and indexed for fast retrieval by type, tags, or both. Optimistic concurrency is enforced through append conditions that fail the write if matching events appeared since the caller last read.

---

## Quick start

```rust
use dcb_layer::{Event, FdbStore, Query, QueryItem};

#[tokio::main]
async fn main() {
    // Boot the FDB network thread once at startup (see foundationdb-rs docs).
    let _network = unsafe { foundationdb::boot() };
    let db = foundationdb::Database::default().unwrap();

    let store = FdbStore::new(db, "my-app");

    // Append an event unconditionally.
    let event = Event::new(
        "OrderPlaced",
        ["order:42", "customer:7"],
        b"{\"total\":99}".as_slice(),
    );
    let position = store.append(vec![event], vec![]).await.unwrap();

    // Read it back — filter by type AND tag.
    let results = store.read(
        Query {
            items: vec![QueryItem {
                types: vec!["OrderPlaced".into()],
                tags: vec!["order:42".into()],
            }],
        },
        None,
    ).await.unwrap();

    println!("{} event(s) at position {:?}", results.len(), results[0].position);
}
```

---

## Core concepts

### Event

| Field | Accepts | Notes |
|-------|---------|-------|
| `type_name` | `&str` / `String` | Required, non-empty (e.g. `"OrderPlaced"`) |
| `tags` | any iterable of strings | Up to 10; `"_"` is reserved |
| `data` | `&[u8]` / `Vec<u8>` | Opaque bytes — encode however you like |

Tags are free-form strings. A common convention is `"entity:id"` (e.g. `"order:42"`), but the store treats them as opaque labels.

### Query

A `Query` is a list of `QueryItem`s joined by **OR**. Each item matches events that satisfy **all** of its constraints:

- `types` — `type_name` must be one of these (empty = any type)
- `tags` — event must carry **all** of these tags (empty = any tags)

Every item must specify at least one type or one tag.

```rust
// Match OrderPlaced OR OrderCancelled, both tagged order:42
let query = Query {
    items: vec![
        QueryItem { types: vec!["OrderPlaced".into()],   tags: vec!["order:42".into()] },
        QueryItem { types: vec!["OrderCancelled".into()], tags: vec!["order:42".into()] },
    ],
};
```

### AppendCondition

Guards a write with an optimistic-concurrency check: `append` returns `Error::AppendConditionFailed` if any event matching `query` exists after the `after` position (or at all when `after` is `None`).

```rust,no_run
// 1. Read events and note the last position seen.
let results = store.read(
    Query { items: vec![QueryItem { types: vec!["OrderPlaced".into()], tags: vec!["order:42".into()] }] },
    None,
).await.unwrap();
let last_position = results.last().map(|e| e.position);

// 2. Append — fail if a concurrent write appeared after last_position.
let condition = AppendCondition {
    query: Query {
        items: vec![QueryItem {
            types: vec!["OrderPlaced".into()],
            tags: vec!["order:42".into()],
        }],
    },
    after: last_position,
};
store.append(vec![Event::new("OrderShipped", ["order:42"], b"".as_slice())], vec![condition]).await.unwrap();
```

### ReadOptions

```rust,no_run
let opts = ReadOptions {
    limit: 100,                    // 0 = unlimited
    after: Some(last_position),    // only events after this position
    reverse: true,                 // newest first
};
store.read(query, Some(opts)).await.unwrap();
```

---

## Limits

| Constraint | Value |
|------------|-------|
| Max events per `append` | 65 535 |
| Max tags per event | 10 |
| Reserved tag value | `"_"` |

---

## FoundationDB setup

This crate wraps [foundationdb-rs](https://docs.rs/foundationdb). You are responsible for:

- Running an FDB cluster ([getting started guide](https://apple.github.io/foundationdb/getting-started-linux.html))
- Calling `unsafe { foundationdb::boot() }` once at process startup
- Opening a `foundationdb::Database` and passing it to `FdbStore::new`

Refer to the [foundationdb-rs docs](https://docs.rs/foundationdb) for client configuration, cluster files, and TLS options.

### Choosing an FDB API version

The default is FDB 7.4. To target FDB 7.3, disable the default features and enable `fdb-7_3`:

```toml
# FDB 7.4 (default)
dcb = { version = "..." }

# FDB 7.3
dcb = { version = "...", default-features = false, features = ["fdb-7_3"] }
```

The feature controls the C API version compiled against; the installed `libfdb_c` does not need to match exactly — a newer client library is backward compatible.

---

## Running tests on ARM

The integration tests pull the `foundationdb/foundationdb:7.4.5-arm` Docker image. That image is not published to Docker Hub — you must build it yourself from the [FoundationDB source repo](https://github.com/apple/foundationdb):

```sh
git clone https://github.com/apple/foundationdb.git
cd foundationdb/packaging/docker

# Build the image (override FDB_VERSION if needed)
docker build --build-arg FDB_VERSION=7.4.5 \
    -t foundationdb/foundationdb:7.4.5-arm \
    --target=foundationdb .
```

Then run `cargo test` as usual.

---

## License

MIT
