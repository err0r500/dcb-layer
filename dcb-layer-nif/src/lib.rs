use std::sync::Once;

use dcb_layer::{AppendCondition, Event, FdbStore, Query, QueryItem, ReadOptions, Versionstamp};
use once_cell::sync::Lazy;
use rustler::types::tuple::make_tuple;
use rustler::{Binary, Encoder, Env, LocalPid, NifResult, OwnedBinary, OwnedEnv, ResourceArc, Term};
use tokio::runtime::Runtime;

mod atoms {
    rustler::atoms! {
        ok,
        error,
        // event / stored-event field keys
        type_name,
        tags,
        data,
        position,
        // query field keys
        items,
        types,
        // condition / opts field keys
        query,
        after,
        limit,
        reverse,
        // error atoms
        empty_events,
        append_condition_failed,
        invalid_query,
        missing_event_type,
        batch_too_large,
        too_many_tags,
        reserved_tag,
        all_ff_key,
        tuple_encode,
        tuple_decode,
        event_not_found,
        fdb_error,
        // nil sentinel
        nil,
        // subscription
        fdb_watch_fired,
    }
}

static RUNTIME: Lazy<Runtime> = Lazy::new(|| Runtime::new().expect("tokio runtime"));
static FDB_BOOT: Once = Once::new();

pub struct FdbStoreResource(FdbStore);

// FDB's C library is thread-safe; FdbStore wraps Database (Arc-backed) + String.
unsafe impl Send for FdbStoreResource {}
unsafe impl Sync for FdbStoreResource {}

#[allow(non_local_definitions)]
fn load(env: Env, _info: Term) -> bool {
    let _ = rustler::resource!(FdbStoreResource, env);

    // Boot the FDB network thread once per process; forget the guard so it
    // is never dropped and the network stays alive for the process lifetime.
    FDB_BOOT.call_once(|| {
        let guard = unsafe { foundationdb::boot() };
        std::mem::forget(guard);
    });

    Lazy::force(&RUNTIME);
    true
}

// ---------------------------------------------------------------------------
// Decode helpers (Elixir term → Rust type)
// ---------------------------------------------------------------------------

fn is_nil(term: Term) -> bool {
    term.is_atom() && term.atom_to_string().map(|s| s == "nil").unwrap_or(false)
}

fn decode_vs(term: Term) -> NifResult<Versionstamp> {
    let bin: Binary = term.decode()?;
    let slice = bin.as_slice();
    if slice.len() != 12 {
        return Err(rustler::Error::BadArg);
    }
    let mut vs = [0u8; 12];
    vs.copy_from_slice(slice);
    Ok(vs)
}

fn decode_opt_vs(term: Term) -> NifResult<Option<Versionstamp>> {
    if is_nil(term) { Ok(None) } else { Ok(Some(decode_vs(term)?)) }
}

fn decode_event(term: Term) -> NifResult<Event> {
    let env = term.get_env();
    let type_name: String = term.map_get(atoms::type_name().encode(env))?.decode()?;
    let tags: Vec<String> = term.map_get(atoms::tags().encode(env))?.decode()?;
    // Copy binary out of Erlang heap before returning owned Event
    let data_bin: Binary = term.map_get(atoms::data().encode(env))?.decode()?;
    let data = data_bin.as_slice().to_vec();
    Ok(Event::new(type_name, tags, data))
}

fn decode_query_item(term: Term) -> NifResult<QueryItem> {
    let env = term.get_env();
    let types: Vec<String> = term.map_get(atoms::types().encode(env))?.decode()?;
    let tags: Vec<String> = term.map_get(atoms::tags().encode(env))?.decode()?;
    Ok(QueryItem { types, tags })
}

fn decode_query(term: Term) -> NifResult<Query> {
    let env = term.get_env();
    let raw: Vec<Term> = term.map_get(atoms::items().encode(env))?.decode()?;
    let items = raw.into_iter().map(decode_query_item).collect::<NifResult<_>>()?;
    Ok(Query { items })
}

fn decode_condition(term: Term) -> NifResult<AppendCondition> {
    let env = term.get_env();
    let query = decode_query(term.map_get(atoms::query().encode(env))?)?;
    let after = decode_opt_vs(term.map_get(atoms::after().encode(env))?)?;
    Ok(AppendCondition { query, after })
}

fn decode_read_opts(term: Term) -> NifResult<ReadOptions> {
    let env = term.get_env();
    let limit: usize = term.map_get(atoms::limit().encode(env))?.decode()?;
    let after = decode_opt_vs(term.map_get(atoms::after().encode(env))?)?;
    let reverse: bool = term.map_get(atoms::reverse().encode(env))?.decode()?;
    Ok(ReadOptions { limit, after, reverse })
}

