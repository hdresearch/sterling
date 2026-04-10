KERNELS_DIR="/var/lib/chelsea/kernels"
KERNELS_S3_TIER=${KERNELS_S3_TIER:-development}
KERNEL_NAME="default.bin"

ARCH="$(uname -m)"

sudo mkdir -p "${KERNELS_DIR}"
curl "https://vers-${KERNELS_S3_TIER}-use1-az4-x-s3.s3.us-east-1.amazonaws.com/vmbase/vmlinux.gz" -o "${KERNEL_NAME}.gz"
gunzip "${KERNEL_NAME}.gz"
sudo mv "./${KERNEL_NAME}" "${KERNELS_DIR}/${KERNEL_NAME}"
