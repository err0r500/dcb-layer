defmodule Dcb.Native do
  use Rustler, otp_app: :library_kata, crate: "dcb_layer_nif", path: "../../"

  def dcb_store_open(_cluster_file, _namespace),    do: :erlang.nif_error(:not_loaded)
  def dcb_store_append(_store, _events, _conds),    do: :erlang.nif_error(:not_loaded)
  def dcb_store_read(_store, _query, _opts),        do: :erlang.nif_error(:not_loaded)
  def dcb_store_read_all(_store),                   do: :erlang.nif_error(:not_loaded)
end
