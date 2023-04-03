use glob::glob;
use std::fs;
use clap::Parser;
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

    let mut path: String = match args.path {
        Some(p) => p,
        None => ".".to_string()
    };

    if path.ends_with("/") {
        path.pop();
    }
    let start_time = Instant::now();
    let mut count = 0;
    let path = format!("{}/**/.DS_Store", path);
    println!("Search for {} ...", path);
    for entry in glob(&path).expect(&format!("Failed to read {}", path)) {
        match entry {
            Ok(path) => {
                if path.is_file() {
                    if args.show {
                        println!("rm file {:?}", path.display());
                    }
                    count += 1;
                    fs::remove_file(path.as_path()).expect(&format!("Failed to delete file {}!", path.display()))
                }
            }
            Err(e) => eprintln!("{:?}", e),
        }
    }

    let end_time = Instant::now();
    let time_elapsed = end_time.duration_since(start_time);
    println!("{} files have been deleted,program execution time：{:?}", count, time_elapsed);
}
