This document was originally written for [PR #835](https://github.com/hdresearch/chelsea/pull/835)  

## Requirements
This PR introduces 3 new requirements:
- R-60406-44368-24068-62865-00370-65313-62131-00148: On startup, and periodically thereafter, Mulberry will check for issues that may exist on the host.
- R-20053-45833-44949-14649-49984-15269-07156-62382: When a ghost VM is found by Mulberry, an attempt to reboot the VM will be made first.
- R-18140-37390-47377-13296-21974-03264-56819-20805: In the event a ghost VM's reboot fails, then the VM will be fully deleted, with stop code GhostRebootFailed.

Below, I will test these requirements and ensure that all cases of ghost VMs described above are handled.

## Testing
1) Start the SNE. Set `mulberry_ghost_vm_check_interval_seconds` in `/etc/vers/520-chelsea.ini` to a smaller value; I used 10. Ensure your log level is at least `info`.
2) Create a new VM via `./public-api.sh new`.
3) Connect to the first via `./commands.sh connect`, `touch testfile` and issue the `reboot` command. Be ready to issue `./public-api.sh list`. We'll only do this once, but this will ensure the reboot codepath correctly interacts with the readiness service.
4) On the next tick of Mulberry's ghost VM check, you'll see the following output: `2026-01-31T00:23:50.074849Z  INFO mulberry::mulberry: Found ghost VM; attempting to restart vm_id=27ef3efe-2915-4cfa-acf2-45e4de8819c6`. The instant you see this, press enter to issue the `./public-api.sh list`. If done correctly, you'll find that there is a brief moment where the VM shows as booting, and then it returns to the running state:
```
root@ip-172-31-3-249:/home/ubuntu/src/chelsea# ./public-api.sh list
[
  {
    "vm_id": "27ef3efe-2915-4cfa-acf2-45e4de8819c6",
    "owner_id": "ef90fd52-66b5-47e7-b7dc-e73c4381028f",
    "created_at": "2026-01-31T00:12:52.154197Z",
    "state": "booting"
  }
]
root@ip-172-31-3-249:/home/ubuntu/src/chelsea# ./public-api.sh list
[
  {
    "vm_id": "27ef3efe-2915-4cfa-acf2-45e4de8819c6",
    "owner_id": "ef90fd52-66b5-47e7-b7dc-e73c4381028f",
    "created_at": "2026-01-31T00:12:52.154197Z",
    "state": "running"
  }
]
```
5) Ensure you can once again SSH into the VM via `./commands.sh` and that your `testfile` file exists. You can `reboot` as many times as you'd like at this point, and the VM will recover.
6) Kill the process via `kill`. You can glance at `ps`, or you can use the sqlite database to get the PID. Notice that the VM recovers in the same way, and `testfile` still exists.
```bash
sqlite3 /var/lib/chelsea/db/chelsea.db "select vm_process_pid from vm;"
```
7) Other things to verify:
  - The RBD image has not changed; new snaps weren't created etc. `rbd showmapped`.
  - Wireguard info has not changed; `ip netns exec vm_192_168_100_0 wg show` (The netns may vary. You can get this from `sqlite3 /var/lib/chelsea/db/chelsea.db "select netns_name from vm_network where host_addr = (select vm_network_host_addr from vm where id = '27ef3efe-2915-4cfa-acf2-45e4de8819c6')"`, where `27ef...` is my VM ID).
8) Let's simulate an invalid record in Sqlite. `./api.sh new`, then delete the Ceph volume record:
```
sqlite3 /var/lib/chelsea/db/chelsea.db "delete from ceph_vm_volume where id = (select vm_volume_id from vm where id = 'be52659d-aa60-4e6b-a2cd-e1202a1b65f7')"
```
9) SSH into the VM and `reboot`. On the next cycle, you will see the following output:
```
2026-01-31T00:50:14.301873Z  INFO mulberry::mulberry: Found ghost VM; attempting to restart vm_id=be52659d-aa60-4e6b-a2cd-e1202a1b65f7
2026-01-31T00:50:14.354325Z  WARN chelsea_lib::vm_manager::manager: Failed to reboot VM; deleting now error=unknown vm manager error: Failed to find Ceph VmVolume with id '819f681c-11f3-43a2-b302-0e78111933b9'. vm_id=be52659d-aa60-4e6b-a2cd-e1202a1b65f7
2026-01-31T00:50:14.417474Z  INFO chelsea_lib::network_manager::manager: running on_vm_killed
2026-01-31T00:50:14.525407Z  WARN mulberry::mulberry: Error while deleting ghost VM (this may be expected) error=unknown vm manager error: One or more errors while killing VM be52659d-aa60-4e6b-a2cd-e1202a1b65f7: firecracker error: jailer error: No such process (os error 3); Failed to find Ceph VmVolume with id '819f681c-11f3-43a2-b302-0e78111933b9'.
```
10) Verify that the VM has been fully deleted. This includes checking the PG database and ensuring the VM usage segment was properly closed:
```
vers=# select * from chelsea.vm_usage_segments where vm_id='be52659d-aa60-4e6b-a2cd-e1202a1b65f7';
                vm_id                 | start_timestamp | start_created_at | stop_timestamp | stop_created_at | vcpu_count | ram_mib | disk_gib | start_code | stop_code
--------------------------------------+-----------------+------------------+----------------+-----------------+------------+---------+----------+------------+-----------
 be52659d-aa60-4e6b-a2cd-e1202a1b65f7 |      1769820422 |       1769820422 |     1769820614 |      1769820614 |          1 |     512 |          |            |
(1 row)
```
11) Create VMs, then stop chelsea and repeat any number of the above tests (`reboot` on VM, killing process, creating invalid record.) Note that shortly after restarting chelsea, these VMs will be detected as ghost VMs and rebooted accordingly.

## Known issue
When a ghost VM is rebooted via this mechanism, the following output is always produced:
```
2026-01-30T23:42:01.545318Z DEBUG chelsea_lib::vm_manager::manager: Error while killing VM process for reboot; attempting to continue error=firecracker error: jailer error: No such process (os error 3)
```
At the moment, many error types are still anyhow, and this particular error message is simply because it is not possible at present to disambiguate between different error types at the callsite. It has been dropped to debug level logging because the reboot operation is meant to attempt a best-effort shutdown before starting a new VM - it is expected that ghost VMs may be the result of slightly invalid state. Future improvement would be to catch the specific error case(s) expected, rather than dismissing all under this "error, but continue anyway" message.  

At present, start and stop codes are not inserted, but when they are, we will want to ensure the correct code is inserted when deleting a VM that failed to reboot.