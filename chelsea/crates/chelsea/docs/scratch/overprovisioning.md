This document was originally written for [PR #845](https://github.com/hdresearch/chelsea/pull/845)  

## Testing
The requirements below may make reference to these tests. In order to capture the events sent to the Mulberry event server, I used `mitmproxy` listening on the orchestrator's address (`[fd00:fe11:deed::ffff]:8090`).

### Test 1
This test required configuring the host to accept a large number of VMs; I set the VM subnet to `192.168.96.0/19` (8192 addresses, 4096 pairs) and the SSH port range to `[28000, 32096)` (4096 ports). This causes the NetworkManager initialization to take a while, but it will eventually finish. (Progress can be monitored very crudely with `watch -n10 "ip netns | wc"`). Verify that the max VM count is indeed 4096 with `./api.sh telemetry`.
```tcl
proc newVm {} {
    set id [exec ./api.sh new --disk 512 --mem 512 | jq -r .vm_id]
}

puts -nonewline "Creating VMs... (0 / 3000)"
flush stdout
for {set i 1} {$i <= 3000} {incr i} {
    if {[catch {set vmId [newVm]} error]} {
        puts "Error: $error"
    }
    puts -nonewline "\033\[1GCreating VMs... ($i / 3000) (last VM: $vmId)"
    flush stdout
}
```
The results of which were that 2745 VMs were created before the SNE Ceph Firecracker started choking. I suspect this is very much due to trying to run 3 OSDs on the same 6 GiB RAM VM, but I also may have slightly misconfigured things while sizing up the SNE cluster. BlueStore doesn't appear to like being resized in-flight. But anyway, other interesting numbers:
```
root@ip-172-31-3-249:/home/ubuntu/share/chelsea# top
top - 22:04:06 up  4:06,  1 user,  load average: 1.79, 2.07, 3.40
Tasks: 14811 total,   2 running, 14809 sleeping,   0 stopped,   0 zombie
%Cpu(s):  1.1 us,  1.7 sy,  0.0 ni, 97.2 id,  0.0 wa,  0.0 hi,  0.0 si,  0.0 st
GiB Mem :    188.5 total,      3.5 free,    139.1 used,     48.4 buff/cache
GiB Swap:    500.0 total,    280.3 free,    219.7 used.     49.5 avail Mem
```
Ceph utilization: 90 GiB (A suspiciously round number)

### Test 2 
This test simply spins up some VMs with CPU hogs to test the CPU half of the new Mulberry checks.
1) Spawn a busy VM.
```sh
vm_id=$(./api.sh new --vcpu 4 --mem 512 --disk 1024 | jq -r .vm_id)
./commands.sh connect $vm_id
```
2) Install and run stress (on the VM.)
```sh
apt update && apt install -y stress && stress -c 4
```
3) Spawn VMs to reach the CPU utilization thresholds.
```sh
# Commit the VM; this will be used to quickly clone it
echo "Making commit..."
commit_id=$(./api.sh commit $vm_id | jq -r .commit_id)
echo "Made commit $commit_id"

echo -n "Spawning VMs quickly (4 vCPU / 120)"
for (( n=2; n<21; n++ )); do
    ./api.sh run-commit $commit_id
    echo -ne "\033[1K\033[1GSpawning VMs quickly ($((n*4)) vCPU / 120)"
done

for (( n=21; n<=30; n++ )); do
    ./api.sh run-commit $commit_id
    echo -ne "\033[1K\033[1GSpawning VMs slowly ($((n*4)) vCPU / 120)"
    sleep 5
done
```

## Requirements
This PR adds 9 new requirements.

