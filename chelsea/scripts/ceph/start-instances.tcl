#!/usr/bin/env tclsh

# Required AWS env vars (unless CLI is authed):
# AWS_ACCESS_KEY_ID
# AWS_SECRET_ACCESS_KEY
# AWS_REGION

set instanceNames [lmap x {1 2 3} {format "arden-ceph-test_%s" $x}]

puts "This script will start the following instances: [join $instanceNames {, }]"
puts "Type 'yes' to continue."
if {[gets stdin] != "yes"} {
    puts "Aborting."
    exit 1
}

proc getInstanceId {instanceName} {
    set id [exec aws ec2 describe-instances \
        --filters "Name=tag:Name,Values=$instanceName" \
        --query [subst -nocommands {Reservations[*].Instances[*].InstanceId}] \
        --output text]

    if {$id == ""} {
        puts "No instance found with name '$instanceName'. Aborting."
        exit 1
    }

    puts "Found ID for $instanceName: $id"
    return $id
}

proc startInstances {instanceIds} {
    if {[llength $instanceIds] == 0} {
        puts "No instances to start!"
        return
    }
    puts -nonewline "Starting... "
    exec aws ec2 start-instances --instance-ids {*}$instanceIds
    puts "Done!"
}

set instanceIds [lmap instanceName $instanceNames {getInstanceId $instanceName}]
startInstances $instanceIds

puts "Done!"
puts "TODO: If these instances were previously configured with a Ceph cluster, you'll need to set up a loop device on their backing files. Run:\nlosetup -f /var/lib/chelsea-ceph/backingFile.img"
puts "I tried to do this automatically but I ran into some issues retrieving the IP after status checks passed so I'm not worried about this atm. Be sure to restart the OSD daemons if they've errored (ceph orch ps; ceph orch daemon restart NAME)"