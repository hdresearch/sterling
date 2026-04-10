//! Benchmark comparing download/upload speeds across three implementations:
//!   1. AWS SDK (single-stream get_object / put_object)
//!   2. AWS CLI (`aws s3 cp`)
//!   3. vers_s3 (parallel range GETs / multipart uploads)
//!
//! Run with: `cargo test -p vers_s3 --test bench_compare -- --nocapture --ignored`
//!
//! Accepts env vars:
//!   BENCH_SIZES_MIB  — comma-separated sizes to test (default: "32,128,512")
//!   BENCH_ITERATIONS — number of iterations per size (default: 3)

use aws_sdk_s3::primitives::ByteStream;
use aws_sdk_s3::Client;
use std::path::Path;
use std::time::{Duration, Instant};
use vers_s3::TransferConfig;

const TEST_BUCKET: &str = "vers-commits-dev--use1-az4--x-s3";
const TEST_PREFIX: &str = "vers_s3_bench/";

async fn make_client() -> Client {
    let config = aws_config::load_from_env().await;
    Client::new(&config)
}

fn bench_key(label: &str) -> String {
    format!("{TEST_PREFIX}{label}")
}

async fn cleanup(client: &Client, key: &str) {
    let _ = client
        .delete_object()
        .bucket(TEST_BUCKET)
        .key(key)
        .send()
        .await;
}

/// Generate deterministic test data of the given size.
fn generate_data(size_bytes: usize) -> Vec<u8> {
    let mut data = Vec::with_capacity(size_bytes);
    let mut i: u32 = 0;
    while data.len() + 4 <= size_bytes {
        data.extend_from_slice(&i.to_le_bytes());
        i = i.wrapping_add(1);
    }
    // Fill remaining bytes
    while data.len() < size_bytes {
        data.push(0);
    }
    data
}

// ─── Downloaders ───

async fn download_sdk(client: &Client, key: &str, dst: &Path) -> Result<(), String> {
    let resp = client
        .get_object()
        .bucket(TEST_BUCKET)
        .key(key)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    let mut file = tokio::fs::File::create(dst)
        .await
        .map_err(|e| e.to_string())?;
    let mut stream = resp.body.into_async_read();
    tokio::io::copy(&mut stream, &mut file)
        .await
        .map_err(|e| e.to_string())?;
    tokio::io::AsyncWriteExt::flush(&mut file)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

async fn download_cli(key: &str, dst: &Path) -> Result<(), String> {
    let s3_uri = format!("s3://{TEST_BUCKET}/{key}");
    let output = tokio::process::Command::new("aws")
        .arg("s3")
        .arg("cp")
        .arg(&s3_uri)
        .arg(dst)
        .output()
        .await
        .map_err(|e| e.to_string())?;

    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }
    Ok(())
}

async fn download_vers_s3(
    client: &Client,
    key: &str,
    dst: &Path,
    config: &TransferConfig,
) -> Result<(), String> {
    vers_s3::download_file(client, TEST_BUCKET, key, dst, config)
        .await
        .map_err(|e| e.to_string())
}

// ─── Uploaders ───

async fn upload_sdk(client: &Client, key: &str, src: &Path) -> Result<(), String> {
    let file_size = tokio::fs::metadata(src)
        .await
        .map_err(|e| e.to_string())?
        .len();
    let body = ByteStream::from_path(src)
        .await
        .map_err(|e| e.to_string())?;

    client
        .put_object()
        .bucket(TEST_BUCKET)
        .key(key)
        .body(body)
        .content_length(file_size as i64)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

async fn upload_cli(key: &str, src: &Path) -> Result<(), String> {
    let s3_uri = format!("s3://{TEST_BUCKET}/{key}");
    let output = tokio::process::Command::new("aws")
        .arg("s3")
        .arg("cp")
        .arg(src)
        .arg(&s3_uri)
        .output()
        .await
        .map_err(|e| e.to_string())?;

    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }
    Ok(())
}

async fn upload_vers_s3(
    client: &Client,
    key: &str,
    src: &Path,
    config: &TransferConfig,
) -> Result<(), String> {
    vers_s3::upload_file(client, TEST_BUCKET, key, src, config)
        .await
        .map_err(|e| e.to_string())
}

// ─── Bench harness ───

struct BenchResult {
    label: String,
    size_mib: usize,
    durations: Vec<Duration>,
}

impl BenchResult {
    fn mean_ms(&self) -> f64 {
        let sum: f64 = self
            .durations
            .iter()
            .map(|d| d.as_secs_f64() * 1000.0)
            .sum();
        sum / self.durations.len() as f64
    }