### R-43361-48245-44535-26873-01322-39725-59694-14783
Any new VM request which would exceed the hard, per-VM maxima - `chelsea_vm_max_vcpu_count`, `chelsea_vm_max_memory_mib`, and `chelsea_vm_max_volume_mib` - will be rejected, regardless of whether the host could theoretically run it or not.
```sh
./api.sh new --vcpu 99999
./api.sh new --mem  99999
./api.sh new --disk 99999
```
```json
{"error":"allocation error: Requested VM violates hard maximum for vCPU count. Requested: 99999; Maximum: 4"}
{"error":"allocation error: Requested VM violates hard maximum for memory MiB. Requested: 99999; Maximum: 8196"}
{"error":"allocation error: Requested VM violates hard maximum for volume MiB. Requested: 99999; Maximum: 16392"}
```

### R-56920-28366-17477-13804-43066-38278-17541-56203
On startup, unless overprovisioning is enabled (see Overprovisioning), if either of `chelsea_vm_total_vcpu_count` or `chelsea_vm_total_memory_mib` exceeds the host's vCPU count or memory count respectively, then an error will be returned.  

Setting `chelsea_allow_vcpu_overprovisioning=false` and `chelsea_vm_total_vcpu_count=99999`
```
[Debug] vers_config: proxy_wg_public_key from 560-proxy
[Warn] vers_config: One or more unused config vars: pool_auth_token
Timing layer disabled
Error: Cannot set VM vCPU count allocation to 99999; host only has 96 vCPUs. To ignore this error, set chelsea_allow_vcpu_overprovisioning=true.
```
Setting `chelsea_allow_memory_overprovisioning=false` and `chelsea_vm_total_memory_mib=999999`
```
[Debug] vers_config: proxy_wg_public_key from 560-proxy
[Warn] vers_config: One or more unused config vars: pool_auth_token
Timing layer disabled
Error: Cannot set VM memory allocation to 999999 MiB; host only has 193025 MiB. set chelsea_allow_memory_overprovisioning=true
```

### R-27788-31827-10003-53341-01793-53542-02039-28664
Similarly, on startup, unless overprovisioning is enabled, a warning will be printed if either `chelsea_vm_total_vcpu_count` or `chelsea_vm_total_memory_mib` would leave the host with less than 4 vCPUs or 8192 MiB of memory for non-VM processes.  

Disabling overprovisioning and setting `chelsea_vm_total_vcpu_count=95` and `chelsea_vm_total_memory_mib=192000`:
```
[Debug] vers_config: proxy_wg_public_key from 560-proxy
[Warn] vers_config: One or more unused config vars: pool_auth_token
Timing layer disabled
2026-02-04T00:59:18.313349Z  WARN chelsea::startup_checks::provisioning: Host has 96 vCPUs; setting chelsea_vm_total_vcpu_count to 95 would leave host with fewer than 4 for non-VM processes.
2026-02-04T00:59:18.313391Z  WARN chelsea::startup_checks::provisioning: Host has 193025 MiB total memory; setting chelsea_vm_total_memory_mib to 192000 would leave host with fewer than 8192 MiB for non-VM processes.
2026-02-04T00:59:18.441823Z  INFO chelsea: Setting up VM networks (this will take a while, especially if you've allocated a larger VM subnet.
```

### R-50244-54977-40377-27017-14554-40165-42686-28114
On receiving a new VM request, Chelsea will first ensure that the new VM would not cause the total amount of "reserved" resources to exceed `chelsea_vm_total_vcpu_count` or `chelsea_vm_total_memory_mib`.  

For `chelsea_vm_total_vcpu_count=4`:
```sh
for (( i=0; i<3; i++ )); do
  ./api.sh new --vcpu 2
done
```
```json
{"vm_id":"c2b89b61-ddc1-4ee9-b692-cf31817ef1af"}
{"vm_id":"76c0881c-fb74-4d82-a487-eaaa1b948de6"}
{"error":"allocation error: Host does not have adequate vCPU count for requested VM. Requested: 2; Available: 0"}
```
For `chelsea_vm_total_memory_mib=8192`:
```sh
for (( i=0; i<5; i++ )); do
  ./api.sh new --mem 2048
done
```
```json
{"vm_id":"067e1130-bd1a-4ef2-80bd-37ff9b04c765"}
{"vm_id":"59bf7b4f-f54b-459e-a9f5-2609e50b4fba"}
{"vm_id":"dff09c53-4dde-46b3-a07a-f221b6640776"}
{"vm_id":"e59e5e15-1056-4a0e-9799-9f9d4859f6b6"}
{"error":"allocation error: Host does not have adequate memory MiB for requested VM. Requested: 2048; Available: 0"}
```

