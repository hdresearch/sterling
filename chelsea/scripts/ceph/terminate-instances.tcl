#!/usr/bin/env tclsh

# Required AWS env vars (unless CLI is authed):
# AWS_ACCESS_KEY_ID
# AWS_SECRET_ACCESS_KEY
# AWS_REGION

set instanceNames [lmap x {1 2 3} {format "arden-ceph-test_%s" $x}]

puts "This script will terminate the following instances: [join $instanceNames {, }]"
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

proc terminateInstances {instanceIds} {
    if {[llength $instanceIds] == 0} {
        puts "No instances to terminate!"
        return
    }
    puts -nonewline "Terminating... "
    exec aws ec2 terminate-instances --instance-ids {*}$instanceIds
    puts "Done!"
}

set instanceIds [lmap instanceName $instanceNames {getInstanceId $instanceName}]
terminateInstances $instanceIds