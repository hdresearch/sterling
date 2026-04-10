#!/usr/bin/env bash

echo "This script will purge the rbd pool of images and snapshots. This is meant just for debugging and must be run directly on the storage nodes."
echo "Type 'yes' to continue."
read input
if [[ "$input" != "yes" ]]; then
    echo "Aborting."
    exit 1
fi

ceph config set mon mon_allow_pool_delete true
ceph osd pool delete rbd rbd --yes-i-really-really-mean-it
ceph osd pool create rbd
rbd pool init
ceph config set mon mon_allow_pool_delete false