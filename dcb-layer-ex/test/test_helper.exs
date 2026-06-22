ExUnit.start()

# Auto-detect Docker socket (Colima, Docker Desktop, etc.) when DOCKER_HOST is unset.
# Mirroring the same logic used by the Rust test suite.
if is_nil(System.get_env("DOCKER_HOST")) do
  candidates = [
    Path.expand("~/.colima/default/docker.sock"),
    Path.expand("~/.docker/run/docker.sock")
  ]

  case Enum.find(candidates, &File.exists?/1) do
    nil ->
      :ok

    path ->
      System.put_env("DOCKER_HOST", "unix://#{path}")
      # Colima can't bind-mount its own socket into Ryuk; disable the reaper.
      System.put_env("TESTCONTAINERS_RYUK_DISABLED", "true")
  end
end

{:ok, _} = Application.ensure_all_started(:testcontainers)
{:ok, _} = Testcontainers.start_link()

{:ok, container} =
  Testcontainers.Container.new("foundationdb/foundationdb:7.4.5")
  |> Testcontainers.Container.with_exposed_port(4500)
  |> Testcontainers.Container.with_environment("FDB_NETWORKING_MODE", "host")
  |> Testcontainers.Container.with_waiting_strategy(
    Testcontainers.LogWaitStrategy.new(~r/FDBD joined cluster\./, 60_000)
  )
  |> Testcontainers.start_container()

System.cmd("docker", ["exec", container.container_id, "fdbcli", "--exec", "configure new single ssd"])

fdb_available? = fn ->
  {out, _} = System.cmd("docker", ["exec", container.container_id, "fdbcli", "--exec", "status minimal"])
  String.contains?(out, "available")
end

Enum.reduce_while(1..60, nil, fn _, _ ->
  if fdb_available?.() do
    {:halt, :ok}
  else
    Process.sleep(1_000)
    {:cont, nil}
  end
end) == :ok || raise "FDB did not become available in time"

cluster_file = Path.join(System.tmp_dir!(), "fdb_test.cluster")
File.write!(cluster_file, "docker:docker@127.0.0.1:4500\n")
Application.put_env(:dcb_layer_ex, :fdb_cluster_file, cluster_file)