    fn stddev_ms(&self) -> f64 {
        let mean = self.mean_ms();
        let variance: f64 = self
            .durations
            .iter()
            .map(|d| {
                let ms = d.as_secs_f64() * 1000.0;
                (ms - mean).powi(2)
            })
            .sum::<f64>()
            / self.durations.len() as f64;
        variance.sqrt()
    }

    fn throughput_mib_s(&self) -> f64 {
        let mean_secs = self.mean_ms() / 1000.0;
        if mean_secs > 0.0 {
            self.size_mib as f64 / mean_secs
        } else {
            0.0
        }
    }
}

#[tokio::test]
#[ignore]
async fn bench_download() {
    let client = make_client().await;
    let config = TransferConfig::default();

    let sizes_mib: Vec<usize> = std::env::var("BENCH_SIZES_MIB")
        .unwrap_or_else(|_| "32,128,512".to_string())
        .split(',')
        .map(|s| s.trim().parse().expect("invalid size"))
        .collect();

    let iterations: usize = std::env::var("BENCH_ITERATIONS")
        .unwrap_or_else(|_| "3".to_string())
        .parse()
        .expect("invalid iteration count");

    let dir = tempfile::tempdir().unwrap();
    let mut all_results: Vec<BenchResult> = Vec::new();

    for &size_mib in &sizes_mib {
        let size_bytes = size_mib * 1024 * 1024;
        let key = bench_key(&format!("dl_{size_mib}mib.bin"));

        // Upload test data
        println!("Uploading {size_mib} MiB test data...");
        let data = generate_data(size_bytes);
        let src = dir.path().join(format!("src_{size_mib}.bin"));
        tokio::fs::write(&src, &data).await.unwrap();
        upload_cli(&key, &src).await.unwrap();

        // --- SDK ---
        let mut sdk_durations = Vec::new();
        for i in 0..iterations {
            let dst = dir.path().join(format!("sdk_{size_mib}_{i}.bin"));
            let start = Instant::now();
            download_sdk(&client, &key, &dst).await.unwrap();
            sdk_durations.push(start.elapsed());
            let _ = tokio::fs::remove_file(&dst).await;
        }
        all_results.push(BenchResult {
            label: "sdk".to_string(),
            size_mib,
            durations: sdk_durations,
        });

        // --- CLI ---
        let mut cli_durations = Vec::new();
        for i in 0..iterations {
            let dst = dir.path().join(format!("cli_{size_mib}_{i}.bin"));
            let start = Instant::now();
            download_cli(&key, &dst).await.unwrap();
            cli_durations.push(start.elapsed());
            let _ = tokio::fs::remove_file(&dst).await;
        }
        all_results.push(BenchResult {
            label: "cli".to_string(),
            size_mib,
            durations: cli_durations,
        });

        // --- vers_s3 ---
        let mut vers_durations = Vec::new();
        for i in 0..iterations {
            let dst = dir.path().join(format!("vers_{size_mib}_{i}.bin"));
            let start = Instant::now();
            download_vers_s3(&client, &key, &dst, &config)
                .await
                .unwrap();
            vers_durations.push(start.elapsed());
            let _ = tokio::fs::remove_file(&dst).await;
        }
        all_results.push(BenchResult {
            label: "vers_s3".to_string(),
            size_mib,
            durations: vers_durations,
        });

        cleanup(&client, &key).await;
    }

    // Print results
    println!("\n{}", "=".repeat(80));
    println!("DOWNLOAD BENCHMARK RESULTS ({iterations} iterations per test)");
    println!("{}", "=".repeat(80));
    println!(
        "{:<10} {:>10} {:>14} {:>12} {:>14}",
        "impl", "size_mib", "mean_ms", "stddev_ms", "throughput"
    );
    println!("{:-<62}", "");
    for r in &all_results {
        println!(
            "{:<10} {:>10} {:>14.1} {:>12.1} {:>11.1} MiB/s",
            r.label,
            r.size_mib,
            r.mean_ms(),
            r.stddev_ms(),
            r.throughput_mib_s(),
        );
    }
    println!();
}

