# dcb_layer_ex

Elixir bindings for [dcb-layer](https://github.com/err0r500/dcb-layer), a DCB-compliant event store backed by FoundationDB.
The native implementation is compiled via Rustler.

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
    {:dcb_layer_ex, "~> 0.1"}
  ]
end
```

By default the NIF is compiled with `fdb-7_4`. To use 7.3, set in `config.exs`:

```elixir
config :dcb_layer_ex, Dcb.Native, features: ["fdb-7_3"]
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

# Watch for new events (sends a message to self() when new events arrive)
:ok = Dcb.Store.watch(store)

# Named cursors
{:ok, pos} = Dcb.Store.get_cursor(store, "my-consumer")
:ok        = Dcb.Store.set_cursor(store, "my-consumer", pos)
```

## License

MIT
