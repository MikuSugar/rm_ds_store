use clap::Parser;
use rayon::prelude::*;
use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Instant;

#[cfg(unix)]
use std::os::unix::fs::MetadataExt;

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

    println!("Search for {} ...", path.display());

    let files = match find_ds_store_files(&path) {
        Ok(files) => files,
        Err(err) => {
            eprintln!("Failed to read {}: {}", path.display(), err);
            std::process::exit(1);
        }
    };

    let count = files
        .par_iter()
        .filter_map(|path| {
            if args.show {
                println!("rm file {:?}", path.display());
            }
            fs::remove_file(path).ok().map(|_| 1)
        })
        .sum::<usize>();

    let end_time = Instant::now();
    let time_elapsed = end_time.duration_since(start_time);
    println!(
        "{} files have been deleted, program execution time：{:?}",
        count, time_elapsed
    );
}

fn find_ds_store_files(root: &Path) -> io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    let mut visited = HashSet::new();
    let mut stack = vec![root.to_path_buf()];

    while let Some(path) = stack.pop() {
        let metadata = fs::metadata(&path)?;

        if metadata.is_file() {
            if is_ds_store(&path) {
                files.push(path);
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
            let entry_type = entry.file_type()?;

            if entry_type.is_symlink() {
                continue;
            }

            let entry_metadata = entry.metadata()?;

            if entry_metadata.is_file() {
                if is_ds_store(&entry_path) {
                    files.push(entry_path);
                }
                continue;
            }

            if entry_metadata.is_dir() {
                stack.push(entry_path);
            }
        }
    }

    Ok(files)
}

fn is_ds_store(path: &Path) -> bool {
    path.file_name().map_or(false, |name| name == ".DS_Store")
}

#[cfg(unix)]
fn dir_id(_path: &Path, metadata: &fs::Metadata) -> io::Result<(u64, u64)> {
    Ok((metadata.dev(), metadata.ino()))
}

#[cfg(not(unix))]
fn dir_id(path: &Path, _metadata: &fs::Metadata) -> io::Result<PathBuf> {
    fs::canonicalize(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[cfg(unix)]
    use std::os::unix::fs::symlink;

    #[test]
    fn finds_ds_store_files_without_following_symlink_loops() {
        let root = test_dir("rm_ds_store_loop");
        let nested = root.join("nested");
        fs::create_dir_all(&nested).unwrap();
        fs::write(root.join(".DS_Store"), b"root").unwrap();
        fs::write(nested.join(".DS_Store"), b"nested").unwrap();

        #[cfg(unix)]
        symlink(&root, nested.join("loop")).unwrap();

        let mut files = find_ds_store_files(&root).unwrap();
        files.sort();

        assert_eq!(
            files,
            vec![root.join(".DS_Store"), nested.join(".DS_Store")]
        );

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
