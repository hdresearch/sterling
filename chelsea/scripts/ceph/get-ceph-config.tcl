#!/usr/bin/env tclsh

# Required AWS env vars (unless CLI is authed):
# AWS_ACCESS_KEY_ID
# AWS_SECRET_ACCESS_KEY
# AWS_REGION

# Alternatively, you can use -the ip [addr] flag to pass in an IP to skip the AWS describe check

set instanceId arden-ceph-test_1
set keyringPath "/etc/ceph/ceph.client.chelsea.keyring"
set configPath "/etc/ceph/ceph.conf"
set sshKeyName "arden-ceph-test.pem"

if {![file exists $sshKeyName]} {
    puts "Please ensure $sshKeyName is present in the working directory."
    exit 1
}

set ipPos [lsearch $argv "-ip"]
if {$ipPos >= 0} {
    set instanceIp [lindex $argv [expr $ipPos + 1]]
}

if {[info exists instanceIp]} {
    set keyringSrc $instanceIp
} else {
    set keyringSrc $instanceId
}
puts "This script will download the client.chelsea keyring from $keyringSrc and save to $keyringPath. (Pass in -ip \[ip\] if you do not have AWS EC2 privileges on the machine.)"
puts "Type 'yes' to continue."

if {[gets stdin] != "yes"} {
    puts "Aborting."
    exit 0
}

proc getInstancePublicIp {instanceId} {
    set ip [exec aws ec2 describe-instances --filters "Name=tag:Name,Values=$instanceId" --query {Reservations[*].Instances[*].PublicIpAddress} --output text]
    if {$ip == ""} {
        puts "Failed to retrieve IP for $instanceId. Aborting."
        exit 1
    }
    puts "Found IP for $instanceId: $ip"
    return $ip
}

if {![info exists instanceIp]} {
    set instanceIp [getInstancePublicIp $instanceId]
}

proc scp {instanceIp src dest} {
    global sshKeyName
    if {[catch {exec sudo scp -i $sshKeyName -o StrictHostKeyChecking=accept-new root@$instanceIp:$src $dest} result]} {
        set reKnownHost {Warning: Permanently added '\d+\.\d+\.\d+\.\d+' .* to the list of known hosts\.}
        if [regexp $reKnownHost $result] {
            puts "Added $instanceIp to the list of known hosts."
        } else {
            error $result
        }
    }
}

exec sudo mkdir -p [file dirname $keyringPath]
# configPath currently has the same parent dir as keyringPath

if {[file exists $keyringPath]} {
    exec sudo rm $keyringPath
}
scp $instanceIp $keyringPath $keyringPath

if {[file exists $configPath]} {
    exec sudo rm $configPath
}
scp $instanceIp $configPath $configPath
