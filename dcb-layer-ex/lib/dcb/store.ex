defmodule Dcb.Store do
  alias Dcb.Native

  def open(namespace, opts \\ []) do
    cluster_file = Keyword.get(opts, :cluster_file, nil)
    Native.dcb_store_open(cluster_file, namespace)
  end

  def append(store, events, conditions \\ []) do
    Native.dcb_store_append(store, events, conditions)
  end

  def read(store, query, opts \\ []) do
    read_opts = %{
      limit:   Keyword.get(opts, :limit, 0),
      after:   Keyword.get(opts, :after, nil),
      reverse: Keyword.get(opts, :reverse, false)
    }
    Native.dcb_store_read(store, query, read_opts)
  end

  def read_all(store), do: Native.dcb_store_read_all(store)

  def watch(store), do: Native.dcb_store_watch(store, self())

  def get_cursor(store, name), do: Native.dcb_store_get_cursor(store, name)

  def set_cursor(store, name, position), do: Native.dcb_store_set_cursor(store, name, position)
end
