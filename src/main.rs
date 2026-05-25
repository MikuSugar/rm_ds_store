use clap::Parser;
use std::collections::{HashSet, VecDeque};
use std::env;
use std::fs;
use std::io::{self, IsTerminal, Write};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    mpsc, Arc, Condvar, Mutex,
};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Parser, Debug)]
#[command(author = "mikusugar", version, about = "Helps delete Mac OS .DS_Stroe files", long_about = None)]
struct Args {
    #[arg(short, long)]
    path: Option<String>,

    #[arg(short, long, default_value_t = true)]
    show: bool,
}

fn main() {
    let args = Args::parse();

    let path = PathBuf::from(args.path.unwrap_or_else(|| ".".to_string()));
    let start_time = Instant::now();

    let stdout = io::stdout();
    let show_status = stdout.is_terminal();
    let mut stdout = stdout.lock();
    let result = match remove_ds_store_files(&path, &mut stdout, show_status) {
        Ok(result) => result,
        Err(err) => {
            if show_status {
                clear_status_line(&mut stdout).ok();
            }
            eprintln!("Failed to read {}: {}", path.display(), err);
            std::process::exit(1);
        }
    };

    if show_status {
        clear_status_line(&mut stdout).ok();
    }
    drop(stdout);

    let end_time = Instant::now();
    let time_elapsed = end_time.duration_since(start_time);

    if args.show {
        for path in &result.removed {
            println!("rm file {}", path.display());
        }

        for (path, err) in &result.failed {
            println!("failed to remove {}: {}", path.display(), err);
        }
    }

    println!(
        "{} files have been deleted, program execution time：{:?}",
        result.removed.len(),
        time_elapsed
    );
}

#[derive(Debug, Default)]
struct RemoveResult {
    removed: Vec<PathBuf>,
    failed: Vec<(PathBuf, io::Error)>,
}

fn remove_ds_store_files<W: Write>(
    root: &Path,
    out: &mut W,
    show_status: bool,
) -> io::Result<RemoveResult> {
    let root_metadata = fs::metadata(root)?;
    write_status_line(out, show_status, root, 0)?;

    let removed_count = Arc::new(AtomicUsize::new(0));
    let delete_removed_count = Arc::clone(&removed_count);
    let (delete_tx, delete_rx) = mpsc::channel::<PathBuf>();
    let delete_thread = thread::spawn(move || {
        let mut result = RemoveResult::default();
        for path in delete_rx {
            let removed_before = result.removed.len();
            remove_ds_store_file(&path, &mut result);
            if result.removed.len() > removed_before {
                delete_removed_count.fetch_add(1, Ordering::Relaxed);
            }
        }
        result
    });

    if root_metadata.is_file() {
        if is_ds_store(root) {
            send_delete(root.to_path_buf(), &delete_tx)?;
        }
        drop(delete_tx);
        let result = join_delete_thread(delete_thread)?;
        write_status_line(out, show_status, root, result.removed.len())?;
        return Ok(result);
    }

    if !root_metadata.is_dir() {
        drop(delete_tx);
        let result = join_delete_thread(delete_thread)?;
        write_status_line(out, show_status, root, result.removed.len())?;
        return Ok(result);
    }

    let queue = Arc::new(DirQueue::new(root.to_path_buf()));
    let visited = Arc::new(Mutex::new(HashSet::new()));
    let first_error = Arc::new(Mutex::new(None));
    let latest_path = Arc::new(Mutex::new(root.to_path_buf()));
    let worker_count = traversal_thread_count();
    let mut workers = Vec::with_capacity(worker_count);

    for _ in 0..worker_count {
        let queue = Arc::clone(&queue);
        let visited = Arc::clone(&visited);
        let first_error = Arc::clone(&first_error);
        let latest_path = Arc::clone(&latest_path);
        let delete_tx = delete_tx.clone();

        workers.push(thread::spawn(move || {
            let mut last_status_update = None;
            while let Some(path) = queue.pop() {
                process_directory(
                    &path,
                    &queue,
                    &visited,
                    &first_error,
                    &latest_path,
                    &mut last_status_update,
                    &delete_tx,
                );
                queue.finish_dir();
            }
        }));
    }

    drop(delete_tx);

    while workers.iter().any(|worker| !worker.is_finished()) {
        let path = latest_path.lock().unwrap().clone();
        write_status_line(
            out,
            show_status,
            &path,
            removed_count.load(Ordering::Relaxed),
        )?;
        thread::sleep(status_refresh_interval());
    }

    for worker in workers {
        if worker.join().is_err() {
            queue.close();
            return Err(io::Error::new(
                io::ErrorKind::Other,
                "directory traversal worker panicked",
            ));
        }
    }

    let result = join_delete_thread(delete_thread)?;
    write_status_line(out, show_status, root, result.removed.len())?;

    if let Some(err) = first_error.lock().unwrap().take() {
        return Err(err);
    }

    Ok(result)
}

fn remove_ds_store_file(path: &Path, result: &mut RemoveResult) {
    match fs::remove_file(path) {
        Ok(()) => result.removed.push(path.to_path_buf()),
        Err(err) => result.failed.push((path.to_path_buf(), err)),
    }
}

