# Cloud Hypervisor Block IO

## Problem statement

- Cloud Hypervisor is unable to resize disks when they are backed by Ceph (or
  any network block device).

- Disk resizing is a part of the required feature set to replace Firecracker in
  production.

- Vincent is the only team member with substantial "hands on" time with
  Cloud Hypervisor and Ceph, and reports a reduction in stability and Ceph
  "working harder".

- Failing to unmap an RBD device before Ceph shutdown (which happens
  semi-regularly in Dev and CI environments) necessitates a machine reboot
  (because the in-kernel rbd client refuses to give up trying to connect to the
  cluster).

- We want [interactions with Ceph to be faster](../crates/cephalopod)

- We want resizing to be easy, or better, transparent to users, and to use thin
  provisioning.


## Solution

What would it look like if we rebuilt network block storage / our Ceph
integration from first principles?

It looks like we would use **vhost-user-blk**

### vhost-user-blk

Developed in 2017

Uses zero copy MMIO to move blocks from the host to the VM and uses a Unix
domain socket to send "interrupts" to the VM letting it know the IO is in
place.

The actual moving of data is handled in userspace in the VMM, and can be any
arbitrary block store.

Cloud Hypervisor [always has support built
in](https://github.com/cloud-hypervisor/cloud-hypervisor/blob/main/docs/device_model.md#vhost-user-blk).

Cloud Hypervisor's [team recommends
it](https://github.com/cloud-hypervisor/cloud-hypervisor/issues/5896) when you
need to add an arbitrary block storage to VMs.

To implement it we would need to supply a "back end" which Cloud Hypervisor's
"front end" could connect to.

Cloud Hypervisor [has an example back
end](https://github.com/cloud-hypervisor/cloud-hypervisor/tree/main/vhost_user_block)

There is also an [example/framework in the Rust-vmm
crates](https://github.com/rust-vmm/vhost/tree/main)

It looks like we would implement the backend directly on top of librados, likely
using one of the existing [Rust wrappers](https://crates.io/search?q=rados)

By implementing the this we should be able to address each part of the problem
statement:

- We can correctly implement resize

- We can provide a sparse volume. I recommend standardizing on a large, say 1TB
  volume, and possibly not providing a resize operation.

- This is the lowest level, direct access to Ceph, which should remove our "Ceph
  works harder" or has "increased unreliability".

- We would no longer be using the in-kernel RBD driver, so we would not have
  instances of it requiring machine reboot.

- We should see speedups similar to Cephalopod, with zero copy, and more direct
  IO, we should see further performance increases.


vhost-user-blk is in [developer
preview](https://github.com/firecracker-microvm/firecracker/blob/main/docs/api_requests/block-vhost-user.md)
in Firecracker

Background links:
- [Talk covering vhost-user-blk](https://archive.fosdem.org/2023/schedule/event/sds_vhost_user_blk/)
- [Slides from the same talk](https://archive.fosdem.org/2023/schedule/event/sds_vhost_user_blk/attachments/slides/5444/export/events/attachments/sds_vhost_user_blk/slides/5444/stefanha_fosdem_2023.pdf)
- [Virtio Spec](https://docs.oasis-open.org/virtio/virtio/v1.2/virtio-v1.2.html)
- [libblkio](https://libblkio.gitlab.io/libblkio)
