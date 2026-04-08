use anyhow::{Context, Result};
use std::path::PathBuf;

pub fn parse_args(args: &[String]) -> Result<(PathBuf, PathBuf)> {
    let mut defs_dir = PathBuf::from("fixtures/definitions");
    let mut gen_dir = PathBuf::from("fixtures/generated");

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--help" | "-h" => {
                print_help();
                std::process::exit(0);
            }
            "--version" | "-V" => {
                println!("generate-fixtures {}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            "--definitions" | "-d" => {
                i += 1;
                defs_dir = PathBuf::from(args.get(i).context("missing value for --definitions")?);
            }
            "--output" | "-o" => {
                i += 1;
                gen_dir = PathBuf::from(args.get(i).context("missing value for --output")?);
            }
            other => anyhow::bail!("unknown argument: {other}\n\nRun with --help for usage."),
        }
        i += 1;
    }

    Ok((defs_dir, gen_dir))
}

fn print_help() {
    println!(
        "generate-fixtures {}
Generate git fixture repos from JSON definitions.

USAGE:
    generate-fixtures [OPTIONS]

OPTIONS:
    -d, --definitions <DIR>    Path to JSON definitions directory [default: fixtures/definitions]
    -o, --output <DIR>         Output directory for generated repos [default: fixtures/generated]
    -h, --help                 Print help information
    -V, --version              Print version",
        env!("CARGO_PKG_VERSION")
    );
}
