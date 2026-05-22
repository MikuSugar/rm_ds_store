use clap::Parser;
use std::collections::HashSet;
use std::env;
use std::fs;
use std::io::{self, IsTerminal, Write};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::Instant;

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
    let mut result = RemoveResult::default();
    let mut visited = HashSet::new();
    let mut stack = vec![root.to_path_buf()];

    while let Some(path) = stack.pop() {
        write_status_line(out, show_status, &path, result.removed.len())?;
        let metadata = fs::metadata(&path)?;

        if metadata.is_file() {
            if is_ds_store(&path) {
                remove_ds_store_file(&path, &mut result);
            }
            continue;
        }

        if !metadata.is_dir() {
            continue;
        }

        if !visited.insert(dir_id(&path, &metadata)?) {
            continue;
        }

        for entry in fs::read_dir(&path)? {
            let entry = entry?;
            let entry_path = entry.path();
            write_status_line(out, show_status, &entry_path, result.removed.len())?;

            let entry_type = entry.file_type()?;

            if entry_type.is_symlink() {
                continue;
            }

            let entry_metadata = entry.metadata()?;

            if entry_metadata.is_file() {
                if is_ds_store(&entry_path) {
                    remove_ds_store_file(&entry_path, &mut result);
                }
                continue;
            }

            if entry_metadata.is_dir() {
                stack.push(entry_path);
            }
        }
    }

    Ok(result)
}

fn remove_ds_store_file(path: &Path, result: &mut RemoveResult) {
    match fs::remove_file(path) {
        Ok(()) => result.removed.push(path.to_path_buf()),
        Err(err) => result.failed.push((path.to_path_buf(), err)),
    }
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

    let status = format!("Searching: {} | deleted: {}", path.display(), count);
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
