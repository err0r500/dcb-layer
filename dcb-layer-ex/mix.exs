defmodule DcbLayerEx.MixProject do
  use Mix.Project

  @source_url "https://github.com/err0r500/dcb-layer"
  @version "0.2.2"

  def project do
    [
      app: :dcb_layer,
      version: @version,
      elixir: "~> 1.17",
      start_permanent: Mix.env() == :prod,
      deps: deps(),
      description:
        "Elixir bindings for dcb-layer, a DCB-compliant event store backed by FoundationDB",
      package: package(),
      docs: docs(),
      source_url: @source_url,
      homepage_url: @source_url
    ]
  end

  defp docs do
    [
      main: "readme",
      extras: ["README.md"],
      source_url: @source_url
    ]
  end

  defp package do
    [
      files: [
        "lib",
        "native/dcb_layer_nif/src",
        "native/dcb_layer_nif/Cargo.toml",
        "native/dcb_layer_nif/Cargo.lock",
        "native/dcb_layer_nif/.cargo",
        "mix.exs",
        "README.md",
        # Required by rustler_precompiled to verify downloaded binaries.
        "checksum-Elixir.Dcb.Native.exs"
      ],
      licenses: ["MIT"],
      links: %{"GitHub" => "https://github.com/err0r500/dcb-layer"}
    ]
  end

  def application do
    [extra_applications: [:logger]]
  end

  defp deps do
    [
      {:rustler_precompiled, "~> 0.8"},
      # Kept for local/force builds (DCB_BUILD_NIF=1, dev, test).
      {:rustler, "~> 0.36", optional: true, runtime: false},
      {:testcontainers, "~> 1.0", only: [:test]},
      {:ex_doc, "~> 0.34", only: :dev, runtime: false}
    ]
  end
end
