#!/bin/bash

set -eu

# ┌────────────────────────────────────────────────────────────────────────────┐
# │                                                                            │
# │   Runs everything on a single machine using tmux for process management.   │
# │                                                                            │
# └────────────────────────────────────────────────────────────────────────────┘

if [ "$(id -u)" -ne 0 ]; then
    echo "This script must be run as root."
    exit 1
fi

script_dir="$(cd "$(dirname "$0")" && pwd)"
project_dir="$(dirname "$script_dir")"

# Make sure we are at the repository root.
cd "$(dirname "$0")/../"
deps_file=./scripts/single-node/deps.sh
deps_done_file=/var/lib/chelsea/sne/deps.sh.done
sne_logs_dir=/var/lib/vers-sne/logs

config_src="$project_dir/config"
config_dst="/etc/vers"

config_src="$project_dir/config"
config_dst="/etc/vers"

export SESSION_NAME=vers
export RUST_LOG="${RUST_LOG:-info}"

tmux_attach=true
preserve_config=false
hypervisor=firecracker

usage() {
    echo ""
    echo "Usage: $0 start [-d] [--preserve-config] [--hypervisor <type>] | packages | deps | shutdown | nuke | test"
    echo ""
    echo "start: Starts a tmux session, runs each process in a tmux window, and "
    echo "       attaches to that session (unless -d is set)"
    echo ""
    echo "       --hypervisor <type>  Select hypervisor: firecracker (default) or cloud-hypervisor"
    echo ""
    echo "deps: Force re-installation of dependencies"
    echo ""
    echo "shutdown: Shut stuff down — gentler than Nuke, some stat is preserved"
    echo "          good for everyday use, use Nuke when things get weird"
    echo ""
    echo "nuke: Tear it down and start fresh"
    echo ""
    echo "test: Smoke test. These are rough-and-ready sanity checks. Read the"
    echo "      output carefully!"
    echo ""
    echo "For support hit up Jordan"
    echo ""
    exit
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        -d)
            # After spawning the tmux session, do not attach to it
            tmux_attach=false
            shift
            ;;
        --preserve-config)
            # Do not copy the contents of $config_src to $config_dst; used to preserve customizations
            preserve_config=true
            shift
            ;;
        --hypervisor)
            # Select hypervisor type: firecracker or cloud-hypervisor
            hypervisor="$2"
            shift 2
            ;;
        *)
            # Save non-flag arguments (like 'start', 'shutdown', etc.)
            args+=("$1")
            shift
            ;;
    esac
done
set -- "${args[@]:-}"

tmux_check() {
    if tmux has-session -t "$SESSION_NAME" 2>/dev/null; then
        echo "Session '$SESSION_NAME' already exists!"
        echo "Please manually shut it and all subprocesses down"
        exit 1
    fi
}

tmux_create() {
    echo ""
    echo "────────────────────────────────────────"
    echo "  Creating tmux session: $SESSION_NAME"
    echo "────────────────────────────────────────"
    echo ""
    tmux new-session -d -s "$SESSION_NAME"
}

complete() {
    echo ""
    echo "────────────────────────────────────────"
    echo "          Mission Accomplished"
    echo "────────────────────────────────────────"
    echo ""
    exit
}

install_packages() {
    ./scripts/single-node/packages.sh
}

always_install_deps() {
    ./scripts/single-node/deps.sh
}

install_deps() {
    if ! md5sum --check $deps_done_file 2>/dev/null; then
        always_install_deps
        mkdir -p $(dirname $deps_done_file)
        md5sum ${deps_file} | tee ${deps_done_file} > /dev/null
    fi
}

