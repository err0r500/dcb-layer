defmodule Dcb.Store do
  @moduledoc """
  Public API for the DCB event store backed by FoundationDB.

  This module wraps the raw Rust NIF bindings with idiomatic Elixir
  defaults and option handling.

  See the [README](readme.html) for data structure shapes (event, query, condition).
  """

  alias Dcb.Native

  @doc """
  Opens a store connection scoped to `namespace`.

  ## Options

    * `:cluster_file` - path to the FDB cluster file (default: `nil`, uses the default cluster file)

  ## Example

      {:ok, store} = Dcb.Store.open("my_namespace")
      {:ok, store} = Dcb.Store.open("my_namespace", cluster_file: "/etc/foundationdb/fdb.cluster")
  """
  def open(namespace, opts \\ []) do
    cluster_file = Keyword.get(opts, :cluster_file, nil)
    Native.dcb_store_open(cluster_file, namespace)
  end

  @doc """
  Appends `events` to the store.

  Optionally accepts `conditions` for optimistic concurrency: the append
  fails if any matching events exist after the position specified in each condition.

  ## Example

      events = [%{type_name: "UserCreated", tags: ["user-1"], data: <<>>}]
      {:ok, position} = Dcb.Store.append(store, events)

      condition = %{query: %{items: [%{types: ["UserCreated"], tags: ["user-1"]}]}, after: nil}
      {:ok, position} = Dcb.Store.append(store, events, [condition])
  """
  def append(store, events, conditions \\ []) do
    Native.dcb_store_append(store, events, conditions)
  end

  @doc """
  Reads events matching `query` from the store.

  ## Options

    * `:limit` - max events to return, `0` means unlimited (default: `0`)
    * `:after` - only return events after this position (default: `nil`)
    * `:reverse` - return events in reverse order (default: `false`)

  ## Example

      query = %{items: [%{types: ["UserCreated"], tags: ["user-1"]}]}
      {:ok, events} = Dcb.Store.read(store, query)
      {:ok, events} = Dcb.Store.read(store, query, limit: 50, after: position)
  """
  def read(store, query, opts \\ []) do
    read_opts = %{
      limit: Keyword.get(opts, :limit, 0),
      after: Keyword.get(opts, :after, nil),
      reverse: Keyword.get(opts, :reverse, false)
    }

    Native.dcb_store_read(store, query, read_opts)
  end

  @doc "Returns all events in the store, in insertion order."
  def read_all(store), do: Native.dcb_store_read_all(store)

  @doc """
  Subscribes the calling process to store change notifications.

  Sends `{:dcb_store_changed, store}` to `self()` whenever new events are appended.
  The watch is one-shot; call `watch/1` again after receiving the message to re-subscribe.
  """
  def watch(store), do: Native.dcb_store_watch(store, self())

  @doc "Returns the position stored under `name` for the given named cursor."
  def get_cursor(store, name), do: Native.dcb_store_get_cursor(store, name)

  @doc "Persists `position` under `name` as a named cursor."
  def set_cursor(store, name, position), do: Native.dcb_store_set_cursor(store, name, position)
end
