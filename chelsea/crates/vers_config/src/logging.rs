#[macro_export]
macro_rules! log {
    ($($arg:tt)*) => {{
        // Blue color: \x1b[34m, Reset: \x1b[0m
        println!("\x1b[34m[Debug]\x1b[0m vers_config: {}", format!($($arg)*));
    }};
}

#[macro_export]
macro_rules! warn {
    ($($arg:tt)*) => {{
        // Yellow color: \x1b[33m, Reset: \x1b[0m
        println!("\x1b[33m[Warn]\x1b[0m vers_config: {}", format!($($arg)*));
    }};
}
