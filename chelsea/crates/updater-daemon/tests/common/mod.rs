use ::tokio::process::Command;

pub enum Process {
    Chelsea,
    UpdaterDaemon,
}
pub async fn count_processes(process: Process) -> usize {
    let process_search: &'static str;
    match process {
        Process::Chelsea => process_search = "chelsea",
        Process::UpdaterDaemon => process_search = "updater-daemon",
    }
    let output = Command::new("pgrep")
        .args(["-c", process_search])
        .output()
        .await
        .expect("Error unable to count processes");
    //add more err handling here
    let count_str = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let count: usize = count_str.parse().unwrap_or(0);
    return count;
}