### R-35206-09471-13694-19842-02446-08772-52416-07232
When `chelsea_allow_vcpu_overprovisioning=true` is set, chelsea will not validate the value of `chelsea_vm_total_vcpu_count` against the host vCPU count on start, and similarly when `chelsea_allow_memory_overprovisioning=true` is set, chelsea will not validate the value of `chelsea_vm_total_memory_mib` against the host memory count on start.  

Simply set `chelsea_vm_total_vcpu_count` and `chelsea_vm_total_memory_mib` to 999999 with overprovisioning enabled; no error is thrown.

### R-39486-29021-38801-30936-12753-21030-01512-59973
When `chelsea_allow_vcpu_overprovisioning=true` and `chelsea_vm_total_vcpu_count=0` are both set, then the value of `chelsea_vm_total_vcpu_count` will be instead treated as infinity (or a very large number) for the purposes of the on-VM-creation reserved resource check and telemetry; the same is true of `chelsea_allow_memory_overprovisioning=true` and `chelsea_vm_total_memory_mib=0`.  

When setting these variables and running Test 1, this is the output from the telemetry endpoint:
```json
{
  "ram": {
    "real_mib_total": 193025,
    "real_mib_available": 49776,
    "vm_mib_total": 4294967295,
    "vm_mib_available": 4293561855
  },
  "cpu": {
    "real_total": 100.0,
    "real_available": 97.65073776245117,
    "vcpu_count_total": 96,
    "vcpu_count_vm_total": 4294967295,
    "vcpu_count_vm_available": 4294964550
  },
  "fs": {
    "mib_total": 1982707,
    "mib_available": 196805
  },
  "chelsea": {
    "vm_count_max": 4096,
    "vm_count_current": 2745
  }
}
```

### R-63557-36457-06620-04382-36117-28914-50640-62530
When Mulberry detects that CPU usage has exceeded `mulberry_cpu_usage_warning_threshold`, or that memory usage (*including* swap usage) has exceeded `mulberry_memory_mib_usage_warning_threshold`, a notification will be sent to `mulberry_event_server_host`.  

For CPU: Test 2 sent a few such messages to the event server every 5s:
```json
{
    "CpuSoftThresholdExceeded": {
        "cpu_usage": 88.48982,
        "threshold": 80.0
    }
}
```
For memory: Test 1 sent many such messages to the event server every 5s:
```json
{
    "MemorySoftThresholdExceeded": {
        "memory_usage": 83.89587,
        "threshold": 80.0
    }
}
```

### R-22618-22613-29506-21742-18007-08723-48641-42672
When Mulberry detects that CPU usage has exceeded `mulberry_cpu_usage_hard_threshold`, or that memory usage (*including* swap usage) has exceeded `mulberry_memory_mib_usage_hard_threshold`, it will begin to Sleep VMs until usage returns to a value lower than the violated threshold.  

For CPU: Test 2 sent many such messages to the event server every 5s:
```json
{
    "CpuHardThresholdExceeded": {
        "cpu_usage": 98.59984,
        "threshold": 95.0
    }
}
```
For memory: Test #1 sent many such messages to the event server every 5s:
```json
{
    "MemoryHardThresholdExceeded": {
        "memory_usage": 160.95532,
        "threshold": 95.0
    }
}
```
Note that the second half of the requirement is currently unmet; #795 is still pending. Tracked by #852

### R-10309-10481-50739-25955-26934-22047-26170-17198
For each VM Mulberry Sleeps for hard threshold violations, it will send a notification to `mulberry_event_server_host`.  

Unmet for now; #795 is still pending. Tracked by #852