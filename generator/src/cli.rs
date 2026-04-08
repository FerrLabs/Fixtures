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

#[cfg(test)]
mod tests {
    use super::*;

    fn args(slice: &[&str]) -> Vec<String> {
        slice.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn defaults_when_no_args() {
        let (defs, gen) = parse_args(&args(&["bin"])).unwrap();
        assert_eq!(defs, PathBuf::from("fixtures/definitions"));
        assert_eq!(gen, PathBuf::from("fixtures/generated"));
    }

    #[test]
    fn long_flags() {
        let (defs, gen) = parse_args(&args(&[
            "bin",
            "--definitions",
            "/tmp/defs",
            "--output",
            "/tmp/out",
        ]))
        .unwrap();
        assert_eq!(defs, PathBuf::from("/tmp/defs"));
        assert_eq!(gen, PathBuf::from("/tmp/out"));
    }

    #[test]
    fn short_flags() {
        let (defs, gen) = parse_args(&args(&["bin", "-d", "my/defs", "-o", "my/out"])).unwrap();
        assert_eq!(defs, PathBuf::from("my/defs"));
        assert_eq!(gen, PathBuf::from("my/out"));
    }

    #[test]
    fn only_definitions_flag() {
        let (defs, gen) = parse_args(&args(&["bin", "-d", "custom"])).unwrap();
        assert_eq!(defs, PathBuf::from("custom"));
        assert_eq!(gen, PathBuf::from("fixtures/generated"));
    }

    #[test]
    fn only_output_flag() {
        let (defs, gen) = parse_args(&args(&["bin", "-o", "custom"])).unwrap();
        assert_eq!(defs, PathBuf::from("fixtures/definitions"));
        assert_eq!(gen, PathBuf::from("custom"));
    }

    #[test]
    fn missing_definitions_value() {
        let result = parse_args(&args(&["bin", "--definitions"]));
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("missing value"), "unexpected error: {msg}");
    }

    #[test]
    fn missing_output_value() {
        let result = parse_args(&args(&["bin", "--output"]));
        assert!(result.is_err());
    }

    #[test]
    fn unknown_argument() {
        let result = parse_args(&args(&["bin", "--foo"]));
        assert!(result.is_err());
        let msg = result.unwrap_err().to_string();
        assert!(msg.contains("unknown argument"), "unexpected error: {msg}");
    }
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