// ---------------------------------------------------------------------------
// Encode helpers (Rust type → Elixir term)
// ---------------------------------------------------------------------------

fn vs_to_term<'a>(env: Env<'a>, vs: &Versionstamp) -> Term<'a> {
    let mut bin = OwnedBinary::new(12).unwrap();
    bin.as_mut_slice().copy_from_slice(vs);
    bin.release(env).encode(env)
}

fn bytes_to_term<'a>(env: Env<'a>, data: &[u8]) -> Term<'a> {
    let mut bin = OwnedBinary::new(data.len()).unwrap();
    bin.as_mut_slice().copy_from_slice(data);
    bin.release(env).encode(env)
}

fn encode_stored_event<'a>(env: Env<'a>, se: dcb_layer::StoredEvent) -> Term<'a> {
    let tags: Vec<Term> = se.event.tags.iter().map(|t| t.encode(env)).collect();
    Term::map_new(env)
        .map_put(atoms::type_name().encode(env), se.event.type_name.encode(env)).unwrap()
        .map_put(atoms::tags().encode(env), tags.encode(env)).unwrap()
        .map_put(atoms::data().encode(env), bytes_to_term(env, &se.event.data)).unwrap()
        .map_put(atoms::position().encode(env), vs_to_term(env, &se.position)).unwrap()
}

fn encode_dcb_error<'a>(env: Env<'a>, e: dcb_layer::Error) -> Term<'a> {
    use dcb_layer::Error::*;
    match e {
        EmptyEvents => atoms::empty_events().encode(env),
        AppendConditionFailed => atoms::append_condition_failed().encode(env),
        InvalidQuery => atoms::invalid_query().encode(env),
        MissingEventType => atoms::missing_event_type().encode(env),
        BatchTooLarge => atoms::batch_too_large().encode(env),
        TooManyTags => atoms::too_many_tags().encode(env),
        ReservedTag => atoms::reserved_tag().encode(env),
        AllFfKey => atoms::all_ff_key().encode(env),
        Fdb(e) => (atoms::fdb_error(), e.code()).encode(env),
        TupleEncode(s) => (atoms::tuple_encode(), s).encode(env),
        TupleDecode(s) => (atoms::tuple_decode(), s).encode(env),
        EventNotFound(s) => (atoms::event_not_found(), s).encode(env),
    }
}

fn ok<'a>(env: Env<'a>, val: Term<'a>) -> Term<'a> {
    (atoms::ok(), val).encode(env)
}

fn err<'a>(env: Env<'a>, reason: Term<'a>) -> Term<'a> {
    (atoms::error(), reason).encode(env)
}

// ---------------------------------------------------------------------------
// NIFs
// ---------------------------------------------------------------------------

/// Open a FoundationDB-backed store.
///
/// `cluster_file` — path to the cluster file, or `nil` to use the default.
/// Returns `{:ok, store_ref}` or `{:error, {:fdb_error, code}}`.
#[rustler::nif(schedule = "DirtyIo")]
fn dcb_store_open<'a>(env: Env<'a>, cluster_file: Term<'a>, namespace: String) -> Term<'a> {
    let db = if is_nil(cluster_file) {
        foundationdb::Database::default()
    } else {
        match cluster_file.decode::<String>() {
            Ok(path) => foundationdb::Database::from_path(&path),
            Err(_) => return err(env, "invalid_cluster_file".encode(env)),
        }
    };
    match db {
        Ok(db) => ok(env, ResourceArc::new(FdbStoreResource(FdbStore::new(db, namespace))).encode(env)),
        Err(e) => err(env, (atoms::fdb_error(), e.code()).encode(env)),
    }
}

/// Append events atomically.
///
/// `events` — list of `%{type_name: binary, tags: [binary], data: binary}`.
/// `conditions` — list of `%{query: query, after: nil | <<12 bytes>>}`.
/// Returns `{:ok, <<12 bytes>>}` (versionstamp of the last event) or `{:error, reason}`.
#[rustler::nif(schedule = "DirtyIo")]
fn dcb_store_append<'a>(
    env: Env<'a>,
    store: ResourceArc<FdbStoreResource>,
    events_term: Term<'a>,
    conditions_term: Term<'a>,
) -> Term<'a> {
    let events = match events_term
        .decode::<Vec<Term>>()
        .and_then(|ts| ts.into_iter().map(decode_event).collect::<NifResult<Vec<_>>>())
    {
        Ok(v) => v,
        Err(_) => return err(env, "invalid_events".encode(env)),
    };
    let conditions = match conditions_term
        .decode::<Vec<Term>>()
        .and_then(|ts| ts.into_iter().map(decode_condition).collect::<NifResult<Vec<_>>>())
    {
        Ok(v) => v,
        Err(_) => return err(env, "invalid_conditions".encode(env)),
    };

    match RUNTIME.block_on(store.0.append(events, conditions)) {
        Ok(vs) => ok(env, vs_to_term(env, &vs)),
        Err(e) => err(env, encode_dcb_error(env, e)),
    }
}

