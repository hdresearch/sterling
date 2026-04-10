## Cephadm Requirements
- Python 3
- Systemd
- Podman or Docker for running containers
- Time synchronization (such as Chrony or the legacy ntpd)
- LVM2 for provisioning storage devices

## Boostrapping requirements
- sshd running 

## No Ceph repo for Ubuntu "noble" (24.04)
This only applies when installing cephadm via curl, and as such I highly recommend apt instead, because those are up to date. But if we insist on curl over apt...  
I noticed that when using `cephadm add-repo`, I would get an error stating the following apt error:
```
The repository 'https://download.ceph.com/debian-squid noble Release' does not have a Release file.
```
I had to edit `/etc/apt/sources.list.d/ceph.list` to use the older `jammy` (Ubuntu 22.04) instead of `noble` (24.04). The other version available for squid appears to be `bookworm`, as of 2 Sep 2025. https://download.ceph.com/debian-squid/

## Process
```bash
apt install -y cephadm; apt install -y ceph-common
# Bootstrap a new cluster; for now we'll skip the monitor network because we do not have a CIDR range
# Bootstrapping creates a monitor and manager on the current node.
cephadm bootstrap --mon-ip 127.0.0.1 --skip-mon-network
ceph status  # Expected: HEALTH_WARN because there are 0 OSDs. Let's fix this

# Create a backing file w/ loop device
dd if=/dev/zero of=$BACKING_FILE_PATH bs=1GB count=$BACKING_FILE_SIZE_GB
losetup -f --show $BACKING_FILE_PATH

# Create an LVM logical volume on the loop device; Ceph doesn't recognize block devices as "raw block devices."
pvcreate $LOOP_DEVICE
vgcreate $VG_NAME $LOOP_DEVICE
lvcreate -l 100%FREE -n $LV_NAME $VG_NAME

# For single-node clusters (TESTING PURPOSES ONLY; prod cluster should have >= 3 nodes)
ceph config set osd osd_crush_chooseleaf_type 0  # Allow OSD to store objects on itself instead of forcing it to go elsewhere

# Add an OSD on the new device
ceph orch daemon add osd $(hostname):$LV_PATH  # Check ceph orch host ls for host name if it isn't equal to $(hostname)

# Adjust memory limit for OSD
ceph orch ps  # Notice that by default the OSD is given a large amount of the host memory.
ceph config set osd.0 osd_memory_target 4G  # If a different value is desired

# Create an OSD pool
ceph osd pool create $POOL_NAME

# In this example, we only have one OSD, so we cannot have more than one replica
ceph config set mon.$(hostname) mon_allow_pool_size_one true
ceph osd pool set pool0 size 1 --yes-i-really-mean-it
ceph osd pool set pool0 min_size 1

# Initialize pool as RBD pool. This may hang if the pool size is greater than the number of OSDs; check this with ceph osd pool get $POOL_NAME size (also min_size)
rbd pool init $POOL_NAME

# Not shown: Creating an RBD user. Without it, the pool will be accessed as admin. https://docs.ceph.com/en/reef/rbd/rados-rbd-cmds/#create-a-block-device-user
```
- Install `cephadm` and `ceph-common` via apt. (The latter includes the executables for `rbd`, `ceph`, `mount.ceph`, and others.)
- `cephadm bootstrap --mon-ip 127.0.0.1 --skip-mon-network --skip-monitoring-stack`; normally, the IP is expected to refer to a CIDR range. For now, we're running a single-node setup, so we'll configure the network at a later time. This is only necessary when boostrapping a new cluster. For the test cluster, the monitoring stack is overkill.
- Ignore if installed via apt. For executing ceph, rbd, mount.ceph, and other commands, you can use `cephadm shell` for interactive, and `cephadm shell -- COMMAND` for non-interactive. Note that `cephadm install ceph-common` won't install when using the version workaround above. The `jammy` libraries are not installable on `noble`, but so long as `cephadm` installs, it comes bundled with the necessary commands.
- `cephadm shell` invocations are very slow in the inferring steps and will redo these steps on every invocation. Use explicit values of `--fsid` (see `/etc/ceph/ceph.conf`) and `--config` (likely in `/var/lib/ceph/$FSID/mon.ip-$MONITOR_IP/config`) to skip these.

## Admin notes
- Ceph recommends 3 monitors for smaller clusters, and 5 for larger clusters. They do not recommend more than 7 monitors. An odd number is recommended due to the consensus model; whether you have 3 monitors or 4, you can only tolerate at most 1 going down. 2/4 is not enough for majority. So the 4th is... not terribly useful.
- We may not want to put managers and monitors on chelsea nodes. We want a stable number of monitors; managers are not mission critical, so we can just keep one alive and restart it on a new node if it is down for a while. Maybe two if desired.
- We DEFINITELY want to run OSDs on different nodes from chelsea. There are potential kernel conflicts that can happen when you mount kernel clients (such as mapped RBDs) on the same machine as OSDs. (From the [ceph docs](https://docs.ceph.com/en/reef/rados/troubleshooting/troubleshooting-pg/#one-node-cluster).) This is fine though! Because it pushes us towards an architecture where we can scale up our storage nodes independently of our compute nodes, which is really slick. Compute nodes can manipulate RBD clones/snapshots etc to their hearts' content and the storage cluster will manage them. It just means we want to ideally ensure that storage and compute nodes are on the same network - located in the same data center, or better yet, on the same rack.

## Test cluster specs
3x t4g.micro (1GB RAM, 1 vCPU) + 10 GB gp3 EBS volume
    1 - manager, monitor, osd
    2 - monitor, osd
    3 - monitor, osd
Cost:
    3x t4g.micro = 3x $.008/hr = $.024/hr
    3x 10 GB gp3 = 3x $1/mo = $3/mo

## Cleaning up Ceph
```bash
# Only on bootstrap node
cephadm cluster-rm --force --zap-osds --fsid $(ceph fsid)

# On all nodes
docker stop $(docker ps -q)
rm -r /etc/ceph /var/lib/ceph /log/ceph /etc/systemd/system/ceph* /usr/lib/systemd/system/ceph* /etc/systemd/system/multi-user.target.wants/ceph* /run/ceph /run/cephadm
systemctl reset-failed
systemctl daemon-reload
lvremove -y vg-ceph/lv-ceph
vgremove -y vg-ceph
losetup -d $(losetup -j /var/lib/chelsea-ceph/backingFile.img | cut -d: -f1)
rm /var/lib/chelsea-ceph/backingFile.img

# Be sure to check bootstrap node for floating systemd targets.
systemctl | grep ceph
```