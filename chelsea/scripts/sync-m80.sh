#!/bin/bash

# Get the instance IP address
INSTANCE_IP=$(aws ec2 describe-instances \
    --filters "Name=tag:Name,Values=m80-dev" \
    --query 'Reservations[*].Instances[*].PublicIpAddress' \
    --output text)

if [ -z "$INSTANCE_IP" ]; then
    echo "Error: Could not find public IP for m80-dev"
    exit 1
fi

# Check if fswatch is installed
if ! command -v fswatch >/dev/null 2>&1; then
    echo "Error: fswatch is required. Install it with:"
    echo "  MacOS: brew install fswatch"
    exit 1
fi

# Directory to sync (default to current directory)
SOURCE_DIR="."
# Default remote directory
REMOTE_DIR="~/project"

# Function to perform sync
do_sync() {
    rsync -avz --delete \
        --exclude '.git' \
        --exclude 'node_modules' \
        --exclude 'target/' \
        -e "ssh -i ./firecracker.pem" \
        "$SOURCE_DIR/" \
        "ubuntu@${INSTANCE_IP}:${REMOTE_DIR}/"
    echo "$(date): Sync completed"
}

echo "Initial sync..."
do_sync

echo "Watching for changes..."
# On Linux:
# inotifywait -m -r -e modify,create,delete,move "$SOURCE_DIR" | while read -r directory events filename; do
#     # Add a small delay to batch rapid changes
#     sleep 0.5
#     echo "Change detected in $directory$filename"
#     do_sync
# done

# Uncomment below and comment above if on MacOS:
fswatch -o "$SOURCE_DIR" | while read f; do
    sleep 0.5
    echo "Change detected"
    do_sync
done 