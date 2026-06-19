defmodule LibraryKataTest do
  use ExUnit.Case

  setup do
    # Each test gets its own namespace so FDB state doesn't bleed between tests.
    {:ok, store} = LibraryKata.open_store("library-test-#{System.unique_integer([:positive])}-#{System.os_time(:millisecond)}")
    %{store: store}
  end

  test "borrow succeeds for an available book", %{store: store} do
    assert :ok = LibraryKata.borrow_book(store, "isbn-1", "alice")
  end

  test "borrow succeeds for an available book (after being returned)", %{store: store} do
    book_id = "isbn-1"
    :ok = LibraryKata.borrow_book(store, book_id, "alice")
    :ok = LibraryKata.return_book(store, book_id, "alice")
    assert :ok = LibraryKata.borrow_book(store, book_id, "alice")
  end

  test "cannot borrow an already-borrowed book", %{store: store} do
    :ok = LibraryKata.borrow_book(store, "isbn-1", "alice")
    assert {:error, :book_already_borrowed} = LibraryKata.borrow_book(store, "isbn-1", "bob")
  end

  test "borrower limit of #{5} books is enforced", %{store: store} do
    for i <- 1..5, do: :ok = LibraryKata.borrow_book(store, "isbn-#{i}", "alice")
    assert {:error, :borrower_limit_reached} = LibraryKata.borrow_book(store, "isbn-6", "alice")
  end

  test "only one concurrent borrower of the same book succeeds", %{store: store} do
    results =
      1..5
      |> Enum.map(fn i ->
        Task.async(fn -> LibraryKata.borrow_book(store, "isbn-race", "borrower-#{i}") end)
      end)
      |> Task.await_many(5_000)

    assert Enum.count(results, &(&1 == :ok)) == 1
    assert Enum.count(results, &(&1 == {:error, :book_already_borrowed})) == 4
  end
end
