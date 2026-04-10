#! /usr/bin/env tclsh

set oldFile .env.old
set newFile .env.new

if {[file exists $oldFile] != 1} {
    puts "Failed to find $oldFile in the working directory. Aborting."
    exit
}

puts "This script uses a file named '$oldFile' as input and will migrate it to a file name '$newFile', both in the current directory. This will not mutate the original. This script is current as of October 7th and is meant solely to migrate the old (pre-v2 next branch merge) .env to the new format.\nType 'yes' to continue."
if {[gets stdin] != "yes"} {
    puts "Aborting."
    exit
}

# Read vars from old file into oldVars
set fd [open $oldFile]

while {[gets $fd line] >= 0} {
    if {[regexp {^(.*)=(.*)$} $line match key value] != 1} {
        continue
    }

    set oldVars($key) $value
}

close $fd

# Write vars into new file
set fd [open $newFile w]

proc writeWithDefault {newKey oldKey default} {
    upvar oldVars oldVars
    upvar fd fd

    if {[info exists oldVars($oldKey)]} {
        puts $fd "$newKey=$oldVars($oldKey)"
    } else {
        puts $fd "$newKey=$default"
    }
}

# Helper to write a comment
proc writeComment {text} {
    upvar fd fd
    puts $fd "# $text"
}

# Helper to write a blank line
proc writeBlank {} {
    upvar fd fd
    puts $fd ""
}

# Actaully write new file
writeWithDefault "RUST_LOG" "RUST_LOG" "info,aws_smithy_runtime=info,aws_smithy_runtime_api=info,aws_sdk_s3=info,aws_smithy_checksums=info"
writeBlank

writeComment "The port on which the Chelsea server API will live"
writeWithDefault "CHELSEA_SERVER_PORT" "CHELSEA_SERVER_PORT" "80"
writeBlank

writeComment "AWS bucket+auth info"
writeWithDefault "AWS_COMMIT_BUCKET_NAME" "CHELSEA_REMOTE_BUCKET_NAME" ""
writeWithDefault "AWS_ACCESS_KEY_ID" "AWS_ACCESS_KEY_ID" ""
writeWithDefault "AWS_SECRET_ACCESS_KEY" "AWS_SECRET_ACCESS_KEY" ""
writeWithDefault "AWS_REGION" "AWS_REGION" ""
writeBlank

writeComment "Base data directory - where all Chelsea data will be stored"
writeWithDefault "DATA_DIR" "DATA_DIR" "/var/lib/chelsea"
writeBlank

writeComment "Database configuration"
writeWithDefault "DB_NAME" "CHEESE" "chelsea.db"
writeBlank

writeComment "Data dir subdirs"
writeWithDefault "DB_SUBDIR" "CHEESE" "db"
writeComment "chelsea_monitor logs"
writeWithDefault "MONITORING_LOG_SUBDIR" "MONITORING_LOG_SUBDIR" "monitor_logs"
writeComment "stdout/stderr from VM processes"
writeWithDefault "PROCESS_LOG_SUBDIR" "VM_LOG_SUBDIR" "process_logs"
writeComment "Directory to download kernels to"
writeWithDefault "KERNEL_SUBDIR" "KERNEL_SUBDIR" "kernels"
writeComment "VM commit subdir"
writeWithDefault "COMMIT_SUBDIR" "COMMIT_SUBDIR" "commits"
writeBlank

writeComment "Resource margins - how much memory/CPU to keep available for the host"
writeWithDefault "MEM_SIZE_MIB_MARGIN" "MEMORY_MIB_MARGIN" "8196"
writeWithDefault "VCPU_CORES_MARGIN" "CPU_CORES_MARGIN" "6"
writeBlank

writeComment "Hard maxima - individual VMs may not exceed these counts"
writeWithDefault "MEM_SIZE_MIB_VM_MAX" "VM_MAX_MEM_MIB" "8196"
writeWithDefault "VCPU_COUNT_VM_MAX" "VM_MAX_VCPU_COUNT" "4"
writeWithDefault "FS_SIZE_MIB_VM_MAX" "VM_MAX_FS_MIB" "16392"
writeBlank

writeComment "Default values for VM creation"
writeWithDefault "VM_DEFAULT_IMAGE_NAME" "ROOTFS_NAME" "default"
writeWithDefault "VM_DEFAULT_KERNEL_NAME" "KERNEL_NAME" "default.bin"
writeWithDefault "VM_DEFAULT_VCPU_COUNT" "CHEESE" "1"
writeWithDefault "VM_DEFAULT_MEM_SIZE_MIB" "CHEESE" "512"
writeWithDefault "VM_DEFAULT_FS_SIZE_MIB" "CHEESE" "1024"
writeBlank

writeComment "Network configuration"
writeWithDefault "NETWORK_INTERFACE" "PHYSICAL_INTERFACE" "enp125s0"
writeWithDefault "VM_SUBNET" "CHEESE" "192.168.100.0/24"
writeWithDefault "NETWORK_RESERVE_TIMEOUT_SECS" "CHEESE" "10"
writeBlank

writeComment "SSH port range for VMs (inclusive start, exclusive end in code)"
writeWithDefault "VM_SSH_PORT_START" "CHEESE" "28000"
writeWithDefault "VM_SSH_PORT_END" "CHEESE" "28128"
writeBlank

# Not sure if this should still exist.
writeComment "Path to the firecracker executable"
writeWithDefault "FIRECRACKER_BIN_PATH" "CHEESE" "/usr/local/bin/firecracker"
writeComment "The maximum time, in seconds, we will wait for a response from the Firecracker API"
writeWithDefault "FIRECRACKER_API_TIMEOUT_SECS" "CHEESE" "30"
writeBlank

writeComment "For any base Ceph image, there must exist a snap from which all clones will be created - chelsea and the Ceph cluster must agree on this"
writeWithDefault "CEPH_BASE_IMAGE_SNAP_NAME" "CHEESE" "chelsea_base_image"
writeBlank

writeComment "The RELATIVE PATH (do not prefix with /) path at which the VM's root drive will be mounted"
writeWithDefault "VM_ROOT_DRIVE_PATH" "CHEESE" "dev/vda1"

close $fd

puts "Migration complete. Wrote to $newFile."
