use glob::glob;
use std::fs;
use clap::Parser;

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

    let path = format!("{}/**/.DS_Store", path);
    println!("Search for {} ...", path);
    for entry in glob(&path).expect(&format!("Failed to read {}", path)) {
        match entry {
            Ok(path) => {
                if path.is_file() {
                    if args.show {
                        println!("rm file {:?}", path.display());
                    }
                    fs::remove_file(path.as_path()).expect(&format!("Failed to delete file {}!", path.display()))
                }
            }
            Err(e) => eprintln!("{:?}", e),
        }
    }
}
