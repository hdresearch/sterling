# This script will download the default rootfs from S3 and create an RBD image at rbd --id chelsea/default containing its contents.
# Additionally, create a base snap with name-by-convention: {image_name}@chelsea_base_image
# This is to be run on a ceph storage node.

# Dependencies required: squashfs-tools wget gzip

set -eu

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

ROOTFS_DIR=/var/lib/chelsea/rootfs
ROOTFS_NAME=default

IMAGE_NAME=default
SNAP_NAME=chelsea_base_image

CONFIGURE_IMAGE_SH="$SCRIPT_DIR/configure-image.sh"
DEFAULT_AGENT_BIN="$REPO_ROOT/target/release/chelsea-agent"

if [ -z "${CHELSEA_AGENT_BIN:-}" ]; then
    CHELSEA_AGENT_BIN="$DEFAULT_AGENT_BIN"
fi

if [ ! -f "$CHELSEA_AGENT_BIN" ]; then
    cat >&2 <<EOF
chelsea-agent binary not found at '$CHELSEA_AGENT_BIN'.
Set CHELSEA_AGENT_BIN to a valid path, or ensure cargo/nix is installed so the script can build it.
EOF
    exit 1
fi

# Download the rootfs if not present
SQUASHFS_DIR="/srv/vers-sne"
SQUASHFS_GZ="$SQUASHFS_DIR/ubuntu-24.04.squashfs.upstream.gz"
SQUASHFS="$SQUASHFS_DIR/ubuntu-24.04.squashfs.upstream"

mkdir -p "$SQUASHFS_DIR"

if [ ! -f "$SQUASHFS" ]; then
    if [ ! -f "$SQUASHFS_GZ" ]; then
        wget -O "$SQUASHFS_GZ" "https://vers-development-use1-az4-x-s3.s3.us-east-1.amazonaws.com/vmbase/ubuntu-24.04.squashfs.gz"
    fi
    gunzip -k "$SQUASHFS_GZ"  # keep original .gz as well
fi

if [ ! -d "squashfs-root" ]; then
    unsquashfs "$SQUASHFS"
fi

CHELSEA_AGENT_BIN="$CHELSEA_AGENT_BIN" FS_ROOT=squashfs-root $CONFIGURE_IMAGE_SH

# Move extracted rootfs 
rm -rf ${ROOTFS_DIR}/${ROOTFS_NAME}
mkdir -p ${ROOTFS_DIR}
mv squashfs-root ${ROOTFS_DIR}/${ROOTFS_NAME}

# Create new image and copy rootfs contents to it
rbd --id chelsea create $IMAGE_NAME --size 512M

DEVICE=$(rbd --id chelsea device map $IMAGE_NAME)
trap "rbd --id chelsea device unmap $IMAGE_NAME" EXIT

mkfs.ext4 $DEVICE
mkdir -p /mnt/tmp
mount $DEVICE /mnt/tmp
trap "umount /mnt/tmp && rbd --id chelsea device unmap $IMAGE_NAME" EXIT

cp -r ${ROOTFS_DIR}/${ROOTFS_NAME}/* /mnt/tmp

umount /mnt/tmp && rbd --id chelsea device unmap $IMAGE_NAME
trap "" EXIT

rbd --id chelsea snap create $IMAGE_NAME@$SNAP_NAME
rbd --id chelsea snap protect $IMAGE_NAME@$SNAP_NAME
