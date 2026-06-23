# dcb_layer

Elixir bindings for [dcb-layer](https://github.com/err0r500/dcb-layer), a [DCB](https://dcb.events)-compliant event store backed by FoundationDB.
The native implementation is compiled via Rustler.

DCB (Dynamic Consistency Boundary) is an event-sourcing approach that allows multiple aggregates to be appended atomically with per-append consistency conditions, without relying on fixed aggregate boundaries.

## Requirements

- Rust toolchain (the NIF is compiled from source on `mix deps.get`)
  ```sh
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
  ```
- [FoundationDB](https://github.com/apple/foundationdb/releases) client library (7.3 or 7.4) installed on the system
- A running FoundationDB cluster

## Installation

```elixir
def deps do
  [
    {:dcb_layer, "~> 0.1"}
  ]
end
```

By default the NIF is compiled with `fdb-7_4`. To use 7.3, set in `config.exs`:

```elixir
config :dcb_layer, Dcb.Native, features: ["fdb-7_3"]
```

## Data structures

### Event

```elixir
%{
  type_name: String.t(),   # e.g. "UserCreated"
  tags:      [String.t()], # used to scope/filter events, e.g. ["user-42"]
  data:      binary()      # arbitrary payload
}
```

### Query

A query is a list of items; an event matches if it satisfies **any** item.
Each item filters by `types` (OR) and `tags` (all must be present):

```elixir
%{
  items: [
    %{types: ["UserCreated", "UserUpdated"], tags: ["user-42"]}
  ]
}
```

### Condition

Used for optimistic concurrency in `append/3`. The append fails if any
matching events exist **after** the given position:

```elixir
%{
  query: %{items: [%{types: ["UserCreated"], tags: ["user-42"]}]},
  after: nil  # nil means "from the beginning"
}
```

## Usage

```elixir
# Open a store scoped to a namespace
{:ok, store} = Dcb.Store.open("my_namespace")

# With an explicit cluster file
{:ok, store} = Dcb.Store.open("my_namespace", cluster_file: "/etc/foundationdb/fdb.cluster")

# Append events
events = [%{type_name: "UserCreated", tags: ["user-1"], data: <<>>}]
{:ok, position} = Dcb.Store.append(store, events)

# Append with an optimistic-concurrency condition
condition = %{query: %{items: [%{types: ["UserCreated"], tags: ["user-1"]}]}, after: nil}
{:ok, position} = Dcb.Store.append(store, events, [condition])

# Read events matching a query
query = %{items: [%{types: ["UserCreated"], tags: []}]}
{:ok, events} = Dcb.Store.read(store, query)
{:ok, events} = Dcb.Store.read(store, query, limit: 100, after: position, reverse: false)

# Read all events
{:ok, events} = Dcb.Store.read_all(store)

# Watch for new events — sends {:dcb_store_changed, store} to self() on change
:ok = Dcb.Store.watch(store)

# Named cursors (persist a read position across restarts)
{:ok, pos} = Dcb.Store.get_cursor(store, "my-consumer")
:ok        = Dcb.Store.set_cursor(store, "my-consumer", pos)
```

## License

MIT
