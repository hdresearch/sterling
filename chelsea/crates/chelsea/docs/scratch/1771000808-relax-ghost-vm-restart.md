This document was originally written for [PR #903](https://github.com/hdresearch/chelsea/pull/903).

# Overview
Previously, there was a race condition in which mulberry would catch VM in the process of being initialized that would falsely register as a ghost VM. I want to make it clear that the root cause of this is not currently known; ghost VMs should only be detected when a DB record exists and a process has not been spawned. In the code as written, the DB record is created as the *last* step in VM creation, much after process spawning. So the only "intuitive" explanation I can think of at the moment is that there is some delay between when the spawn command is issued, and when the PID becomes visible to the subroutine that scans for running PIDs.  

The solution implemented by this PR is to simply wait until a VM has failed the ghost VM check 3 consecutive times before being marked for restart. This ensures that the VM's state has time to become consistent when it's caught in the middle of transition states. This is *not* a permanent solution to the issue, as time-based solutions are not entirely robust. Note that the correct solution likely lies in #902, currently blocked by #805. Once Mulberry is aware of the state of VMs, it should be able to make better decisions about which VMs to consider or ignore. But having this behavior by default is hardly a bad idea; it's a good fallback to ensure we don't take the destructive action of restarting a VM without first being sure that we should.    

For the following tests, I set `mulberry_ghost_vm_check_interval_seconds=2`; the `sleep` durations are set accordingly. Running the commands manually is also sufficient in the event the timings do not work out with such short intervals. Please note that a value of 2 may not be appropriate in production or testing settings; in the course of testing, I saw normal execution paths where the consecutive failure count reached 2 before the VM was correctly identified as a false ghost and had its count reset, implying this threshold may be cutting it close.

## Requirements
This PR adds one requirement and modifies one other requirement.

### R-52513-20875-63854-08541-13802-11074-60737-54803
__When a ghost VM is detected by Mulberry, it will not be marked for restart until it has been detected as a ghost VM in `mulberry_ghost_vm_fail_count_restart_threshold` *consecutive* task iterations.__  
This requirement is tested in the testing section below.

### R-53651-01327-20828-15429-08765-14649-25588-30488
Formerly R-20053-45833-44949-14649-49984-15269-07156-62382  
__When a ghost VM is marked for restart by Mulberry, an attempt to reboot the VM will be made first.__  
This requirement has not changed in a meaningful way; the language has simply changed to reflect the fact that ghost VMs are no longer restarted immediately upon being detected; they must fail 3 times in a row before being marked for restart.

## Testing

### Ensure VMs that exceed 3 consecutive failures are rebooted
```
DB=/var/lib/chelsea/db/chelsea.db
vm_id=$(./api.sh new | jq -r .vm_id)
echo "Created VM $vm_id; waiting 5 seconds to ensure boot"
sleep 5
pid=$(sqlite3 $DB "select vm_process_pid from vm where id = '$vm_id'")
echo "Killing PID $pid"
kill $pid
echo "Killed PID $pid; monitor chelsea output at DEBUG level to ensure VM counter is incremented, then restarted at 3."
```
Chelsea output (Irrelevant lines omitted):
```
2026-02-13T16:30:09.832075Z DEBUG chelsea::server_core: Received notify request from VM 'd8402b92-54af-4860-9984-1141519bc617' request=Ready("true")
2026-02-13T16:30:15.460737Z DEBUG mulberry::mulberry: Found ghost VM; incrementing failure count vm_id=d8402b92-54af-4860-9984-1141519bc617 failure_count=1
2026-02-13T16:30:17.493014Z DEBUG mulberry::mulberry: Found ghost VM; incrementing failure count vm_id=d8402b92-54af-4860-9984-1141519bc617 failure_count=2
2026-02-13T16:30:19.518495Z DEBUG mulberry::mulberry: Found ghost VM; incrementing failure count vm_id=d8402b92-54af-4860-9984-1141519bc617 failure_count=3
2026-02-13T16:30:19.518508Z  INFO mulberry::mulberry: Ghost VM has exceeded consecutive failure check threshold; restarting vm_id=d8402b92-54af-4860-9984-1141519bc617 failure_count=3
2026-02-13T16:30:19.634496Z DEBUG chelsea_lib::vm_manager::manager: Error while killing VM process for reboot; attempting to continue error=firecracker error: jailer error: No such process (os error 3)
2026-02-13T16:30:19.854009Z  INFO mulberry::mulberry: Successfully restarted ghost VM; resetting failure count vm_id=d8402b92-54af-4860-9984-1141519bc617 failure_count=3
2026-02-13T16:30:20.661698Z DEBUG chelsea::server_core: Received notify request from VM 'd8402b92-54af-4860-9984-1141519bc617' request=Ready("true")
```

## Ensure VMs that are "falsely" detected as ghosts/that somehow recover have their consecutive fail count reset
```sh
DB=/var/lib/chelsea/db/chelsea.db
vm_id=$(./api.sh new | jq -r .vm_id)
echo "Created VM $vm_id; waiting 5 seconds to ensure boot"
sleep 5
original_pid=$(sqlite3 $DB "select vm_process_pid from vm where id = '$vm_id'")
echo "Changing recorded PID from $pid to 12345"
sqlite3 $DB "update vm set vm_process_pid = 12345 where id = '$vm_id'"
# Wait for count to increment to either 1 or 2
sleep 3
echo "Restoring recorded PID back to $original_pid"
sqlite3 $DB "update vm set vm_process_pid = $original_pid where id = '$vm_id'"
echo "Monitor chelsea output at DEBUG to ensure fail count is reset."
sleep 5
echo "Repeating previous flow"
echo "Changing recorded PID from $pid to 12345"
sqlite3 $DB "update vm set vm_process_pid = 12345 where id = '$vm_id'"
# Wait for count to increment to either 1 or 2
sleep 3
echo "Restoring recorded PID back to $original_pid"
sqlite3 $DB "update vm set vm_process_pid = $original_pid where id = '$vm_id'"
echo "Monitor chelsea output at DEBUG to ensure fail count is reset."
```
Chelsea output:
```
2026-02-13T16:27:57.346152Z DEBUG chelsea::server_core: Received notify request from VM '3277a918-eeb7-4584-a6f4-4ae26a2b2898' request=Ready("true")
2026-02-13T16:28:01.813486Z DEBUG mulberry::mulberry: Found ghost VM; incrementing failure count vm_id=3277a918-eeb7-4584-a6f4-4ae26a2b2898 failure_count=1
2026-02-13T16:28:03.838423Z DEBUG mulberry::mulberry: Found ghost VM; incrementing failure count vm_id=3277a918-eeb7-4584-a6f4-4ae26a2b2898 failure_count=2
2026-02-13T16:28:05.862590Z DEBUG mulberry::mulberry: Previously-detected ghost VM is no longer a ghost VM; resetting failure count vm_id=3277a918-eeb7-4584-a6f4-4ae26a2b2898 failure_count=2
2026-02-13T16:28:09.911196Z DEBUG mulberry::mulberry: Found ghost VM; incrementing failure count vm_id=3277a918-eeb7-4584-a6f4-4ae26a2b2898 failure_count=1
2026-02-13T16:28:11.935938Z DEBUG mulberry::mulberry: Found ghost VM; incrementing failure count vm_id=3277a918-eeb7-4584-a6f4-4ae26a2b2898 failure_count=2
2026-02-13T16:28:13.960242Z DEBUG mulberry::mulberry: Previously-detected ghost VM is no longer a ghost VM; resetting failure count vm_id=3277a918-eeb7-4584-a6f4-4ae26a2b2898 failure_count=2
```

## Clean test output (in case CI actions are failing to unrelated issues)
```
LEAKED: 0
KNOWN BUGS: images-1.1 1 images-1.2 1 images-1.3 1 images-2.0 1 images-2.1 1
PASSED: 23
SKIPPED: 5
SKIPPED: images-1.1 knownBug images-1.2 knownBug images-1.3 knownBug images-2.0 knownBug images-2.1 knownBug
SKIPPED (BUG): 5
TOTAL: 28
SKIP PERCENTAGE: 17.8571%
PASS PERCENTAGE: 100%
OVERALL RESULT: script /home/ubuntu/share/chelsea/tests/all.eagle, suite Chelsea Test Suite for Eagle
OVERALL RESULT: SUCCESS
```