defmodule Dcb.Native do
  @moduledoc false
  use Rustler,
    otp_app: :dcb_layer,
    crate: "dcb_layer_nif",
    features: Application.compile_env(:dcb_layer, [Dcb.Native, :features], ["fdb-7_4"])

  def dcb_store_open(_cluster_file, _namespace), do: :erlang.nif_error(:not_loaded)
  def dcb_store_append(_store, _events, _conds), do: :erlang.nif_error(:not_loaded)
  def dcb_store_read(_store, _query, _opts), do: :erlang.nif_error(:not_loaded)
  def dcb_store_read_all(_store), do: :erlang.nif_error(:not_loaded)
  def dcb_store_watch(_store, _pid), do: :erlang.nif_error(:not_loaded)
  def dcb_store_get_cursor(_store, _name), do: :erlang.nif_error(:not_loaded)
  def dcb_store_set_cursor(_store, _name, _pos), do: :erlang.nif_error(:not_loaded)
end