setup_ceph() {
    # Test/Dev credentials that match the pre-setup Ceph
    cat <<"EOT" > /etc/ceph/ceph.conf
[global]
    fsid = be4d1849-9fc1-11f0-a026-0600ac100002
    mon_host = [v2:172.16.0.2:3300/0,v1:172.16.0.2:6789/0]

EOT

    cat <<"EOT" > /etc/ceph/ceph.client.chelsea.keyring
[client.chelsea]
    key = AQBt0t5o1o+ABxAA0qEJ8XZ4uQfRFZE+hiIUiA==

EOT

    # Test if the extracted test cluster exists
    if [ ! -d /srv/ceph-test-cluster ]; then
        local olddir=$(pwd)
        cd /srv

        # Grab the test Ceph cluster only if the archive file doesn't exist
        if [ ! -f ceph-test-cluster.tar.zst ]; then
            echo "Cluster archive not found; fetching"
            curl -O https://hdr-devops-public.s3.us-east-1.amazonaws.com/ceph-test-cluster.tar.zst
        fi

        # expands to ~51G
        echo "Extracting cluster archive; this will take a while"
        tar -xf ceph-test-cluster.tar.zst
        cd $olddir
    fi
}

start_ceph() {
    tmux new-window -t "$SESSION_NAME" -n "ceph"
    tmux send-keys -t "$SESSION_NAME:ceph" "cd /srv/ceph-test-cluster" C-m
    tmux send-keys -t "$SESSION_NAME:ceph" "nohup ./start-firecracker.sh 2>&1 &" C-m
    tmux send-keys -t "$SESSION_NAME:ceph" "./start-vm.sh" C-m
}

setup_pg() {
    ./pg/scripts/setup-dev-db.sh
}

start_pg() {
    tmux new-window -t "$SESSION_NAME" -n "pg"

    mkcert -install
    tmux send-keys -t "$SESSION_NAME:pg" "source ./pg/scripts/insert-vers-tls-db.sh" C-m

    tmux send-keys -t "$SESSION_NAME:pg" "./pg/scripts/connect-dev-db.sh" C-m
}

setup_namespaces () {
    # Run Chelsea in the root namespace
    # So do nothing here

    # Orchestrator
    ip netns add orchestrator
    ip netns exec orchestrator ip link set lo up
    ip link add vethorch1 type veth peer vethorch0 netns orchestrator
    ip netns exec orchestrator ip addr add 204.0.0.6/30 dev vethorch0
    ip netns exec orchestrator ip link set dev vethorch0 up
    ip addr add 204.0.0.5/30 dev vethorch1
    ip link set dev vethorch1 up
    ip netns exec orchestrator ip route add default via 204.0.0.5

    # Proxy
    ip netns add proxy
    ip netns exec proxy ip link set lo up
    ip link add vethproxy1 type veth peer vethproxy0 netns proxy
    ip netns exec proxy ip addr add 204.0.0.2/30 dev vethproxy0
    ip netns exec proxy ip link set dev vethproxy0 up
    ip addr add 204.0.0.1/30 dev vethproxy1
    ip link set dev vethproxy1 up
    ip netns exec proxy ip route add default via 204.0.0.1

    # Setup /etc/resolv.conf
    if ! grep -q '1.1.1.1' /etc/resolv.conf; then
        if [ -L /etc/resolv.conf ]; then
            unlink /etc/resolv.conf
        fi
        cat /run/systemd/resolve/stub-resolv.conf > /etc/resolve.conf
        echo 'nameserver 1.1.1.1' >> /etc/resolv.conf
        echo 'nameserver 8.8.8.8' >> /etc/resolv.conf
    fi

    # Assign an IP to the primary network interface
    # so that all services can reach postgres
    if ip a | grep -q 204.0.0.8; then
        echo Address 204.0.0.8 already assigned
    else
        ip addr add 204.0.0.8/24 dev $(ip -json a | jq -r .[1].ifname)
    fi
}

start_chelsea() {
    tmux new-window -t "$SESSION_NAME" -n "chelsea"
    local log_file="${sne_logs_dir}/chelsea.log"
    echo "Starting chelsea with hypervisor: $hypervisor"
    tmux send-keys -t "$SESSION_NAME:chelsea" "chelsea_hypervisor_type=${hypervisor} ./target/release/chelsea | tee $log_file" C-m
}

start_orchestrator() {
    tmux new-window -t "$SESSION_NAME" -n "orchestrator"
    local log_file="${sne_logs_dir}/orchestrator.log"
    tmux send-keys -t "$SESSION_NAME:orchestrator" "ip netns exec orchestrator ./target/release/orchestrator | tee $log_file" C-m
}

