#!/bin/sh

set -e

# ┌────────────────────────────────────────────────────────────────────────────┐
# │                                                                            │
# │       Steps that are run on a host to update to the latest version         │
# │                                                                            │
# └────────────────────────────────────────────────────────────────────────────┘

if [ "$(id -u)" -ne 0 ]; then
    echo "This script must be run as root."
    exit 1
fi

# What service runs on this host?
if [ -f "/etc/systemd/system/chelsea.service" ]; then
    kind=chelsea
elif [ -f "/etc/systemd/system/orchestrator.service" ]; then
    kind=orchestrator
elif [ -f "/etc/systemd/system/proxy.service" ]; then
    kind=proxy
else
    echo "Host isn't configured for any service!" >&2
    exit 127
fi

# Everything is based on being in the ubuntu home directory
cd /home/ubuntu/

# Obtain the latest release
aws s3 cp "s3://sh.vers.releases/staging-release.tar.zst" .

# Remove the old folder if it exists
sudo rm -rf result

# Extract it
tar -xf "staging-release.tar.zst"

# Copy the config into place
source=./result/bin/
target=/etc/vers/
mkdir -p $target
rm -rf ${target}*
cp -r "${source}"*.ini $target
cp -r "${source}"*.txt $target
cp /var/lib/chelsea/config.ini ${target}999-chelsea.ini

# Restart the service
sudo systemctl restart "${kind}.service"
