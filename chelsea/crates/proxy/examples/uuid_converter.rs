//! UUID to Short UUID converter utility
//!
//! Usage:
//!   cargo run --example uuid_converter <uuid>
//!
//! Example:
//!   cargo run --example uuid_converter 67391927-ec7d-488b-809d-560bdcaa4162

use short_uuid::ShortUuid;
use uuid::Uuid;

fn main() {
    let args: Vec<String> = std::env::args().collect();

    if args.len() < 2 {
        eprintln!("Usage: {} <uuid>", args[0]);
        eprintln!("\nConverts a UUID to Short UUID format used in hostnames");
        eprintln!("\nExample:");
        eprintln!("  {} 67391927-ec7d-488b-809d-560bdcaa4162", args[0]);
        std::process::exit(1);
    }

    let uuid_str = &args[1];

    match Uuid::parse_str(uuid_str) {
        Ok(uuid) => {
            let short = ShortUuid::from(uuid);

            println!("UUID:       {}", uuid);
            println!("Short UUID: {}", short);
            println!("Hostname:   {}.vm.vers.sh", short);
        }
        Err(e) => {
            eprintln!("Error: Invalid UUID format: {}", e);
            std::process::exit(1);
        }
    }
}