start_proxy() {
    tmux new-window -t "$SESSION_NAME" -n "proxy"
    local log_file="${sne_logs_dir}/proxy.log"
    tmux send-keys -t "$SESSION_NAME:proxy" "ip netns exec proxy ./target/release/proxy | tee $log_file" C-m
}

copy_config() {
    rm -r "$config_dst" || true
    mkdir -p "$config_dst"
    cp "${config_src}"/* "$config_dst"
}

wait_for_ceph() {
    echo "Waiting for Ceph to become healthy..."
    local max_attempts=60
    local attempt=0
    while [ $attempt -lt $max_attempts ]; do
        if ceph --user chelsea status 2>/dev/null | grep -qE 'HEALTH_OK|HEALTH_WARN'; then
            echo "Ceph is healthy"
            return 0
        fi
        attempt=$((attempt + 1))
        sleep 5
    done
    echo "ERROR: Ceph did not become healthy within timeout"
    return 1
}

reset_ceph() {
    echo "Resetting Ceph and recreating the 'default' base image."
    "$script_dir/single-node/reset-ceph.sh"
}

# Run setup logic, then start processes in individual tmux widows
start() {
    install_packages
    install_deps

    mkdir -p $sne_logs_dir

    # Run setup logic
    setup_ceph
    setup_pg
    setup_namespaces

    if [[ "$preserve_config" != true ]]; then
        copy_config
    fi

    # Start tmux windows
    tmux_create
    start_ceph
    start_pg

    # Wait for Ceph to be healthy, then reset the pool with a fresh 'default' image.
    wait_for_ceph
    reset_ceph

    start_chelsea
    start_orchestrator
    start_proxy

    # Finishing up
    # ─────────────────────────────────────────────────────────────────────────
    tmux send-keys -t "$SESSION_NAME:bash" "exit"  C-m
    if [[ "$tmux_attach" = true ]]; then
        tmux attach -t "$SESSION_NAME"
    fi
    complete
}

shutdown() {
    set +e

    # Unmap all RBD devices before tearing down Ceph.
    # Stale kernel mappings cause "pool does not exist" on the next run.
    echo "Unmapping RBD devices..."
    for dev in /dev/rbd*; do
        [ -b "$dev" ] && rbd --id chelsea device unmap "$dev"
    done

    docker-compose -f pg/docker-compose.yml down --volumes
    ./commands.sh cleanup
    ssh -i $(ls /srv/ceph-test-cluster/*.id_rsa | tail -1) -o StrictHostKeyChecking=accept-new root@172.16.0.2 "reboot"
    echo "Waiting 10 seconds for Ceph to shut down..."
    sleep 10
    ip link del wgchelsea
    ip netns del orchestrator
    ip netns del proxy
    ip addr del 204.0.0.8/24 dev $(ip -json a | jq -r .[1].ifname)
    tmux kill-session -t "$SESSION_NAME"
}

nuke() {
    set +e
    shutdown
    rm -r /srv/ceph-test-cluster
    cargo clean
    complete
}

test() {
    echo "────────────────────────────────────────"
    echo "  Running Tests!"
    echo ""
    echo " If this hangs then Ceph is likely borked"
    echo "────────────────────────────────────────"
    echo ""
    ./scripts/single-node/test/deps.sh
    ./scripts/single-node/test/postgres.sh
    ./scripts/single-node/test/ceph.sh
    ./scripts/single-node/test/chelsea.sh
    ./scripts/single-node/test/orchestrator.sh
    ./scripts/single-node/test/proxy.sh
    ./commands.sh cleanup
    complete
}

[ $# -lt 1 ] && usage
COMMAND=$1

case $COMMAND in
    start)
        tmux_check
        start
    ;;
    packages)
        install_packages
        complete
    ;;
    deps)
        install_packages
        always_install_deps
        complete
    ;;
    shutdown)
        shutdown
        complete
    ;;
    test)
        test
    ;;
    nuke)
        nuke
    ;;
    *)
        usage
    ;;
esac
