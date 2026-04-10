#!/bin/bash

user=$(whoami)

# ┌────────────────────────────────────────────────────────────────────────────┐
# │                                                                            │
# │                         Setup Product Dependencies                         │
# │                                                                            │
# │             To actually run everything, see "../single-node.sh"            │
# │                                                                            │
# └────────────────────────────────────────────────────────────────────────────┘

# Test if we are runing as root
if [ "$EUID" -ne 0 ]; then
  echo "Please run with sudo or as root"
  exit
fi

# ┌────────────────────────────────────────────────────────────────────────────┐
# │                             Setup for AWS CLI                              │
# └────────────────────────────────────────────────────────────────────────────┘
curl "https://awscli.amazonaws.com/awscli-exe-linux-x86_64.zip" -o awscliv2.zip
unzip -o awscliv2.zip && ./aws/install --update
rm -r ./aws ./awscliv2.zip

# ┌────────────────────────────────────────────────────────────────────────────┐
# │                                We need DBMate                              │
# └────────────────────────────────────────────────────────────────────────────┘
curl -fsSL -o /usr/local/bin/dbmate https://github.com/amacneil/dbmate/releases/latest/download/dbmate-linux-amd64
chmod +x /usr/local/bin/dbmate


# ┌────────────────────────────────────────────────────────────────────────────┐
# │              Add current User and "ubuntu" to "docker" Group               │
# └────────────────────────────────────────────────────────────────────────────┘
usermod -a -G docker $user

if [ "$user" != "ubuntu" ]; then
  usermod -a -G docker ubuntu
fi

# ┌────────────────────────────────────────────────────────────────────────────┐
# │                         Get Default Kernel from S3                         │
# └────────────────────────────────────────────────────────────────────────────┘
./scripts/install-kernels.sh

# ┌────────────────────────────────────────────────────────────────────────────┐
# │                          Build Firecracker+Jailer                          │
# └────────────────────────────────────────────────────────────────────────────┘
./scripts/install-hypervisors.sh
