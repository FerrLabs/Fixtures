mod cli;
mod generate;
mod rng;
mod tree;
mod types;
mod validate;

use anyhow::Result;
use rayon::prelude::*;
use std::fs;
use std::sync::atomic::{AtomicUsize, Ordering};

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let mode = cli::parse_args(&args)?;

    match mode {
        cli::Mode::Validate { defs_dir } => {
            if !defs_dir.exists() {
                anyhow::bail!("{} not found.", defs_dir.display());
            }
            if !validate::validate_definitions(&defs_dir)? {
                std::process::exit(1);
            }
        }
        cli::Mode::Generate {
            defs_dir,
            gen_dir,
            verbose,
        } => {
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
            // Each fixture definition produces an independent output
            // directory under gen_dir/<name>, so we can fan out across
            // CPU cores. Rayon's default global pool sizes itself to
            // the host (one thread per logical core), which on a CI
            // runner with ~6 fixtures cuts wall-time roughly in half.
            // git2 is safe to use concurrently as long as each thread
            // builds its own Repository handle, which is exactly what
            // generate_fixture does.
            let ok = AtomicUsize::new(0);
            entries.par_iter().for_each(|entry| {
                let path = entry.path();
                let name = path.file_stem().unwrap().to_string_lossy().to_string();
                let start = std::time::Instant::now();
                match generate::generate_fixture(&path, &gen_dir.join(&name), verbose) {
                    Ok(()) => {
                        if verbose {
                            let elapsed = start.elapsed();
                            println!("  ok  {name} ({elapsed:.2?})");
                        } else {
                            println!("  ok  {name}");
                        }
                        ok.fetch_add(1, Ordering::Relaxed);
                    }
                    Err(e) => {
                        eprintln!("  ERR {name}: {e}");
                    }
                }
            });

            let ok = ok.load(Ordering::Relaxed);
            println!("\n{ok}/{total} fixtures generated");
            if ok < total {
                std::process::exit(1);
            }
        }
    }
    Ok(())
}
