defmodule Dcb.StoreTest do
  use ExUnit.Case

  alias Dcb.Store

  defp unique_ns, do: "test-#{System.unique_integer([:positive])}-#{System.os_time(:millisecond)}"

  setup do
    {:ok, store} = Store.open(unique_ns())
    %{store: store}
  end

  test "append returns a versionstamp", %{store: store} do
    event = %{type_name: "Foo", tags: ["k:v"], data: "{}"}
    assert {:ok, vs} = Store.append(store, [event])
    assert byte_size(vs) == 12
  end

  test "read returns appended events", %{store: store} do
    event = %{type_name: "Foo", tags: ["k:1"], data: "{}"}
    {:ok, _vs} = Store.append(store, [event])

    query = %{items: [%{types: ["Foo"], tags: ["k:1"]}]}
    assert {:ok, [stored]} = Store.read(store, query)
    assert stored.type_name == "Foo"
    assert stored.tags == ["k:1"]
    assert byte_size(stored.position) == 12
  end

  test "read_all returns all events in order", %{store: store} do
    for i <- 1..3 do
      {:ok, _} = Store.append(store, [%{type_name: "E#{i}", tags: [], data: ""}])
    end

    assert {:ok, events} = Store.read_all(store)
    assert length(events) == 3
    assert Enum.map(events, & &1.type_name) == ["E1", "E2", "E3"]
  end

  test "conditional append fails when condition violated", %{store: store} do
    event = %{type_name: "Counted", tags: ["ns:x"], data: ""}
    query = %{items: [%{types: ["Counted"], tags: ["ns:x"]}]}

    {:ok, _vs} = Store.append(store, [event])

    # Condition: no events after nil — but there is one now, so this should fail.
    cond = %{query: query, after: nil}
    assert {:error, :append_condition_failed} = Store.append(store, [event], [cond])
  end

  test "cursor round-trip", %{store: store} do
    {:ok, nil} = Store.get_cursor(store, "sub-1")

    {:ok, vs} = Store.append(store, [%{type_name: "X", tags: [], data: ""}])
    :ok = Store.set_cursor(store, "sub-1", vs)

    assert {:ok, ^vs} = Store.get_cursor(store, "sub-1")
  end

  test "watch fires after append", %{store: store} do
    :ok = Store.watch(store)
    Store.append(store, [%{type_name: "W", tags: [], data: ""}])

    assert_receive {:fdb_watch_fired}, 5_000
  end
end
