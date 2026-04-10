#!/usr/bin/env tclsh

# Required AWS env vars (unless CLI is authed):
# AWS_ACCESS_KEY_ID
# AWS_SECRET_ACCESS_KEY
# AWS_REGION

set instanceType r6g.medium
# GiB
set volumeSize 60

puts "This script will provision 3 $instanceType instances with $volumeSize GiB gp3 EBS volumes attached."
puts "It assumes the existence of certain AWS resources; consult the script source to see what these are."
puts "Type 'yes' to continue."

set response [gets stdin]
if {$response ne "yes"} {
    puts "Aborting."
    exit 1
}

proc runInstance {instanceName} {
    global instanceType
    global volumeSize

    set blockDeviceMapping [subst -nocommands {[{
        "DeviceName": "/dev/sda1",
        "Ebs": {
            "VolumeSize": $volumeSize,
            "DeleteOnTermination": true,
            "VolumeType": "gp3"
        }
    }]}]

    set tagSpecifications [subst -nocommands {ResourceType=instance,Tags=[{Key=Name,Value=$instanceName}]}]

    puts -nonewline "Creating $instanceName... "
    exec aws ec2 run-instances \
        --image-id ami-01b2110eef525172b \
        --count 1 \
        --instance-type $instanceType \
        --key-name arden-ceph-test \
        --security-group-ids sg-0ed0f3b18307465e8 \
        --subnet-id subnet-05197dd696e67a88f \
        --associate-public-ip-address \
        --block-device-mappings $blockDeviceMapping \
        --placement AvailabilityZone=us-east-1f \
        --tag-specifications $tagSpecifications
    puts "Done!"
}

foreach identifier {1 2 3} {
    runInstance "arden-ceph-test_$identifier"
}