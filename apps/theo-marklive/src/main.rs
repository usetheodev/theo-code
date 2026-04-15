//! theo-marklive CLI — Render markdown wiki to beautiful HTML.

use std::path::PathBuf;

fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        eprintln!("theo-marklive — Beautiful markdown wiki viewer\n");
        eprintln!("Usage:");
        eprintln!("  theo-marklive <markdown-dir> [-o output.html] [--title \"My Wiki\"]");
        eprintln!("  theo-marklive .theo/wiki/ -o wiki.html");
        eprintln!("  theo-marklive .theo/wiki/              # writes to stdout");
        std::process::exit(1);
    }

    let input_dir = PathBuf::from(&args[1]);

    // Parse optional args
    let mut output_path: Option<PathBuf> = None;
    let mut title = "Code Wiki".to_string();

    let mut i = 2;
    while i < args.len() {
        match args[i].as_str() {
            "-o" | "--output" => {
                i += 1;
                if i < args.len() {
                    output_path = Some(PathBuf::from(&args[i]));
                }
            }
            "--title" => {
                i += 1;
                if i < args.len() {
                    title = args[i].clone();
                }
            }
            _ => {}
        }
        i += 1;
    }

    let config = theo_marklive::Config {
        title,
        search: true,
    };

    match theo_marklive::render(&input_dir, config) {
        Ok(html) => {
            if let Some(path) = output_path {
                std::fs::write(&path, &html).unwrap_or_else(|e| {
                    eprintln!("Error writing {}: {}", path.display(), e);
                    std::process::exit(1);
                });
                eprintln!("Written to {}", path.display());
            } else {
                print!("{}", html);
            }
        }
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    }
}
