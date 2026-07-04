defmodule Dcb.Native do
  @moduledoc false

  version = Mix.Project.config()[:version]
  github_url = Mix.Project.config()[:source_url]

  use RustlerPrecompiled,
    otp_app: :dcb_layer,
    crate: "dcb_layer_nif",
    base_url: "#{github_url}/releases/download/v#{version}",
    version: version,
    nif_versions: ["2.15"],
    targets: ~w(
      x86_64-unknown-linux-gnu
      aarch64-unknown-linux-gnu
      aarch64-apple-darwin
    ),
    # Compile locally (via rustler) when explicitly requested or in dev/test.
    force_build:
      System.get_env("DCB_BUILD_NIF") in ["1", "true"] or Mix.env() in [:dev, :test],
    # Only the default fdb-7_4 feature is precompiled. FDB 7.3 users must
    # force_build with DCB_BUILD_NIF=1 and set the fdb-7_3 feature.
    features: Application.compile_env(:dcb_layer, [Dcb.Native, :features], ["fdb-7_4"])

  def dcb_store_open(_cluster_file, _namespace), do: :erlang.nif_error(:not_loaded)
  def dcb_store_append(_store, _events, _conds), do: :erlang.nif_error(:not_loaded)
  def dcb_store_read(_store, _query, _opts), do: :erlang.nif_error(:not_loaded)
  def dcb_store_read_all(_store), do: :erlang.nif_error(:not_loaded)
  def dcb_store_watch(_store, _pid), do: :erlang.nif_error(:not_loaded)
  def dcb_store_get_cursor(_store, _name), do: :erlang.nif_error(:not_loaded)
  def dcb_store_set_cursor(_store, _name, _pos), do: :erlang.nif_error(:not_loaded)
end
