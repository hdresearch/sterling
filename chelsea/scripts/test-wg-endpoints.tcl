#! /usr/bin/env tclsh

# This is not intended to be a canonical script. This script was written for the sole purpose of testing PR #450
# It is uploaded as-is in case it is helpful to anyone else.

set hostname localhost:8111

puts "Testing wireguard config passing to chelsea at $hostname"
puts "\033\[33mPLEASE NOTE: As of October 17, 2025, the wait-ready mechanism was merged into the branch this is meant to test.\033\[0m"
puts "This means that the branch and commit operations will fail unless `crates/chelsea_lib/ready_service/service.rs::is_vm_booting()` is set to return false."

proc readFile {fileName} {
    set fd [open $fileName]
    set contents [read $fd]
    close $fd
    return $contents
}

proc randomIpv4 {} {
    set oct1 [expr {int(rand() * 256)}]
    set oct2 [expr {int(rand() * 256)}]
    set oct3 [expr {int(rand() * 256)}]
    set oct4 [expr {int(rand() * 256)}]
    return "$oct1.$oct2.$oct3.$oct4"
}

proc randomIpv6 {} {
    set segments {}
    for {set i 0} {$i < 8} {incr i} {
        set segment [format "%04x" [expr {int(rand() * 0x10000)}]]
        lappend segments $segment
    }
    return [join $segments ":"]
}

proc randomWireguardConfig {} {
    # Generate temporary keypairs
    set privateKey1 [exec wg genkey]
    set publicKey1 [exec echo $privateKey1 | wg pubkey]
    set privateKey2 [exec wg genkey]
    set publicKey2 [exec echo $privateKey2 | wg pubkey]

    # Generate random IP addresses
    set randIpv6_1 [randomIpv6]
    set randIpv6_2 [randomIpv6]
    set randIpv4 [randomIpv4]

    return [format {{
    "private_key": "%s",
    "public_key": "%s",
    "ipv6_address": "%s",
    "proxy_public_key": "%s",
    "proxy_ipv6_address": "%s",
    "proxy_public_ip": "%s"
}} $privateKey1 $publicKey1 $randIpv6_1 $publicKey2 $randIpv6_2 $randIpv4]
}

# Test creating a new root VM
set endpoint $hostname/api/vm/new_root
set rootWireguard [randomWireguardConfig]
set data [format {{
    "vm_config": {},
    "wireguard": %s
}} $rootWireguard]
puts "Sending the following payload to POST $endpoint:\n$data"
set rootVm [exec curl -sS -d $data -H "Content-Type: application/json" $endpoint]
set rootVmId [exec echo $rootVm | jq -r .id]
puts "Root VM created with ID $rootVmId"

# Test branching a new root VM
set endpoint $hostname/api/vm/$rootVmId/branch
set childWireguard [randomWireguardConfig]
set data [format {{
    "wireguard": %s
}} $childWireguard]
puts "Branching VM $rootVm on POST $endpoint:\n$data"
set childVm [exec curl -sS -d $data -H "Content-Type: application/json" $endpoint]
set childVmId [exec echo $childVm | jq -r .vm_id]
puts "Child VM branched with ID $childVmId"

# Test creating a VM from commit
set endpoint $hostname/api/vm/$childVmId/commit
puts "Creating commit at $endpoint"
set commitResponse [exec curl -sS -X POST $endpoint]
set commitId [exec echo $commitResponse | jq -r .commit_id]
puts "Commit created with ID $commitId"

set endpoint $hostname/api/vm/from_commit
set fromCommitWireguard [randomWireguardConfig]
set data [format {{
    "commit_id": "%s",
    "wireguard": %s
}} $commitId $fromCommitWireguard]
puts "Running a VM from commit $commitId on $endpoint with data:\n$data"
set fromCommitVm [exec curl -sS -d $data -H "Content-Type: application/json" $endpoint]
set fromCommitVmId [exec echo $fromCommitVm | jq -r .vm_id]
puts "VM run from commit with ID $fromCommitVmId"

puts "Done! Now you can manually inspect the database to ensure that the records exist and match the input params. When finished, ./api.sh delete should work, and another visual confirmation of the DB at that point should be adequate."