#[tokio::test]
#[ignore]
async fn bench_upload() {
    let client = make_client().await;
    let config = TransferConfig::default();

    let sizes_mib: Vec<usize> = std::env::var("BENCH_SIZES_MIB")
        .unwrap_or_else(|_| "32,128,512".to_string())
        .split(',')
        .map(|s| s.trim().parse().expect("invalid size"))
        .collect();

    let iterations: usize = std::env::var("BENCH_ITERATIONS")
        .unwrap_or_else(|_| "3".to_string())
        .parse()
        .expect("invalid iteration count");

    let dir = tempfile::tempdir().unwrap();
    let mut all_results: Vec<BenchResult> = Vec::new();

    for &size_mib in &sizes_mib {
        let size_bytes = size_mib * 1024 * 1024;

        // Generate source file
        println!("Generating {size_mib} MiB source file...");
        let data = generate_data(size_bytes);
        let src = dir.path().join(format!("src_{size_mib}.bin"));
        tokio::fs::write(&src, &data).await.unwrap();

        // --- SDK ---
        let mut sdk_durations = Vec::new();
        for i in 0..iterations {
            let key = bench_key(&format!("ul_sdk_{size_mib}_{i}.bin"));
            let start = Instant::now();
            upload_sdk(&client, &key, &src).await.unwrap();
            sdk_durations.push(start.elapsed());
            cleanup(&client, &key).await;
        }
        all_results.push(BenchResult {
            label: "sdk".to_string(),
            size_mib,
            durations: sdk_durations,
        });

        // --- CLI ---
        let mut cli_durations = Vec::new();
        for i in 0..iterations {
            let key = bench_key(&format!("ul_cli_{size_mib}_{i}.bin"));
            let start = Instant::now();
            upload_cli(&key, &src).await.unwrap();
            cli_durations.push(start.elapsed());
            cleanup(&client, &key).await;
        }
        all_results.push(BenchResult {
            label: "cli".to_string(),
            size_mib,
            durations: cli_durations,
        });

        // --- vers_s3 ---
        let mut vers_durations = Vec::new();
        for i in 0..iterations {
            let key = bench_key(&format!("ul_vers_{size_mib}_{i}.bin"));
            let start = Instant::now();
            upload_vers_s3(&client, &key, &src, &config).await.unwrap();
            vers_durations.push(start.elapsed());
            cleanup(&client, &key).await;
        }
        all_results.push(BenchResult {
            label: "vers_s3".to_string(),
            size_mib,
            durations: vers_durations,
        });
    }

    // Print results
    println!("\n{}", "=".repeat(80));
    println!("UPLOAD BENCHMARK RESULTS ({iterations} iterations per test)");
    println!("{}", "=".repeat(80));
    println!(
        "{:<10} {:>10} {:>14} {:>12} {:>14}",
        "impl", "size_mib", "mean_ms", "stddev_ms", "throughput"
    );
    println!("{:-<62}", "");
    for r in &all_results {
        println!(
            "{:<10} {:>10} {:>14.1} {:>12.1} {:>11.1} MiB/s",
            r.label,
            r.size_mib,
            r.mean_ms(),
            r.stddev_ms(),
            r.throughput_mib_s(),
        );
    }
    println!();
}

use std::sync::Arc;
use tokio::sync::Semaphore;

