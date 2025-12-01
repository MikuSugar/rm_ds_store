use glob::glob;
use std::fs;
use clap::Parser;
use std::time::Instant;
use rayon::prelude::*;

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

    let mut path: String = args.path.unwrap_or_else(|| ".".to_string());

    if path.ends_with("/") {
        path.pop();
    }
    let start_time = Instant::now();
    let path = format!("{}/**/.DS_Store", path);
    println!("Search for {} ...", path);
    
    let count = glob(&path)
        .expect(&format!("Failed to read {}", path))
        .par_bridge()  // 让 glob 迭代器变成并行迭代器
        .filter_map(Result::ok)
        .filter(|p| p.is_file())
        .filter_map(|path| {
            if args.show {
                println!("rm file {:?}", path.display());
            }
            fs::remove_file(&path).ok().map(|_| 1)
        })
        .sum::<usize>();

    let end_time = Instant::now();
    let time_elapsed = end_time.duration_since(start_time);
    println!("{} files have been deleted, program execution time：{:?}", count, time_elapsed);
}
