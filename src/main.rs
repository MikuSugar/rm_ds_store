use glob::glob;
use std::fs;
use clap::Parser;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
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

    let path = format!("{}/**/.DS_Store", path);
    for entry in glob(&path).expect(&format!("Failed to read {}", path)) {
        match entry {
            Ok(path) => {
                if path.is_file() {
                    println!("rm {:?}", path.display());
                    fs::remove_file(path.as_path()).expect("Failed to delete file!")
                }
            }
            Err(e) => eprintln!("{:?}", e),
        }
    }
}
