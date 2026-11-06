//! `cookbook-export`: print the parsed cookbook recipes as JSON for the Node
//! tools (`scripts/cookbook`). Every recipe is validated first unless
//! `--no-validate`; any problem is printed to stderr and the exit status is 1.
//! Needs no engine, so it builds fast with `--no-default-features`.

use std::path::PathBuf;
use std::process::ExitCode;

use relate_cookbook::{cookbook_dir, export, load_recipes};

const HELP: &str = "\
Print the parsed cookbook recipes as JSON (format 1) for scripts/cookbook.

Usage: cookbook-export [--cookbook DIR] [--no-validate]

Options:
  --cookbook DIR  Recipe root (default: the repository's docs/cookbook)
  --no-validate   Skip the recipe checks; a recipe that does not parse still fails
  -h, --help      Print this help

Problems go to stderr and exit with 1.
";

fn main() -> ExitCode {
    let mut root = cookbook_dir();
    let mut validate = true;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                print!("{HELP}");
                return ExitCode::SUCCESS;
            }
            "--no-validate" => validate = false,
            "--cookbook" => match args.next() {
                Some(dir) => root = PathBuf::from(dir),
                None => return usage("--cookbook needs a directory"),
            },
            other => return usage(&format!("unknown argument `{other}`")),
        }
    }
    let (recipes, problems) = match load_recipes(&root, None, validate) {
        Ok(loaded) => loaded,
        Err(e) => {
            eprintln!("cookbook-export: {}: {e}", root.display());
            return ExitCode::FAILURE;
        }
    };
    if !problems.is_empty() {
        for p in &problems {
            eprintln!("{p}");
        }
        return ExitCode::FAILURE;
    }
    println!("{}", export::export(&recipes));
    ExitCode::SUCCESS
}

fn usage(message: &str) -> ExitCode {
    eprintln!("cookbook-export: {message}\n\n{HELP}");
    ExitCode::from(2)
}
