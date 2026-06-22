defmodule DcbLayerEx.MixProject do
  use Mix.Project

  def project do
    [
      app: :dcb_layer_ex,
      version: "0.1.0",
      elixir: "~> 1.17",
      start_permanent: Mix.env() == :prod,
      deps: deps(),
      description: "Elixir bindings for dcb-layer, a DCB-compliant event store backed by FoundationDB",
      package: package()
    ]
  end

  defp package do
    [
      files: ["lib", "native/dcb_layer_nif/src", "native/dcb_layer_nif/Cargo.toml", "native/dcb_layer_nif/Cargo.lock", "native/dcb_layer_nif/.cargo", "mix.exs", "README.md"],
      licenses: ["MIT"],
      links: %{"GitHub" => "https://github.com/err0r500/dcb-layer"}
    ]
  end

  def application do
    [extra_applications: [:logger]]
  end

  defp deps do
    [
      {:rustler, "~> 0.36", runtime: false},
      {:testcontainers, "~> 1.0", only: [:test]}
    ]
  end
end