/// Read events matching a query.
///
/// `query` — `%{items: [%{types: [binary], tags: [binary]}]}`.
/// `opts` — `%{limit: non_neg_integer, after: nil | <<12 bytes>>, reverse: boolean}`.
/// Returns `{:ok, [stored_event]}` or `{:error, reason}`.
#[rustler::nif(schedule = "DirtyIo")]
fn dcb_store_read<'a>(
    env: Env<'a>,
    store: ResourceArc<FdbStoreResource>,
    query_term: Term<'a>,
    opts_term: Term<'a>,
) -> Term<'a> {
    let query = match decode_query(query_term) {
        Ok(q) => q,
        Err(_) => return err(env, "invalid_query".encode(env)),
    };
    let opts = match decode_read_opts(opts_term) {
        Ok(o) => o,
        Err(_) => return err(env, "invalid_opts".encode(env)),
    };
    match RUNTIME.block_on(store.0.read(query, Some(opts))) {
        Ok(events) => {
            let terms: Vec<Term> = events.into_iter().map(|se| encode_stored_event(env, se)).collect();
            ok(env, terms.encode(env))
        }
        Err(e) => err(env, encode_dcb_error(env, e)),
    }
}

/// Read all events in insertion order (no index used).
///
/// Returns `{:ok, [stored_event]}` or `{:error, reason}`.
#[rustler::nif(schedule = "DirtyIo")]
fn dcb_store_read_all<'a>(env: Env<'a>, store: ResourceArc<FdbStoreResource>) -> Term<'a> {
    match RUNTIME.block_on(store.0.read_all()) {
        Ok(events) => {
            let terms: Vec<Term> = events.into_iter().map(|se| encode_stored_event(env, se)).collect();
            ok(env, terms.encode(env))
        }
        Err(e) => err(env, encode_dcb_error(env, e)),
    }
}

/// Register a one-shot watch on the namespace sentinel key.
///
/// Spawns a tokio task that parks until the sentinel changes (i.e. any append in this
/// namespace), then sends `{:fdb_watch_fired}` to `pid`. Returns `:ok` immediately.
/// If `pid` is dead when the watch fires, the message is silently discarded.
#[rustler::nif(schedule = "DirtyIo")]
fn dcb_store_watch<'a>(
    env: Env<'a>,
    store: ResourceArc<FdbStoreResource>,
    pid_term: Term<'a>,
) -> Term<'a> {
    let pid: LocalPid = match pid_term.decode() {
        Ok(p) => p,
        Err(_) => return err(env, "invalid_pid".encode(env)),
    };
    let store_clone = store.0.clone();
    RUNTIME.spawn(async move {
        store_clone.wait_for_sentinel_change().await;
        let mut msg_env = OwnedEnv::new();
        let _ = msg_env.send_and_clear(&pid, |env| {
            make_tuple(env, &[atoms::fdb_watch_fired().encode(env)])
        });
    });
    atoms::ok().encode(env)
}

/// Read the durable cursor for a named subscription.
///
/// Returns `{:ok, <<12 bytes>>}` or `{:ok, nil}` if no cursor exists yet.
#[rustler::nif(schedule = "DirtyIo")]
fn dcb_store_get_cursor<'a>(
    env: Env<'a>,
    store: ResourceArc<FdbStoreResource>,
    name: String,
) -> Term<'a> {
    match RUNTIME.block_on(store.0.get_cursor(&name)) {
        Ok(None) => ok(env, atoms::nil().encode(env)),
        Ok(Some(vs)) => ok(env, vs_to_term(env, &vs)),
        Err(e) => err(env, encode_dcb_error(env, e)),
    }
}

/// Persist the cursor for a named subscription.
///
/// `position` must be `<<12 bytes>>`. Returns `:ok` or `{:error, reason}`.
#[rustler::nif(schedule = "DirtyIo")]
fn dcb_store_set_cursor<'a>(
    env: Env<'a>,
    store: ResourceArc<FdbStoreResource>,
    name: String,
    position_term: Term<'a>,
) -> Term<'a> {
    let position = match decode_vs(position_term) {
        Ok(vs) => vs,
        Err(_) => return err(env, "invalid_position".encode(env)),
    };
    match RUNTIME.block_on(store.0.set_cursor(&name, position)) {
        Ok(()) => atoms::ok().encode(env),
        Err(e) => err(env, encode_dcb_error(env, e)),
    }
}

rustler::init!( "Elixir.Dcb.Native", load = load);
