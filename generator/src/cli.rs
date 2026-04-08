use anyhow::{Context, Result};
use std::path::PathBuf;

#[derive(Debug)]
pub enum Mode {
    Generate { defs_dir: PathBuf, gen_dir: PathBuf },
    Validate { defs_dir: PathBuf },
}

pub fn parse_args(args: &[String]) -> Result<Mode> {
    let mut defs_dir = PathBuf::from("fixtures/definitions");
    let mut gen_dir = PathBuf::from("fixtures/generated");
    let mut validate = false;

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
            "--validate" => {
                validate = true;
            }
            other => anyhow::bail!("unknown argument: {other}\n\nRun with --help for usage."),
        }
        i += 1;
    }

    if validate {
        Ok(Mode::Validate { defs_dir })
    } else {
        Ok(Mode::Generate { defs_dir, gen_dir })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(slice: &[&str]) -> Vec<String> {
        slice.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn defaults_when_no_args() {
        let mode = parse_args(&args(&["bin"])).unwrap();
        match mode {
            Mode::Generate { defs_dir, gen_dir } => {
                assert_eq!(defs_dir, PathBuf::from("fixtures/definitions"));
                assert_eq!(gen_dir, PathBuf::from("fixtures/generated"));
            }
            _ => panic!("expected Generate mode"),
        }
    }

    #[test]
    fn long_flags() {
        let mode = parse_args(&args(&[
            "bin",
            "--definitions",
            "/tmp/defs",
            "--output",
            "/tmp/out",
        ]))
        .unwrap();
        match mode {
            Mode::Generate { defs_dir, gen_dir } => {
                assert_eq!(defs_dir, PathBuf::from("/tmp/defs"));
                assert_eq!(gen_dir, PathBuf::from("/tmp/out"));
            }
            _ => panic!("expected Generate mode"),
        }
    }

    #[test]
    fn short_flags() {
        let mode = parse_args(&args(&["bin", "-d", "my/defs", "-o", "my/out"])).unwrap();
        match mode {
            Mode::Generate { defs_dir, gen_dir } => {
                assert_eq!(defs_dir, PathBuf::from("my/defs"));
                assert_eq!(gen_dir, PathBuf::from("my/out"));
            }
            _ => panic!("expected Generate mode"),
        }
    }

    #[test]
    fn only_definitions_flag() {
        let mode = parse_args(&args(&["bin", "-d", "custom"])).unwrap();
        match mode {
            Mode::Generate { defs_dir, gen_dir } => {
                assert_eq!(defs_dir, PathBuf::from("custom"));
                assert_eq!(gen_dir, PathBuf::from("fixtures/generated"));
            }
            _ => panic!("expected Generate mode"),
        }
    }

    #[test]
    fn only_output_flag() {
        let mode = parse_args(&args(&["bin", "-o", "custom"])).unwrap();
        match mode {
            Mode::Generate { defs_dir, gen_dir } => {
                assert_eq!(defs_dir, PathBuf::from("fixtures/definitions"));
                assert_eq!(gen_dir, PathBuf::from("custom"));
            }
            _ => panic!("expected Generate mode"),
        }
    }

    #[test]
    fn validate_flag() {
        let mode = parse_args(&args(&["bin", "--validate", "-d", "my/defs"])).unwrap();
        match mode {
            Mode::Validate { defs_dir } => {
                assert_eq!(defs_dir, PathBuf::from("my/defs"));
            }
            _ => panic!("expected Validate mode"),
        }
    }

    #[test]
    fn validate_flag_default_defs() {
        let mode = parse_args(&args(&["bin", "--validate"])).unwrap();
        match mode {
            Mode::Validate { defs_dir } => {
                assert_eq!(defs_dir, PathBuf::from("fixtures/definitions"));
            }
            _ => panic!("expected Validate mode"),
        }
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
        --validate             Validate definitions without generating repos
    -h, --help                 Print help information
    -V, --version              Print version",
        env!("CARGO_PKG_VERSION")
    );
}
