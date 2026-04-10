# How to build a Ceph cluster inside a firecracker VM

We do this for Chelsea dev and testing because Ceph causes kernel
deadlocks if the storage components and the remote block device client
are run on the same kernel.

This process should only need to be repeated infrequently. It is not fully
automated because the only reason it would be repeated is to move to new
versions, which are likely to have minor changes that would break the
automation.

Prerequisites:
- You'll need access to KVM... so do this from Linux
- Firecracker and Ceph assume Ubuntu
- The VM is given 4 cores and 6GB of RAM, and probably needs all of that, so
  your host will need to have that available.
- We also eat 20-50 GBs of disk space.

## Build a kernel that works for Firecracker & Ceph

1. Start dev node
2. Clone firecracker `git clone https://github.com/firecracker-microvm/firecracker.git`
3. Update packages `sudo apt update`
4. Install docker `sudo apt install docker.io`
6. Modify our user to add to the docker group: `sudo usermod -a -G docker ubuntu`
7. Login out and back in to get those permissions
5. Move into firecracker's directory: `cd src/firecracker`
8. Kick off the build process `./tools/devtool build_ci_artifacts kernels 6.1`
9. Test the resulting kernel in firecracker to make sure that we can build a
   working kernel.
10. Combine the firecracker configs `cat /home/ubuntu/src/firecracker/resources/guest_configs/microvm-kernel-ci-x86_64-6.1.config /home/ubuntu/src/firecracker/resources/guest_configs/ci.config /home/ubuntu/src/firecracker/resources/guest_configs/pcie.config /home/ubuntu/src/firecracker/resources/guest_configs/virtio-pmem.config /home/ubuntu/src/firecracker/resources/guest_configs/virtio-mem.config > /tmp/config`
11. Move it into place `sudo mv /tmp/config resources/linux/.config`
12. Change directory and run menuconfig `cd resources/linux && sudo make menuconfig`

    Enable Device Mapper, a dependency of LVM, which is a Ceph dependency.
    Device Drivers
    -> Multiple Devices driver support
      -> Device mapper support
        -> Then just select all

    Ceph also needs TCP BBR
    -> Networking support (NET [=y])
      -> Networking options
        -> TCP/IP networking (INET [=y])
          -> TCP: advanced congestion control (TCP_CONG_ADVANCED [=y])
            -> BBR (DEFAULT_BBR [=n])

    Once that is selected also select
    Ceph also needs TCP BBR
    -> Networking support (NET [=y])
      -> Networking options
        -> TCP/IP networking (INET [=y])
          -> TCP: advanced congestion control (TCP_CONG_ADVANCED [=y])
            -> Default TCP congestion control
            and set it to BBR

    And enable this one too
    Networking support
    -> Networking options
      -> QoS and/or fair queueing
        -> Fair Queue

13. Fircracker's compilation chain runs inside docker. We want to have
    more control / directly compile the kernel with our config, so we will
    compile this round on the host.
13. Install Linux deps: `sudo apt install flex bison libelf-dev`
14. Check our config / fill in defaults: `sudo make olddefconfig`
15. Compile the kernel: `sudo make -j $(nproc) vmlinux`


Things that can go wrong:
- If your kernel config is invalid, the kernel compilation chain will simply
  elide the invalid parts.
- Run `make clean` between kernel compilation runs


## Setup Ceph cluster

Install Ceph common: `sudo apt install ceph-common` so we can test our config
from the host.

Get firecracker: `./get-firecracker.sh`

Get an Ubuntu RootFS from Firecracker and setup our disks for Ceph
```
./get-rootfs-and-setup-disks.sh
```

Start firecracker: `./start-firecracker.sh`

Then in another terminal start our VM and login: `start-firecracker.sh`

Now we can setup Ceph. All these commands are run inside the VM.

Ceph's one true way of setting up or administering a cluster is the `cephadm`
tool See https://docs.ceph.com/en/quincy/cephadm/index.html

```
apt-get update

apt-get install -y docker.io ceph-common ceph-base cephadm python3-jinja2

update-alternatives --set iptables /usr/sbin/iptables-legacy
update-alternatives --set ip6tables /usr/sbin/ip6tables-legacy

service docker restart

cephadm bootstrap \
   --cluster-network 172.16.0.0/24 \
   --mon-ip 172.16.0.2 \
   --skip-dashboard \
   --skip-firewalld \
   --skip-monitoring-stack \
   --skip-mon-network \
   --allow-fqdn-hostname \
   --single-host-defaults
```

Then add the three disks we created
```
ceph orch daemon add osd ubuntu-fc-uvm:/dev/vdb
ceph orch daemon add osd ubuntu-fc-uvm:/dev/vdc
ceph orch daemon add osd ubuntu-fc-uvm:/dev/vdd
```


Then create a pool, and enable it:
```
ceph osd pool create rbd
ceph osd pool application enable rbd rbd
```

We then need keys so that external users (i.e. Chelsea) can access the
cluster
```
ceph auth get-or-create client.chelsea mon 'profile rbd' \
  osd 'profile rbd pool=rbd' mgr 'profile rbd pool=rbd' \
  -o /etc/ceph/ceph.client.chelsea.keyring
```

Grab the key created in the last step from
`/etc/ceph/ceph.client.chelsea.keyring` it will look something like:

```
[client.chelsea]
	key = AQBt0t5o1o+ABxAA0qEJ8XZ4uQfRFZE+hiIUiA==

```
(it needs the trailing newline)

The machine that wants to connect to Ceph will also need a ceph.conf file,
something like:

```
[global]
	fsid = be4d1849-9fc1-11f0-a026-0600ac100002
	mon_host = [v2:172.16.0.2:3300/0,v1:172.16.0.2:6789/0]

```

It also required the trailing newline. The fsid will be in the VM's ceph.conf
file at `/etc/ceph/ceph.conf` The IPs must match your host config.

The default location for these files is `/etc/ceph/`, once you have them there,
you should be able to run commands like
```
ceph --keyring /etc/ceph/ceph.client.chelsea.keyring -n client.chelsea -s
```
from the host.

You can test/confirm if Ceph is up, and visible from the host with:
```
nc 172.16.0.2 3300

# and
nc 172.16.0.2 6789
```

Congratulations, you have a Ceph cluster! Tar it up, store it in S3, and happy
deving.
