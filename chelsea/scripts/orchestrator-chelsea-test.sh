#!/usr/bin/env bash

TIME=$(date +%s)

./commands.sh cleanup
mkdir -p chelsea-testlogs
RUSTFLAGS="--cfg orch_test" cargo build --release -p chelsea
cd crates/chelsea
sudo ../../target/release/chelsea > ../../chelsea-testlogs/${TIME}.txt 2>&1 &
echo "Chelsea launched, logs = ./chelsea-testlogs/${TIME}.txt"
sleep 7
cd ../orchestrator
RUST_BACKTRACE=full cargo nextest r --test routes --features with-chelsea --release ; ../../commands.sh cleanup
