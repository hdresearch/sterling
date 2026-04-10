use tokio::process::Command;
use tokio::time::sleep;
use tokio::time::Duration;
use tracing::{info, Level};
mod common;
use common::{count_processes, Process};

#[tokio::test]
async fn test_updt_start_chelsea() {
    let _ = tracing_subscriber::fmt()
        .with_max_level(Level::DEBUG)
        .try_init();

    let initial_chelsea_c = count_processes(crate::Process::Chelsea).await;
    info!("Prior Chelsea Instances: {}", initial_chelsea_c);

    let initial_updater_c = count_processes(crate::Process::UpdaterDaemon).await;
    info!("Prior Updater Instances: {}", initial_updater_c);

    if initial_chelsea_c != 0 {
        assert!(
            false,
            "Chelsea Already Running somewhere. Test checks if updtr can spawn a chelsea"
        )
    };
    let mut output_updt = Command::new("sudo")
        .arg("./../../target/debug/updater-daemon")
        .spawn()
        .expect("failed to start updater-daemon");
    sleep(Duration::from_secs(3)).await;
    {
        let c = count_processes(crate::Process::Chelsea).await;
        info!("Test Running Chelsea Instances: {}", c);

        //May Error here if the updater detects an update and stops chelsea
        //and this checks before chelsea is started back up
    }
    {
        let c = count_processes(crate::Process::UpdaterDaemon).await;
        info!("Test Running Updater Instances: {}", c);
    }
    let _ = Command::new("./../../commands.sh")
        .arg("cleanup")
        .output()
        .await
        .expect("Cleanup didnt work");
    println!("cleaning up");

    let _ = output_updt.kill().await;
    {
        let c = count_processes(crate::Process::Chelsea).await;
        info!("Completed Test Chelsea Instances: {}", c);
        assert_eq!(
            c, initial_chelsea_c,
            "Initial Chelsea processes and Current amount not equal!"
        );
    }
    {
        let c = count_processes(crate::Process::UpdaterDaemon).await;
        info!("Completed Test Updater Instances: {}", c);
        assert_eq!(
            c, initial_updater_c,
            "Initial Updater processes and Current amount not equal!"
        );
    }
}

#[tokio::test]
async fn check_chelsea_updater_processes() {
    //
    let _ = tracing_subscriber::fmt()
        .with_max_level(Level::DEBUG)
        .try_init();

    let initial_chelsea_c = count_processes(crate::Process::Chelsea).await;
    info!("Prior Chelsea Instances: {}", initial_chelsea_c);

    let initial_updater_c = count_processes(crate::Process::UpdaterDaemon).await;
    info!("Prior Updater Instances: {}", initial_updater_c);

    let child = match initial_chelsea_c {
        0 => Some(
            Command::new("sudo")
                .arg("./../../chelsea")
                .spawn()
                .expect("failed to run chelsea"),
        ),
        _ => {
            info!("Chelsea already running, not starting new instance");
            None
        }
    };
    let mut output_updt = Command::new("sudo")
        .arg("./../../target/debug/updater-daemon")
        .spawn()
        .expect("failed to start updater-daemon");
    sleep(Duration::from_secs(3)).await;
    {
        let c = count_processes(crate::Process::Chelsea).await;
        info!("Test Running Chelsea Instances: {}", c);

        //May Error here if the updater detects an update and stops chelsea
        //and this checks before chelsea is started back up
    }
    {
        let c = count_processes(crate::Process::UpdaterDaemon).await;
        info!("Test Running Updater Instances: {}", c);
    }
    info!("test ran");
    if child.is_some() {
        let _ = Command::new("./../../commands.sh")
            .arg("cleanup")
            .output()
            .await
            .expect("Cleanup didnt work");
        println!("cleaning up");
    }
    let _ = output_updt.kill().await;
    {
        let c = count_processes(crate::Process::Chelsea).await;
        info!("Completed Test Chelsea Instances: {}", c);
        assert_eq!(
            c, initial_chelsea_c,
            "Initial Chelsea processes and Current amount not equal!"
        );
    }
    {
        let c = count_processes(crate::Process::UpdaterDaemon).await;
        info!("Completed Test Updater Instances: {}", c);
        assert_eq!(
            c, initial_updater_c,
            "Initial Updater processes and Current amount not equal!"
        );
    }
}
