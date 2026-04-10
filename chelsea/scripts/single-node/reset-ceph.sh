# Deletes, re-initializes the `rbd` pool, then recreates the `default` base image on the SNE Ceph cluster, assumed to run at $CEPH_SSH_HOST.

set -e

CEPH_SSH_KEY_FILE=$(sudo ls /srv/ceph-test-cluster/*.id_rsa | tail -1)
CEPH_SSH_HOST=root@172.16.0.2

CREATE_DEFAULT_IMAGE_SH="$(cd "$(dirname "$0")/.." && pwd)/base-image/create-default-image.sh"

# Execute via ssh; SNE does not have admin permissions on cluster.
# Disable strict host key checking for CI environments where known_hosts may not be pre-populated.
ssh -o StrictHostKeyChecking=no -o UserKnownHostsFile=/dev/null -i "$CEPH_SSH_KEY_FILE" "$CEPH_SSH_HOST" bash <<'EOF'
set -e
ceph config set mon mon_allow_pool_delete true
ceph osd pool delete rbd rbd --yes-i-really-really-mean-it
ceph osd pool create rbd
rbd pool init
ceph config set mon mon_allow_pool_delete false
EOF

export PATH="$HOME/.cargo/bin:$PATH"

sudo ${CHELSEA_AGENT_BIN:+CHELSEA_AGENT_BIN="$CHELSEA_AGENT_BIN"} PATH=$PATH $CREATE_DEFAULT_IMAGE_SH
