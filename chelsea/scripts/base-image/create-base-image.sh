# usage: ./create-bsae-image.sh <IMAGE_NAME> <SOURCE_DIR>
# This script will create an RBD image at rbd/${IMAGE_NAME} containing the contents of ${SOURCE_DIR}
# Additionally, create a base snap with name-by-convention: ${IMAGE_NAME}@chelsea_base_image

# NOTE: Portions of this script are duplicated logic from fetch_fs.sh and configure_fs.sh and as such may diverge from them.

usage() {
  echo "Usage: $0 <image_name> <source_dir>"
  exit 1
}

IMAGE_NAME="$1"
if [ -z "$IMAGE_NAME" ]; then
  usage
fi

SOURCE_DIR="$2"
if [ -z "$SOURCE_DIR" ]; then
  usage
fi
if [ ! -d "$SOURCE_DIR" ]; then
  echo "Error: Source directory '$SOURCE_DIR' does not exist."
  exit 1
fi

# Do not modify without good reason
SNAP_NAME=chelsea_base_image
RBD_USER=chelsea

set -eu

# Check if snap with name ${IMAGE_NAME}@${SNAP_NAME} exists
if rbd --id "$RBD_USER" snap ls rbd/"$IMAGE_NAME" 2>/dev/null | awk '{print $2}' | grep -qx "${SNAP_NAME}"; then
  echo "Snapshot ${IMAGE_NAME}@${SNAP_NAME} already exists. Exiting."
  exit 1
fi

# Configure the image with required scripts, services, etc.
FS_ROOT=$SOURCE_DIR ./configure-image.sh

# Create new image and copy rootfs contents to it
rbd --id $RBD_USER create $IMAGE_NAME --size 512M

DEVICE=$(rbd --id $RBD_USER device map $IMAGE_NAME)
trap "rbd --id $RBD_USER device unmap $IMAGE_NAME" EXIT

mkfs.ext4 $DEVICE
mkdir -p /mnt/tmp
mount $DEVICE /mnt/tmp
trap "umount /mnt/tmp && rbd --id $RBD_USER device unmap $IMAGE_NAME" EXIT

cp -r ${SOURCE_DIR}/* /mnt/tmp

umount /mnt/tmp && rbd --id $RBD_USER device unmap $IMAGE_NAME
trap "" EXIT

rbd --id $RBD_USER snap create $IMAGE_NAME@$SNAP_NAME
rbd --id $RBD_USER snap protect $IMAGE_NAME@$SNAP_NAME