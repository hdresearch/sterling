#!/bin/bash

set -eu

ARCHIVE_NAME=ceph-test-cluster.tar.zst

# Local paths
CLUSTER_DIR=/srv/ceph-test-cluster
CLUSTER_ARCHIVE=/srv/${ARCHIVE_NAME}

# Remote paths
BUCKET=hdr-devops-public
ARCHIVE_URI=s3://${BUCKET}/${ARCHIVE_NAME}
ARCHIVE_URI_OLD=s3://${BUCKET}/${ARCHIVE_NAME}.old

echo -n "This script will tar and upload the current state of your SNE Ceph cluster (${CLUSTER_DIR}) to S3. "
echo -n 'Please ensure that the cluster is not currently running, and that it is currently in the state you wish to '
echo -n 'push. For instance, you may wish to run ./scripts/single-node/reset-ceph.sh to clean any residual images/'
echo 'snaps first.'

echo -n 'Are you sure you wish to proceed? [y/N]: '
read response

if [[ "${response,,}" != 'y' ]]; then
    echo 'Exiting'
    exit 0
fi

set -x

if [[ -f ${CLUSTER_ARCHIVE} ]]; then
    sudo rm ${CLUSTER_ARCHIVE}
fi

sudo tar --zstd -C $(dirname ${CLUSTER_DIR}) -cf ${CLUSTER_ARCHIVE} $(basename ${CLUSTER_DIR})
aws s3 mv ${ARCHIVE_URI} ${ARCHIVE_URI_OLD}
aws s3 cp ${CLUSTER_ARCHIVE} ${ARCHIVE_URI}