#!/usr/bin/env tclsh

# Required AWS env vars (unless CLI is authed):
# AWS_ACCESS_KEY_ID
# AWS_SECRET_ACCESS_KEY
# AWS_REGION

set instanceIds [lmap x {1 2 3} {format "arden-ceph-test_%s" $x}]
set sshKeyName "arden-ceph-test.pem"

# GiB
set backingFileSize 50

set instance1 [lindex $instanceIds 0]
set instance2 [lindex $instanceIds 1]
set instance3 [lindex $instanceIds 2]

# To save time during debugging, if dependencies were already installed, pass -skipdeps to avoid pointless apt invocations
set skipDeps [expr {[lsearch $argv -skipdeps] + 1}]
set printDebug [expr {[lsearch $argv -v] + 1}]

proc debugs {msg} {
    global printDebug
    if {$printDebug > 0} {
        puts $msg
    }
}

set sshUser ubuntu

puts "This script will configure the following EC2 instances: [join $instanceIds {, }]
The following actions will be taken:
- The file at /home/$sshUser/.ssh/authorized_keys will be copied to /root/.ssh/authorized_keys, overriding the warning that normally prevents SSHing in as root
- Dependencies for cephadm and ceph-common installed
- LVM logical volumes initialized over $backingFileSize GiB backing files on each machine
- $sshKeyName will be copied to each instance
- A Ceph cluster bootstrapped on $instance1 (manager + monitor)
- 2 additional monitors set up, deployed to $instance2 and $instance3
- OSDs added on the logical volumes mentioned above
- An OSD pool will be created and initialized for RBD
"
puts "To do this, ensure the following:
- $sshKeyName is in the same directory as this script
"
puts "Type 'yes' to continue."
if {[gets stdin] != yes} {
    puts "Aborting."
    exit 1
}

# Get instance IPs
proc getInstancePublicIp {instanceId} {
    set ip [exec aws ec2 describe-instances --filters "Name=tag:Name,Values=$instanceId" --query {Reservations[*].Instances[*].PublicIpAddress} --output text]
    if {$ip == ""} {
        puts "Failed to retrieve IP for $instanceId. Aborting."
        exit 1
    }
    puts "Found IP for $instanceId: $ip"
    return $ip
}

set ip1 [getInstancePublicIp $instance1]
set ip2 [getInstancePublicIp $instance2]
set ip3 [getInstancePublicIp $instance3]

proc ssh {instanceIp cmd} {
    global sshUser
    global sshKeyName

    set host [format "%s@%s" $sshUser $instanceIp]

    debugs "ssh: ssh -o StrictHostKeyChecking=accept-new -i $sshKeyName $host $cmd"
    if {[catch {exec ssh -o StrictHostKeyChecking=accept-new -i $sshKeyName $host $cmd} result options]} {
        set reKnownHost {Warning: Permanently added '\d+\.\d+\.\d+\.\d+' .* to the list of known hosts\.}
        if {[regexp $reKnownHost $result] == 0} {
            if {[dict get $options -errorcode] != "NONE"} {
                error $result
            } else {
                return $result
            }
        } else {
            puts "Added $instanceIp to the list of known hosts."
            # It's no longer an error, just regular output
            return $result
        }
    } else {
        return $result
    }
}

# If the bootstrap node already has an FSID, recommend removing it and prevent starting a second cluster
if {[catch {ssh $ip1 "sudo ceph fsid"} result]} {
    puts "No Ceph cluster found on $ip1. Beginning cluster configuration."
} else {
    set fsid $result
    set fsidRegexp {[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}}
    if {[regexp $fsidRegexp $fsid] == 1} {
        puts "There is already a Ceph cluster on $ip1. Please run the following command on this machine to remove the existing cluster first:\ncephadm rm-cluster --force --zap-osds --fsid $fsid"
        exit 1
    }
}

# Overwrite the default warning preventing SSH as root, by permitting the `ubuntu` SSH key to log in as root. (`ubuntu` has sudoer privilege anyway)
proc overwriteRootSshAuthorizedKeys {ip} {
    global sshUser
    ssh $ip "sudo cp /home/$sshUser/.ssh/authorized_keys /root/.ssh/authorized_keys"
}

foreach ip "$ip1 $ip2 $ip3" {
    puts "Allowing root login for $ip"
    overwriteRootSshAuthorizedKeys $ip
}

# Ensure dependencies are installed on each instance
proc installDependencies {instanceIp} {
    ssh $instanceIp {sudo apt-get update && sudo DEBIAN_FRONTEND="noninteractive" apt-get install -y cephadm ceph-common}
}

if {$skipDeps == 0} {
    foreach ip "$ip1 $ip2 $ip3" {
        puts "Installing dependencies for $ip"
        installDependencies $ip
    }
} else {
    puts "Skipping dependency installation"
}

# Create 50 GB LVM logical volumes on each instance
set backingFilePath {/var/lib/chelsea-ceph/backingFile.img}
set volumeGroupName {vg-ceph}
set logicalVolumeName {lv-ceph}

