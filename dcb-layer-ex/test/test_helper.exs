ExUnit.start()

# Mirror the Rust test suite's Docker host detection: set DOCKER_HOST to the
# Colima socket when it exists and DOCKER_HOST is not already set.
# Colima can't bind-mount its own socket into Ryuk, so disable the reaper too.
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
      System.put_env("TESTCONTAINERS_RYUK_DISABLED", "true")
  end
end

{:ok, _} = Application.ensure_all_started(:testcontainers)
{:ok, _} = Testcontainers.start_link()

# Stop any leftover container from a previous run holding port 4500
# (mirrors the Rust cleanup logic; Ryuk is disabled on Colima so nothing auto-cleans).
{ids, _} = System.cmd("docker", ["ps", "-q", "--filter", "publish=4500"])
ids |> String.split("\n", trim: true) |> Enum.each(&System.cmd("docker", ["stop", &1]))

{:ok, container} =
  Testcontainers.Container.new("foundationdb/foundationdb:7.4.5")
  |> Testcontainers.Container.with_fixed_port(4500, 4500)
  |> Testcontainers.Container.with_environment("FDB_NETWORKING_MODE", "host")
  |> Testcontainers.Container.with_waiting_strategy(
    Testcontainers.LogWaitStrategy.new(~r/FDBD joined cluster\./, 60_000)
  )
  |> Testcontainers.start_container()

{_out, 0} =
  System.cmd("docker", [
    "exec",
    container.container_id,
    "fdbcli",
    "--exec",
    "configure new single ssd"
  ])

# Poll until FDB storage is available, mirroring the Rust fdbcli status loop.
fdb_available? = fn ->
  {out, _} =
    System.cmd("docker", ["exec", container.container_id, "fdbcli", "--exec", "status minimal"])

  String.contains?(out, "available")
end

Enum.reduce_while(1..60, nil, fn attempt, _ ->
  if fdb_available?.() do
    {:halt, :ok}
  else
    if attempt == 60, do: raise("FDB did not become available in time")
    Process.sleep(500)
    {:cont, nil}
  end
end)

cluster_file = Path.join(System.tmp_dir!(), "fdb_test.cluster")
File.write!(cluster_file, "docker:docker@127.0.0.1:4500\n")
Application.put_env(:dcb_layer_ex, :fdb_cluster_file, cluster_file)
