#![allow(dead_code)]

use std::mem::ManuallyDrop;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Once, OnceLock};

use bytes::Bytes;
use dcb_layer::{AppendCondition, Event, FdbStore, Query, QueryItem};
use foundationdb::Database;
use testcontainers::core::{ContainerPort, WaitFor};
use testcontainers::runners::AsyncRunner;
use testcontainers::{Image, ImageExt};

// ---------------------------------------------------------------------------
// FDB client library init
// ---------------------------------------------------------------------------

pub fn ensure_network() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        let _ = ManuallyDrop::new(unsafe { foundationdb::boot() });
    });
}

// ---------------------------------------------------------------------------
// FDB container image definition
// ---------------------------------------------------------------------------

const FDB_IMAGE_NAME: &str = "foundationdb/foundationdb";
const FDB_IMAGE_VERSION: &str = "7.4.5";
const FDB_PORT: ContainerPort = ContainerPort::Tcp(4500);

#[derive(Debug, Clone)]
struct FoundationDb {
    tag: String,
    env_vars: Vec<(&'static str, &'static str)>,
}

impl Default for FoundationDb {
    fn default() -> Self {
        let tag = if std::env::consts::ARCH == "aarch64" {
            format!("{}-arm", FDB_IMAGE_VERSION)
        } else {
            FDB_IMAGE_VERSION.to_string()
        };
        Self {
            tag,
            // FDB_NETWORKING_MODE=host makes fdbserver advertise 127.0.0.1 as its
            // coordinator address instead of the container IP (unreachable from macOS).
            env_vars: vec![("FDB_NETWORKING_MODE", "host")],
        }
    }
}

impl Image for FoundationDb {
    fn name(&self) -> &str {
        FDB_IMAGE_NAME
    }

    fn tag(&self) -> &str {
        &self.tag
    }

    fn ready_conditions(&self) -> Vec<WaitFor> {
        vec![WaitFor::message_on_stdout("FDBD joined cluster.")]
    }

    fn env_vars(
        &self,
    ) -> impl IntoIterator<
        Item = (
            impl Into<std::borrow::Cow<'_, str>>,
            impl Into<std::borrow::Cow<'_, str>>,
        ),
    > {
        self.env_vars.iter().map(|(k, v)| (*k, *v))
    }

    fn expose_ports(&self) -> &[ContainerPort] {
        &[FDB_PORT]
    }
}

// ---------------------------------------------------------------------------
// Container lifecycle — started once per test process
//
// A dedicated OS thread owns a single-thread Tokio runtime and the container.
// It sends the rewritten cluster file path back once setup completes, then
// parks forever so the ContainerAsync is never dropped.  This avoids both the
// nested-runtime problem (each #[tokio::test] has its own runtime) and
// cross-runtime OnceCell issues.
// ---------------------------------------------------------------------------

static CLUSTER_FILE: OnceLock<String> = OnceLock::new();

/// Point DOCKER_HOST at Colima's socket when it exists and DOCKER_HOST is unset.
fn detect_docker_host() {
    if std::env::var_os("DOCKER_HOST").is_some() {
        return;
    }
    let candidates = [
        dirs_or_home(".colima/default/docker.sock"),
        dirs_or_home(".docker/run/docker.sock"),
    ];
    for path in candidates.into_iter().flatten() {
        if path.exists() {
            let _ = std::env::set_var("DOCKER_HOST", format!("unix://{}", path.display()));
            return;
        }
    }
}

fn dirs_or_home(rel: &str) -> Option<std::path::PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(std::path::Path::new(&home).join(rel))
}

