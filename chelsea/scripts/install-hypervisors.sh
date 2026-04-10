#!/usr/bin/env bash

sudo mkdir -p /usr/local/bin

sudo aws s3 cp s3://sh.vers.hypervisors/firecracker /usr/local/bin/firecracker
sudo aws s3 cp s3://sh.vers.hypervisors/jailer /usr/local/bin/jailer
sudo chmod +x /usr/local/bin/firecracker
sudo chmod +x /usr/local/bin/jailer

sudo aws s3 cp s3://sh.vers.hypervisors/cloud-hypervisor /usr/local/bin/cloud-hypervisor
sudo chmod +x /usr/local/bin/cloud-hypervisor