proc createLvmVolume {instanceIp} {
    global backingFilePath
    global backingFileSize
    global volumeGroupName
    global logicalVolumeName

    # Check if logical volume already exists
    puts "Creating $backingFileSize GiB LVM volume for $instanceIp at $volumeGroupName/$logicalVolumeName (Backing file at $backingFilePath)"

    # Create backing file if it doesn't exist
    ssh $instanceIp "sudo mkdir -p [file dirname $backingFilePath]"
    set backingFileExists [ssh $instanceIp "test -f $backingFilePath && echo exists || echo missing"]
    if {[string trim $backingFileExists] == "missing"} {
        ssh $instanceIp "sudo fallocate -l [set backingFileSize]G $backingFilePath"
    } else {
        debugs "Backing file already exists, skipping creation"
    }

    # Check if loop device is already set up for this backing file
    set existingLoop [ssh $instanceIp "sudo losetup -j $backingFilePath | cut -d: -f1"]
    if {[string trim $existingLoop] != ""} {
        debugs "Loop device already exists: $existingLoop"
        set loopDevicePath [string trim $existingLoop]
    } else {
        set loopDevicePath [ssh $instanceIp "sudo losetup -f --show $backingFilePath"]
    }

    # Check if PV already exists
    set pvExists [ssh $instanceIp "sudo pvs $loopDevicePath 2>/dev/null | grep -q $loopDevicePath && echo exists || echo missing"]
    if {[string trim $pvExists] == "missing"} {
        ssh $instanceIp "sudo pvcreate $loopDevicePath"
    } else {
        debugs "Physical volume already exists on $loopDevicePath"
    }

    # Check if VG already exists
    set vgExists [ssh $instanceIp "sudo vgs $volumeGroupName 2>/dev/null | grep -q $volumeGroupName && echo exists || echo missing"]
    if {[string trim $vgExists] == "missing"} {
        ssh $instanceIp "sudo vgcreate $volumeGroupName $loopDevicePath"
    } else {
        debugs "Volume group $volumeGroupName already exists"
    }

    # Check if LV already exists
    set lvs [ssh $instanceIp "sudo lvs"]
    if {[string match *$logicalVolumeName* $lvs]} {
        puts "Found $logicalVolumeName for $instanceIp; skipping"
    } else {
        ssh $instanceIp "sudo lvcreate -l 100%FREE -n $logicalVolumeName $volumeGroupName"
    }
}

foreach ip "$ip1 $ip2 $ip3" {
    createLvmVolume $ip
}

# Copy the instance private key to each instance (ensures that each is able to perform admin tasks on the others)
proc copySshKey {instanceIp} {
    global sshUser
    global sshKeyName

    set host [format "%s@%s" $sshUser $instanceIp]
    exec scp -i $sshKeyName $sshKeyName $host:/home/$sshUser/id_rsa
    ssh $instanceIp "sudo mv /home/$sshUser/id_rsa /root/.ssh/id_rsa"
}

foreach ip "$ip1 $ip2 $ip3" {
    puts "Copying SSH key for $ip"
    copySshKey $ip
}

# Bootstrap the cluster with a manager and monitor
proc getPrivateIp {instanceIp} {
    set ens5 [ssh $instanceIp "ip address show ens5"]
    set re {inet (\d+\.\d+\.\d+\.\d+)}
    if {[regexp $re $ens5 match privateIp] == 0} {
        error "Failed to extract private IP for $instanceIp. Aborting."
    }
    puts "Found private IP for $instanceIp: $privateIp"
    return $privateIp
}

set privateIp1 [getPrivateIp $ip1]
set privateIp2 [getPrivateIp $ip2]
set privateIp3 [getPrivateIp $ip3]

puts "Bootstrapping Ceph cluster on $ip1"
ssh $ip1 "sudo cephadm bootstrap --mon-ip $privateIp1 --skip-monitoring-stack"

# Add the other two nodes as hosts
proc hostnameFromPrivateIp {privateIp} {
    # Implicit assumption that the hostname of the bootstrap node is `ip-a-b-c-d` where the original private IP is a.b.c.d; this function is meant to match this pattern
    return "ip-[regsub -all {\.} $privateIp -]"
}

set host1 [hostnameFromPrivateIp $privateIp1]
set host2 [hostnameFromPrivateIp $privateIp2]
set host3 [hostnameFromPrivateIp $privateIp3]

proc addHost {host privateIp} {
    global ip1
    puts "Adding host $host $privateIp"
    ssh $ip1 "sudo ssh-copy-id -f -i /etc/ceph/ceph.pub -o StrictHostKeyChecking=accept-new root@$privateIp"
    ssh $ip1 "sudo ceph orch host add $host $privateIp"
    ssh $ip1 "sudo ceph orch host label add $host _admin"
}

addHost $host2 $privateIp2
addHost $host3 $privateIp3

# Tell the orchestrator to attempt to place 3 monitors
puts "Applying 3 monitors"
ssh $ip1 "sudo ceph orch apply mon --placement=3"

# Explicitly create OSDs on the three hosts (ensuring that they specifically use the LVM volume rather than "any available device" - the default for ceph orch apply osd)
foreach host "$host1 $host2 $host3" {
    set deviceName /dev/$volumeGroupName/$logicalVolumeName
    puts "Adding OSD on $host:$deviceName"
    ssh $ip1 "sudo ceph orch daemon add osd $host:$deviceName"
}

# Create an OSD pool with the default name for an RBD pool: 'rbd'
set poolName rbd
puts "Creating OSD pool '$poolName'"
ssh $ip1 "sudo ceph osd pool create $poolName"

# Initialize the pool for RBD
puts "Initializing pool '$poolName' as RBD pool"
ssh $ip1 "sudo rbd pool init $poolName"

# Create the chelsea RBD user and generate the keyrind on each instance
foreach ip "$ip1 $ip2 $ip3" {
    ssh $ip "sudo ceph auth get-or-create client.chelsea mon 'profile rbd' osd 'profile rbd pool=rbd' mgr 'profile rbd pool=rbd' -o /etc/ceph/ceph.client.chelsea.keyring"
}

puts "Configuration complete!"