struct DirQueue {
    state: Mutex<DirQueueState>,
    available: Condvar,
}

struct DirQueueState {
    dirs: VecDeque<PathBuf>,
    active: usize,
    done: bool,
}

impl DirQueue {
    fn new(root: PathBuf) -> Self {
        let mut dirs = VecDeque::new();
        dirs.push_back(root);

        Self {
            state: Mutex::new(DirQueueState {
                dirs,
                active: 0,
                done: false,
            }),
            available: Condvar::new(),
        }
    }

    fn pop(&self) -> Option<PathBuf> {
        let mut state = self.state.lock().unwrap();

        loop {
            if let Some(path) = state.dirs.pop_front() {
                state.active += 1;
                return Some(path);
            }

            if state.done {
                return None;
            }

            state = self.available.wait(state).unwrap();
        }
    }

    fn push(&self, path: PathBuf) {
        let mut state = self.state.lock().unwrap();
        if state.done {
            return;
        }

        state.dirs.push_back(path);
        self.available.notify_one();
    }

    fn finish_dir(&self) {
        let mut state = self.state.lock().unwrap();
        state.active -= 1;

        if state.active == 0 && state.dirs.is_empty() {
            state.done = true;
            self.available.notify_all();
        }
    }

    fn close(&self) {
        let mut state = self.state.lock().unwrap();
        state.done = true;
        self.available.notify_all();
    }
}

fn process_directory(
    path: &Path,
    queue: &DirQueue,
    visited: &Mutex<HashSet<(u64, u64)>>,
    first_error: &Mutex<Option<io::Error>>,
    latest_path: &Mutex<PathBuf>,
    last_status_update: &mut Option<Instant>,
    delete_tx: &mpsc::Sender<PathBuf>,
) {
    maybe_update_latest_path(latest_path, path, last_status_update);

    let metadata = match fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(err) if should_skip_traversal_error(&err) => return,
        Err(err) => {
            set_traversal_error(err, first_error, queue);
            return;
        }
    };

    if !metadata.is_dir() {
        return;
    }

    let id = match dir_id(path, &metadata) {
        Ok(id) => id,
        Err(err) if should_skip_traversal_error(&err) => return,
        Err(err) => {
            set_traversal_error(err, first_error, queue);
            return;
        }
    };

    if !visited.lock().unwrap().insert(id) {
        return;
    }

    let entries = match fs::read_dir(path) {
        Ok(entries) => entries,
        Err(err) if should_skip_traversal_error(&err) => return,
        Err(err) => {
            set_traversal_error(err, first_error, queue);
            return;
        }
    };

    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(err) if should_skip_traversal_error(&err) => continue,
            Err(err) => {
                set_traversal_error(err, first_error, queue);
                return;
            }
        };
        let entry_path = entry.path();
        maybe_update_latest_path(latest_path, &entry_path, last_status_update);

        let entry_type = match entry.file_type() {
            Ok(entry_type) => entry_type,
            Err(err) if should_skip_traversal_error(&err) => continue,
            Err(err) => {
                set_traversal_error(err, first_error, queue);
                return;
            }
        };

        if entry_type.is_symlink() {
            continue;
        }

        let entry_metadata = match entry.metadata() {
            Ok(metadata) => metadata,
            Err(err) if should_skip_traversal_error(&err) => continue,
            Err(err) => {
                set_traversal_error(err, first_error, queue);
                return;
            }
        };

        if entry_metadata.is_file() {
            if is_ds_store(&entry_path) && send_delete(entry_path, delete_tx).is_err() {
                set_traversal_error(
                    io::Error::new(io::ErrorKind::BrokenPipe, "delete worker stopped"),
                    first_error,
                    queue,
                );
                return;
            }
            continue;
        }

        if entry_metadata.is_dir() {
            queue.push(entry_path);
        }
    }
}

fn should_skip_traversal_error(err: &io::Error) -> bool {
    err.kind() == io::ErrorKind::PermissionDenied
}

fn maybe_update_latest_path(
    latest_path: &Mutex<PathBuf>,
    path: &Path,
    last_status_update: &mut Option<Instant>,
) {
    let now = Instant::now();
    if last_status_update.map_or(true, |last| {
        now.duration_since(last) >= status_refresh_interval()
    }) {
        *latest_path.lock().unwrap() = path.to_path_buf();
        *last_status_update = Some(now);
    }
}

fn status_refresh_interval() -> Duration {
    Duration::from_millis(50)
}

fn set_traversal_error(err: io::Error, first_error: &Mutex<Option<io::Error>>, queue: &DirQueue) {
    let mut first_error = first_error.lock().unwrap();
    if first_error.is_none() {
        *first_error = Some(err);
    }
    queue.close();
}

fn send_delete(path: PathBuf, delete_tx: &mpsc::Sender<PathBuf>) -> io::Result<()> {
    delete_tx
        .send(path)
        .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "delete worker stopped"))
}