/// Network-only benchmark: downloads data and discards it (no disk writes).
/// This isolates network throughput from disk I/O.
#[tokio::test]
#[ignore]
async fn bench_network_only() {
    use aws_sdk_s3::presigning::PresigningConfig;
    use futures::StreamExt;

    let client = make_client().await;

    let sizes_mib: Vec<usize> = std::env::var("BENCH_SIZES_MIB")
        .unwrap_or_else(|_| "128,512,2048".to_string())
        .split(',')
        .map(|s| s.trim().parse().expect("invalid size"))
        .collect();

    let iterations: usize = std::env::var("BENCH_ITERATIONS")
        .unwrap_or_else(|_| "3".to_string())
        .parse()
        .expect("invalid iteration count");

    let dir = tempfile::tempdir().unwrap();
    let config = TransferConfig::default();

    println!("\n{}", "=".repeat(80));
    println!("NETWORK-ONLY BENCHMARK (no disk writes)");
    println!("{}", "=".repeat(80));
    println!(
        "{:<16} {:>10} {:>14} {:>12} {:>14}",
        "method", "size_mib", "mean_ms", "stddev_ms", "throughput"
    );
    println!("{:-<68}", "");

    for &size_mib in &sizes_mib {
        let size_bytes = size_mib * 1024 * 1024;
        let key = bench_key(&format!("netonly_{size_mib}mib.bin"));

        println!("Uploading {size_mib} MiB test data...");
        let data = generate_data(size_bytes);
        let src = dir.path().join(format!("src_{size_mib}.bin"));
        tokio::fs::write(&src, &data).await.unwrap();
        upload_cli(&key, &src).await.unwrap();

        let file_size = size_bytes as u64;

        // --- Single-stream SDK (network only) ---
        let mut single_durations = Vec::new();
        for _ in 0..iterations {
            let start = Instant::now();
            let resp = client
                .get_object()
                .bucket(TEST_BUCKET)
                .key(&key)
                .send()
                .await
                .unwrap();
            let bytes = resp.body.collect().await.unwrap();
            let _ = bytes; // consume the body
            single_durations.push(start.elapsed());
        }
        let r = BenchResult {
            label: "sdk_single".to_string(),
            size_mib,
            durations: single_durations,
        };
        println!(
            "{:<16} {:>10} {:>14.1} {:>12.1} {:>11.1} MiB/s",
            r.label,
            r.size_mib,
            r.mean_ms(),
            r.stddev_ms(),
            r.throughput_mib_s(),
        );

        // --- Parallel reqwest (network only, discard body) ---
        let presigned = client
            .get_object()
            .bucket(TEST_BUCKET)
            .key(&key)
            .presigned(PresigningConfig::expires_in(std::time::Duration::from_secs(3600)).unwrap())
            .await
            .unwrap();
        let presigned_url = presigned.uri().to_string();

        let http_client = reqwest::Client::builder()
            .pool_max_idle_per_host(config.max_concurrency)
            .tcp_nodelay(true)
            .pool_idle_timeout(std::time::Duration::from_secs(30))
            .build()
            .unwrap();

        let _ = http_client.head(&presigned_url).send().await;

        let chunk_size = std::cmp::max(
            8 * 1024 * 1024u64,
            file_size / config.max_concurrency as u64,
        )
        .min(config.chunk_size);
        let chunk_count = (file_size + chunk_size - 1) / chunk_size;

        let mut par_durations = Vec::new();
        for _ in 0..iterations {
            let start = Instant::now();
            let sem = Arc::new(Semaphore::new(config.max_concurrency));
            let mut handles = Vec::new();

            for ci in 0..chunk_count {
                let hc = http_client.clone();
                let url = presigned_url.clone();
                let sem = sem.clone();
                handles.push(tokio::spawn(async move {
                    let _permit = sem.acquire().await.unwrap();
                    let s = ci * chunk_size;
                    let e = std::cmp::min(s + chunk_size, file_size) - 1;
                    let resp = hc
                        .get(&url)
                        .header(reqwest::header::RANGE, format!("bytes={s}-{e}"))
                        .send()
                        .await
                        .unwrap();
                    let mut stream = resp.bytes_stream();
                    let mut total = 0u64;
                    while let Some(chunk) = stream.next().await {
                        total += chunk.unwrap().len() as u64;
                    }
                    total
                }));
            }

            let mut total_bytes = 0u64;
            for h in handles {
                total_bytes += h.await.unwrap();
            }
            assert_eq!(total_bytes, file_size);
            par_durations.push(start.elapsed());
        }
        let r = BenchResult {
            label: "reqwest_par".to_string(),
            size_mib,
            durations: par_durations,
        };
        println!(
            "{:<16} {:>10} {:>14.1} {:>12.1} {:>11.1} MiB/s",
            r.label,
            r.size_mib,
            r.mean_ms(),
            r.stddev_ms(),
            r.throughput_mib_s(),
        );

        // --- vers_s3 full (with disk writes) for comparison ---
        let mut vers_durations = Vec::new();
        for i in 0..iterations {
            let dst = dir.path().join(format!("vers_net_{size_mib}_{i}.bin"));
            let start = Instant::now();
            download_vers_s3(&client, &key, &dst, &config)
                .await
                .unwrap();
            vers_durations.push(start.elapsed());
            let _ = tokio::fs::remove_file(&dst).await;
        }
        let r = BenchResult {
            label: "vers_s3_disk".to_string(),
            size_mib,
            durations: vers_durations,
        };
        println!(
            "{:<16} {:>10} {:>14.1} {:>12.1} {:>11.1} MiB/s",
            r.label,
            r.size_mib,
            r.mean_ms(),
            r.stddev_ms(),
            r.throughput_mib_s(),
        );

        // --- CLI for comparison ---
        let mut cli_durations = Vec::new();
        for i in 0..iterations {
            let dst = dir.path().join(format!("cli_net_{size_mib}_{i}.bin"));
            let start = Instant::now();
            download_cli(&key, &dst).await.unwrap();
            cli_durations.push(start.elapsed());
            let _ = tokio::fs::remove_file(&dst).await;
        }
        let r = BenchResult {
            label: "cli".to_string(),
            size_mib,
            durations: cli_durations,
        };
        println!(
            "{:<16} {:>10} {:>14.1} {:>12.1} {:>11.1} MiB/s",
            r.label,
            r.size_mib,
            r.mean_ms(),
            r.stddev_ms(),
            r.throughput_mib_s(),
        );

        println!();
        cleanup(&client, &key).await;
    }
}
