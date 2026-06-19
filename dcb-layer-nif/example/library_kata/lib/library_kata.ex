# defmodule BookReturnedEvent do
#   def new(book_id, borrower_id) do
#     %{
#       type_name: "BookReturned",
#       tags: [book_id_tag(book_id), borrower_id_tag(borrower_id)],
#       data: JSON.encode!(%{book_id: book_id, borrower_id: borrower_id})
#     }
#   end
# end

defmodule LibraryKata do
  alias Dcb.Native

  @max_books 5

  def open_store(namespace \\ "library"), do: Native.dcb_store_open(nil, namespace)

  def return_book(store, book_id, borrower_id) do
    {:ok, _vs} =
      Native.dcb_store_append(
        store,
        [
          %{
            type_name: "BookReturned",
            tags: [book_id_tag(book_id), borrower_id_tag(borrower_id)],
            data: JSON.encode!(%{book_id: book_id, borrower_id: borrower_id})
          }
        ],
        []
      )

    :ok
  end

  def borrow_book(store, book_id, borrower_id), do: do_borrow(store, book_id, borrower_id)

  defp do_borrow(store, book_id, borrower_id) do
    book_query = %{
      items: [%{types: ["BookBorrowed", "BookReturned"], tags: [book_id_tag(book_id)]}]
    }

    with {:ok, last_book_pos} <- check_book_available(store, book_query),
         :ok <- check_borrower_limit(store, borrower_id) do
      case Native.dcb_store_append(
             store,
             [
               %{
                 type_name: "BookBorrowed",
                 tags: [book_id_tag(book_id), borrower_id_tag(borrower_id)],
                 data: JSON.encode!(%{book_id: book_id, borrower_id: borrower_id})
               }
             ],
             [%{query: book_query, after: last_book_pos}]
           ) do
        {:ok, _vs} -> :ok
        {:error, :append_condition_failed} -> do_borrow(store, book_id, borrower_id)
        {:error, _} = err -> err
      end
    end
  end

  # Returns {:ok, last_position | nil} when the book is available.
  # Reads the single most-recent event (reverse + limit 1); if it's a
  # BookBorrowed the book is currently out; anything else means it's free.
  defp check_book_available(store, query) do
    case Native.dcb_store_read(store, query, %{limit: 1, after: nil, reverse: true}) do
      {:ok, [%{type_name: "BookBorrowed"}]} -> {:error, :book_already_borrowed}
      {:ok, [%{position: pos}]} -> {:ok, pos}
      {:ok, []} -> {:ok, nil}
      {:error, _} = err -> err
    end
  end

  # Replay all borrow/return events for this borrower to count currently held books.
  defp check_borrower_limit(store, borrower_id) do
    query = %{
      items: [%{types: ["BookBorrowed", "BookReturned"], tags: [borrower_id_tag(borrower_id)]}]
    }

    opts = %{limit: 0, after: nil, reverse: false}

    with {:ok, events} <- Native.dcb_store_read(store, query, opts) do
      held =
        Enum.reduce(events, %{}, fn
          %{type_name: "BookBorrowed", data: d}, acc -> Map.put(acc, decode_book_id(d), true)
          %{type_name: "BookReturned", data: d}, acc -> Map.delete(acc, decode_book_id(d))
        end)

      if map_size(held) >= @max_books, do: {:error, :borrower_limit_reached}, else: :ok
    end
  end

  defp decode_book_id(data), do: data |> JSON.decode!() |> Map.fetch!("book_id")

  defp book_id_tag(book_id), do: "book_id:#{book_id}"
  defp borrower_id_tag(borrower_id), do: "borrower_id:#{borrower_id}"
end
