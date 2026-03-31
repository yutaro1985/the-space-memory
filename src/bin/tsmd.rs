use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use clap::Parser;

use the_space_memory::config;
use the_space_memory::daemon;
use the_space_memory::daemon_protocol::{read_request, write_response};
use the_space_memory::db;
use the_space_memory::status;

/// Global shutdown flag for signal handlers.
static SHUTDOWN: AtomicBool = AtomicBool::new(false);

extern "C" fn signal_handler(_sig: libc::c_int) {
    SHUTDOWN.store(true, Ordering::SeqCst);
}

#[derive(Parser)]
#[command(name = "tsmd", version, about = "The Space Memory daemon")]
struct Args {
    /// UNIX socket path
    #[arg(long)]
    socket: Option<PathBuf>,

    /// Database path
    #[arg(long)]
    db: Option<PathBuf>,

    /// Skip embedder startup
    #[arg(long)]
    no_embedder: bool,

    /// Skip watcher startup
    #[arg(long)]
    no_watcher: bool,
}

fn main() -> Result<()> {
    let args = Args::parse();

    let socket_path = args.socket.unwrap_or_else(config::daemon_socket_path);
    let db_path = args.db.unwrap_or_else(config::db_path);
    let project_root = config::project_root();

    // Open DB connection
    let conn = db::get_connection(&db_path)
        .context(format!("Failed to open DB at {}", db_path.display()))?;
    let conn = Arc::new(Mutex::new(conn));

    // Clean up stale socket
    if socket_path.exists() {
        std::fs::remove_file(&socket_path)?;
    }

    // Bind listener
    let listener = UnixListener::bind(&socket_path)
        .context(format!("Failed to bind socket {}", socket_path.display()))?;
    listener.set_nonblocking(true)?;

    // Write PID file
    let pid = std::process::id();
    let pid_path = config::daemon_pid_path();
    std::fs::write(&pid_path, pid.to_string()).context("Failed to write PID file")?;

    // Update status
    let data_dir = config::data_dir();
    let socket_str = socket_path.to_string_lossy().to_string();
    status::update(&data_dir, |s| {
        s.daemon = Some(status::DaemonStatus {
            started_at: chrono::Utc::now().to_rfc3339(),
            pid,
            socket: socket_str.clone(),
        });
    });

    eprintln!("tsmd: listening on {} (PID {pid})", socket_path.display());

    // Start child processes
    let mut embedder_child: Option<Child> = if !args.no_embedder {
        match start_child("tsm-embedder", &[("TSM_EMBEDDER_IDLE_TIMEOUT", "0")]) {
            Ok(child) => {
                eprintln!("tsmd: embedder started (PID {})", child.id());
                Some(child)
            }
            Err(e) => {
                eprintln!("tsmd: warning: failed to start embedder: {e}");
                None
            }
        }
    } else {
        eprintln!("tsmd: embedder disabled (--no-embedder)");
        None
    };

    let mut watcher_child: Option<Child> = if !args.no_watcher {
        match start_child("tsm-watcher", &[]) {
            Ok(child) => {
                eprintln!("tsmd: watcher started (PID {})", child.id());
                Some(child)
            }
            Err(e) => {
                eprintln!("tsmd: warning: failed to start watcher: {e}");
                None
            }
        }
    } else {
        eprintln!("tsmd: watcher disabled (--no-watcher)");
        None
    };

    // Install signal handlers
    unsafe {
        libc::signal(
            libc::SIGTERM,
            signal_handler as *const () as libc::sighandler_t,
        );
        libc::signal(
            libc::SIGINT,
            signal_handler as *const () as libc::sighandler_t,
        );
    }

    let mut embedder_restarts = 0u32;
    let mut watcher_restarts = 0u32;
    const MAX_CHILD_RESTARTS: u32 = 3;

    // Accept loop
    while !SHUTDOWN.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((mut stream, _)) => {
                let conn = Arc::clone(&conn);
                let project_root = project_root.clone();

                std::thread::spawn(move || {
                    if let Err(e) = handle_client(&mut stream, &conn, &project_root) {
                        eprintln!("tsmd: client error: {e}");
                    }
                });
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                maybe_restart_child(
                    "embedder",
                    "tsm-embedder",
                    &[("TSM_EMBEDDER_IDLE_TIMEOUT", "0")],
                    &mut embedder_child,
                    &mut embedder_restarts,
                    MAX_CHILD_RESTARTS,
                );
                maybe_restart_child(
                    "watcher",
                    "tsm-watcher",
                    &[],
                    &mut watcher_child,
                    &mut watcher_restarts,
                    MAX_CHILD_RESTARTS,
                );
                std::thread::sleep(std::time::Duration::from_millis(100));
            }
            Err(e) => {
                eprintln!("tsmd: fatal accept error: {e}");
                break;
            }
        }
    }

    // Cleanup
    eprintln!("tsmd: shutting down");
    stop_child("embedder", embedder_child);
    stop_child("watcher", watcher_child);

    let _ = std::fs::remove_file(&socket_path);
    let _ = std::fs::remove_file(&pid_path);
    status::update(&data_dir, |s| {
        s.daemon = None;
    });

    Ok(())
}

