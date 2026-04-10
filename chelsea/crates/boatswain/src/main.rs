use std::{env, error::Error, fmt::Display, path::PathBuf};

use boatswain::DockerDump;

#[tokio::main]
async fn main() {
    let program_name = env::args().into_iter().next().unwrap();
    let args = match fetch_args() {
        Ok(value) => value,
        Err(e) => {
            print_usage(&e.to_string(), &program_name);
            return;
        }
    };

    let dump = match DockerDump::new(&args.source_path, args.destination_path).await {
        Ok(value) => value,
        Err(e) => {
            println!("{}", e.to_string());
            return;
        }
    };

    println!("Docker dump created at {}", dump.path.display());
}

fn print_usage(error_msg: &str, program_name: &str) {
    println!("{error_msg}\nUsage: {program_name} source destination");
}

struct BoatswainArgs {
    source_path: PathBuf,
    destination_path: PathBuf,
}

#[derive(Debug)]
enum FetchArgsError {
    MissingSourceDirectory,
    MissingDestinationDirectory,
}

impl Error for FetchArgsError {}

impl Display for FetchArgsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingSourceDirectory => write!(f, "Missing source directory"),
            Self::MissingDestinationDirectory => write!(f, "Missing destination directory"),
        }
    }
}

fn fetch_args() -> Result<BoatswainArgs, FetchArgsError> {
    let mut args = env::args().into_iter();

    let source = args.nth(1).ok_or(FetchArgsError::MissingSourceDirectory)?;
    let destination = args
        .next()
        .ok_or(FetchArgsError::MissingDestinationDirectory)?;

    let source_path = PathBuf::from(source);
    let destination_path = PathBuf::from(destination);

    Ok(BoatswainArgs {
        source_path,
        destination_path,
    })
}
