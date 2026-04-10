KERNELS_DIR="/var/lib/chelsea/kernels"
KERNELS_S3_TIER=${KERNELS_S3_TIER:-development}

sudo mkdir -p "${KERNELS_DIR}"
sudo aws s3 cp s3://sh.vers.kernels/firecracker "${KERNELS_DIR}/default.bin"
sudo aws s3 cp s3://sh.vers.kernels/cloud_hypervisor $KERNELS_DIR/ch.bin
