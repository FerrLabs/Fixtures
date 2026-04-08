mod cli;
mod generate;
mod rng;
mod tree;
mod types;

use anyhow::Result;
use std::fs;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let (defs_dir, gen_dir) = cli::parse_args(&args)?;

    if !defs_dir.exists() {
        anyhow::bail!("{} not found.", defs_dir.display());
    }

    if gen_dir.exists() {
        fs::remove_dir_all(&gen_dir)?;
    }
    fs::create_dir_all(&gen_dir)?;

    let mut entries: Vec<_> = fs::read_dir(&defs_dir)?
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "json"))
        .collect();
    entries.sort_by_key(|e| e.path());

    let total = entries.len();
    let mut ok = 0;

    for entry in &entries {
        let path = entry.path();
        let name = path.file_stem().unwrap().to_string_lossy().to_string();

        match generate::generate_fixture(&path, &gen_dir.join(&name)) {
            Ok(()) => {
                println!("  ok  {name}");
                ok += 1;
            }
            Err(e) => {
                eprintln!("  ERR {name}: {e}");
            }
        }
    }

    println!("\n{ok}/{total} fixtures generated");
    if ok < total {
        std::process::exit(1);
    }
    Ok(())
}