pub fn fdb_cluster_file() -> &'static str {
    CLUSTER_FILE.get_or_init(|| {
        detect_docker_host();

        let lock_path = std::env::temp_dir().join("fdb_test.lock");
        let cluster_path = std::env::temp_dir().join("fdb_test.cluster");

        // Only one process across all concurrent test binaries starts the container.
        // The lock file is removed after startup, so the next `cargo test` invocation
        // always gets a fresh starter that cleans up the previous run.
        let is_starter = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock_path)
            .is_ok();

        if !is_starter {
            eprintln!("[fdb] waiting for another process to start the container...");
            let deadline =
                std::time::Instant::now() + std::time::Duration::from_secs(60);
            while !cluster_path.exists() {
                assert!(
                    std::time::Instant::now() < deadline,
                    "timed out waiting for FDB cluster file"
                );
                std::thread::sleep(std::time::Duration::from_millis(200));
            }
            eprintln!("[fdb] cluster file appeared: {}", cluster_path.display());
            return cluster_path.to_str().unwrap().to_string();
        }

        // Starter: stop any container left over from a previous run and remove stale files.
        eprintln!("[fdb] cleaning up previous run...");
        let _ = std::fs::remove_file(&cluster_path);
        if let Ok(out) = Command::new("docker")
            .args(["ps", "-q", "--filter", "publish=4500"])
            .output()
        {
            for id in String::from_utf8_lossy(&out.stdout).split_whitespace() {
                eprintln!("[fdb] stopping old container {id}");
                let _ = Command::new("docker").args(["stop", id]).output();
            }
        }

        eprintln!("[fdb] starting container (DOCKER_HOST={:?})...", std::env::var("DOCKER_HOST").ok());

        let cluster_path_clone = cluster_path.clone();
        let (tx, rx) = std::sync::mpsc::channel::<String>();

        std::thread::Builder::new()
            .name("fdb-container".into())
            .spawn(move || {
                tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .unwrap()
                    .block_on(async move {
                        eprintln!("[fdb] pulling/starting image...");
                        let container = FoundationDb::default()
                            .with_mapped_port(4500u16, FDB_PORT)
                            .start()
                            .await
                            .expect("failed to start FDB container");
                        eprintln!("[fdb] container started: {}", container.id());

                        let container_id = container.id().to_string();
                        eprintln!("[fdb] running fdbcli configure...");
                        let out = Command::new("docker")
                            .args([
                                "exec",
                                &container_id,
                                "fdbcli",
                                "--exec",
                                "configure new single ssd",
                            ])
                            .output()
                            .expect("failed to run fdbcli configure");
                        let stdout = String::from_utf8_lossy(&out.stdout);
                        let stderr = String::from_utf8_lossy(&out.stderr);
                        eprintln!("[fdb] fdbcli stdout: {stdout}");
                        eprintln!("[fdb] fdbcli stderr: {stderr}");
                        assert!(
                            stdout.contains("Database created"),
                            "fdbcli configure failed (stdout: {stdout}, stderr: {stderr})"
                        );

                        // Wait until FDB storage is ready to serve reads.
                        eprintln!("[fdb] waiting for storage to become available...");
                        for attempt in 0..60 {
                            let s = Command::new("docker")
                                .args(["exec", &container_id, "fdbcli", "--exec", "status minimal"])
                                .output()
                                .expect("failed to run fdbcli status");
                            let s_out = String::from_utf8_lossy(&s.stdout);
                            eprintln!("[fdb] status attempt {attempt}: {s_out}");
                            if s_out.contains("available") {
                                break;
                            }
                            assert!(attempt < 59, "FDB did not become available in time");
                            std::thread::sleep(std::time::Duration::from_millis(500));
                        }

                        std::fs::write(&cluster_path_clone, "docker:docker@127.0.0.1:4500\n")
                            .expect("failed to write cluster file");
                        let _ = std::fs::remove_file(std::env::temp_dir().join("fdb_test.lock"));
                        eprintln!("[fdb] ready — cluster file written: {}", cluster_path_clone.display());

                        tx.send(cluster_path_clone.to_str().unwrap().to_string()).unwrap();

                        let _live = container;
                        std::future::pending::<()>().await
                    });
            })
            .expect("failed to spawn fdb-container thread");

        rx.recv().expect("fdb-container thread exited before sending cluster file path")
    })
}

// ---------------------------------------------------------------------------
// Store factory
// ---------------------------------------------------------------------------

pub fn make_store(prefix: &str) -> FdbStore {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    ensure_network();
    let cluster_file = fdb_cluster_file();
    let db = Database::new(Some(cluster_file))
        .expect("failed to open FDB database");
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();
    let ns = format!("{prefix}_{ts}_{}", COUNTER.fetch_add(1, Ordering::Relaxed));
    FdbStore::new(db, ns)
}

// ---------------------------------------------------------------------------
// Event / query builders shared across test files
// ---------------------------------------------------------------------------

pub fn event(type_name: &str) -> Event {
    Event { type_name: type_name.into(), tags: vec![], data: Bytes::new() }
}

pub fn event_with_data(type_name: &str, tags: &[&str], data: &[u8]) -> Event {
    Event {
        type_name: type_name.into(),
        tags: tags.iter().map(|&s| s.into()).collect(),
        data: Bytes::copy_from_slice(data),
    }
}

pub fn tagged(type_name: &str, tags: &[&str]) -> Event {
    Event {
        type_name: type_name.into(),
        tags: tags.iter().map(|&s| s.into()).collect(),
        data: Bytes::new(),
    }
}

pub fn type_query(types: &[&str]) -> Query {
    Query {
        items: vec![QueryItem {
            types: types.iter().map(|s| s.to_string()).collect(),
            tags: vec![],
        }],
    }
}

pub fn tag_query(tags: &[&str]) -> Query {
    Query {
        items: vec![QueryItem {
            types: vec![],
            tags: tags.iter().map(|s| s.to_string()).collect(),
        }],
    }
}

pub fn type_tag_query(types: &[&str], tags: &[&str]) -> Query {
    Query {
        items: vec![QueryItem {
            types: types.iter().map(|s| s.to_string()).collect(),
            tags: tags.iter().map(|s| s.to_string()).collect(),
        }],
    }
}

pub fn or_query(items: &[(&[&str], &[&str])]) -> Query {
    Query {
        items: items
            .iter()
            .map(|(types, tags)| QueryItem {
                types: types.iter().map(|s| s.to_string()).collect(),
                tags: tags.iter().map(|s| s.to_string()).collect(),
            })
            .collect(),
    }
}

pub fn type_condition(types: &[&str]) -> AppendCondition {
    AppendCondition { query: type_query(types), after: None }
}
