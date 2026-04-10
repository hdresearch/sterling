#!/bin/bash
# This script fetches and configures the rootfs

set -eu

ROOTFS_S3_TIER=${ROOTFS_S3_TIER:-development}

# Expected env vars:
# ROOTFS_S3_TIER - optional, must be "development" or "production"
# ROOTFS_DIR - directory to store the rootfs
# ROOTFS_NAME - name for the rootfs directory

cleanup() {
  if [[ -v NO_FETCH_FS_CLEANUP ]]; then
    echo "Cleanup disabled via NO_FETCH_FS_CLEANUP."
  else
    sudo rm -r squashfs-root
    sudo rm ubuntu-24.04.squashfs.upstream
  fi
}
trap cleanup EXIT

# Create the rootfs directory
sudo mkdir -p ${ROOTFS_DIR}

# Download the rootfs
wget -O ubuntu-24.04.squashfs.upstream.gz "https://vers-${ROOTFS_S3_TIER}-use1-az4-x-s3.s3.us-east-1.amazonaws.com/vmbase/ubuntu-24.04.squashfs.gz"

# Next, uncompress the downloaded file
gunzip ubuntu-24.04.squashfs.upstream.gz

# Extract the squashfs image
unsquashfs ubuntu-24.04.squashfs.upstream

# Configure the FS with necessary scripts, services, etc.
FS_ROOT=squasfs-root ./configure-image.sh

# Reset owners on everything in the squashfs file system.
sudo chown -R root:root squashfs-root

# Customize the file system with the Linux kernel drivers, their supporting files, etc.
# TODO: Temporarily commented out until this is more smoothly integrated (moving to Ceph means we currently don't have this in the pipeline.)
# See: https://github.com/hdresearch/chelsea/issues/516
# dotnet exec --roll-forward Major ${CHELSEA_BIN_DIR}/EagleShell.dll -file ${CHELSEA_BIN_DIR}/../tools/deployToFileSystem.eagle ${CHELSEA_BIN_DIR} $(pwd)/squashfs-root

# Make sure target data directory is freshly clean and available.
sudo rm -rf ${ROOTFS_DIR}/${ROOTFS_NAME}
sudo mkdir -p ${ROOTFS_DIR}/${ROOTFS_NAME}

# Move the squashfs rootfs to the target data directory.
if [[ -v NO_FETCH_FS_MOVE ]]; then
  echo "Root file system contents copied to ${ROOTFS_DIR}/${ROOTFS_NAME}."
  sudo cp -aR squashfs-root/. ${ROOTFS_DIR}/${ROOTFS_NAME}/ 2>/dev/null || true
else
  sudo mv squashfs-root/* ${ROOTFS_DIR}/${ROOTFS_NAME}/
  sudo mv squashfs-root/.* ${ROOTFS_DIR}/${ROOTFS_NAME}/ 2>/dev/null || true
  echo "Root file system contents moved to ${ROOTFS_DIR}/${ROOTFS_NAME}."
fi