fn join_delete_thread(delete_thread: thread::JoinHandle<RemoveResult>) -> io::Result<RemoveResult> {
    delete_thread
        .join()
        .map_err(|_| io::Error::new(io::ErrorKind::Other, "delete worker panicked"))
}

fn traversal_thread_count() -> usize {
    thread::available_parallelism()
        .map(|count| count.get())
        .unwrap_or(4)
}

fn write_status_line<W: Write>(
    out: &mut W,
    show_status: bool,
    path: &Path,
    count: usize,
) -> io::Result<()> {
    if !show_status {
        return Ok(());
    }

    let status = format!("deleted: {} | Searching: {}", count, path.display());
    write!(out, "\r\x1b[2K{}", fit_status_line(&status))?;
    out.flush()
}

fn clear_status_line<W: Write>(out: &mut W) -> io::Result<()> {
    write!(out, "\r\x1b[2K")?;
    out.flush()
}

fn is_ds_store(path: &Path) -> bool {
    path.file_name().map_or(false, |name| name == ".DS_Store")
}

fn fit_status_line(status: &str) -> String {
    let width = terminal_width().saturating_sub(1);
    if status.chars().count() <= width {
        return status.to_string();
    }

    if width <= 3 {
        return ".".repeat(width);
    }

    let keep = width.saturating_sub(3);
    let mut clipped = status.chars().take(keep).collect::<String>();
    clipped.push_str("...");
    clipped
}

fn terminal_width() -> usize {
    env::var("COLUMNS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|width| *width > 0)
        .unwrap_or(80)
}

fn dir_id(_path: &Path, metadata: &fs::Metadata) -> io::Result<(u64, u64)> {
    Ok((metadata.dev(), metadata.ino()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::fs::symlink;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn deletes_ds_store_files_without_following_symlink_loops() {
        let root = test_dir("rm_ds_store_loop");
        let nested = root.join("nested");
        fs::create_dir_all(&nested).unwrap();
        fs::write(root.join(".DS_Store"), b"root").unwrap();
        fs::write(nested.join(".DS_Store"), b"nested").unwrap();

        symlink(&root, nested.join("loop")).unwrap();

        let result = remove_ds_store_files(&root, &mut io::sink(), false).unwrap();

        assert_eq!(result.removed.len(), 2);
        assert!(!root.join(".DS_Store").exists());
        assert!(!nested.join(".DS_Store").exists());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn deletes_ds_store_files_across_independent_subtrees() {
        let root = test_dir("rm_ds_store_parallel");
        let left = root.join("left").join("deep");
        let right = root.join("right").join("deep");
        fs::create_dir_all(&left).unwrap();
        fs::create_dir_all(&right).unwrap();
        fs::write(root.join(".DS_Store"), b"root").unwrap();
        fs::write(left.join(".DS_Store"), b"left").unwrap();
        fs::write(right.join(".DS_Store"), b"right").unwrap();
        fs::write(right.join("notes.txt"), b"keep").unwrap();

        let result = remove_ds_store_files(&root, &mut io::sink(), false).unwrap();

        assert_eq!(result.removed.len(), 3);
        assert_eq!(result.failed.len(), 0);
        assert!(!root.join(".DS_Store").exists());
        assert!(!left.join(".DS_Store").exists());
        assert!(!right.join(".DS_Store").exists());
        assert!(right.join("notes.txt").exists());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn deletes_root_ds_store_file() {
        let root = test_dir("rm_ds_store_root_file");
        fs::create_dir_all(&root).unwrap();
        let ds_store = root.join(".DS_Store");
        fs::write(&ds_store, b"root file").unwrap();

        let result = remove_ds_store_files(&ds_store, &mut io::sink(), false).unwrap();

        assert_eq!(result.removed.len(), 1);
        assert_eq!(result.failed.len(), 0);
        assert!(!ds_store.exists());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn skips_directories_without_permission() {
        let root = test_dir("rm_ds_store_permission_denied");
        let readable = root.join("readable");
        let blocked = root.join("blocked");
        fs::create_dir_all(&readable).unwrap();
        fs::create_dir_all(&blocked).unwrap();
        fs::write(readable.join(".DS_Store"), b"readable").unwrap();
        fs::write(blocked.join(".DS_Store"), b"blocked").unwrap();

        let original_permissions = fs::metadata(&blocked).unwrap().permissions();
        let mut blocked_permissions = original_permissions.clone();
        blocked_permissions.set_mode(0o000);
        fs::set_permissions(&blocked, blocked_permissions).unwrap();

        let result = remove_ds_store_files(&root, &mut io::sink(), false);

        fs::set_permissions(&blocked, original_permissions).unwrap();
        let result = result.unwrap();

        assert_eq!(result.removed.len(), 1);
        assert_eq!(result.failed.len(), 0);
        assert!(!readable.join(".DS_Store").exists());
        assert!(blocked.join(".DS_Store").exists());

        fs::remove_dir_all(root).unwrap();
    }

    fn test_dir(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let mut name = OsString::from(prefix);
        name.push(format!("_{}_{}", std::process::id(), nanos));
        std::env::temp_dir().join(name)
    }
}