// ─── Child process management ─────────────────────────────────────

/// Find a sibling binary in the same directory as the current executable.
fn sibling_binary(name: &str) -> Result<PathBuf> {
    let exe_dir = std::env::current_exe()?
        .parent()
        .context("executable has no parent directory")?
        .to_path_buf();
    Ok(exe_dir.join(name))
}

/// Start a child process by binary name, with optional environment variables.
fn start_child(binary: &str, env_vars: &[(&str, &str)]) -> Result<Child> {
    let bin_path = sibling_binary(binary)?;

    // Clean up stale embedder socket if starting the embedder
    if binary == "tsm-embedder" {
        let embedder_socket = Path::new(config::SOCKET_PATH);
        if embedder_socket.exists() {
            if let Err(e) = std::fs::remove_file(embedder_socket) {
                eprintln!("tsmd: warning: could not remove stale embedder socket: {e}");
            }
        }
    }

    let mut cmd = Command::new(&bin_path);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit());
    for &(k, v) in env_vars {
        cmd.env(k, v);
    }

    cmd.spawn()
        .context(format!("Failed to spawn {binary}"))
}

/// Check if a child has exited and restart it within the retry limit.
fn maybe_restart_child(
    label: &str,
    binary: &str,
    env_vars: &[(&str, &str)],
    child: &mut Option<Child>,
    restarts: &mut u32,
    max: u32,
) {
    let exited = match child {
        Some(c) => matches!(c.try_wait(), Ok(Some(_))),
        None => return,
    };

    if !exited {
        return;
    }

    if *restarts >= max {
        eprintln!("tsmd: {label} crashed {max} times, giving up");
        *child = None;
        return;
    }

    *restarts += 1;
    eprintln!("tsmd: {label} exited, restarting ({restarts}/{max})...");
    match start_child(binary, env_vars) {
        Ok(new_child) => {
            eprintln!("tsmd: {label} restarted (PID {})", new_child.id());
            *child = Some(new_child);
            *restarts = 0;
        }
        Err(e) => {
            eprintln!("tsmd: failed to restart {label}: {e}");
            *child = None;
        }
    }
}

/// Stop a child process gracefully.
fn stop_child(label: &str, child: Option<Child>) {
    if let Some(mut child) = child {
        eprintln!("tsmd: stopping {label} (PID {})...", child.id());
        let _ = child.kill();
        let _ = child.wait();
    }
}

// ─── Client handling ──────────────────────────────────────────────

fn handle_client(
    stream: &mut std::os::unix::net::UnixStream,
    conn: &Arc<Mutex<rusqlite::Connection>>,
    project_root: &std::path::Path,
) -> Result<()> {
    stream.set_read_timeout(Some(std::time::Duration::from_secs(30)))?;
    stream.set_write_timeout(Some(std::time::Duration::from_secs(30)))?;
    let req = read_request(stream)?;
    let conn = conn
        .lock()
        .map_err(|e| anyhow::anyhow!("DB lock poisoned: {e}"))?;
    let resp = daemon::handle_request(&conn, req, project_root, &SHUTDOWN);
    write_response(stream, &resp)?;
    Ok(())
}
