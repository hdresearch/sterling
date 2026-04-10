FROM ubuntu:noble-20250925

# Dependencies
RUN apt-get update
RUN apt-get install -y build-essential
RUN apt-get install -y clang
RUN apt-get install -y dotnet-sdk-8.0
RUN apt-get install -y gnupg
RUN apt-get install -y iproute2
RUN apt-get install -y jq
RUN apt-get install -y libclang-dev
RUN apt-get install -y libelf-dev
RUN apt-get install -y libsqlite3-dev
RUN apt-get install -y llvm
RUN apt-get install -y mono-devel
RUN apt-get install -y socat
RUN apt-get install -y zlib1g-dev
RUN apt-get install -y curl
RUN apt-get install -y kmod
RUN apt-get install -y ceph-common
RUN apt-get install -y sudo

# todo Can be removed after PR 397
RUN apt-get install -y iptables

ARG FIRECRACKER_VERSION=v1.13.1

# Get Firecracker and jailer
RUN cd /tmp \
    && curl -L https://github.com/firecracker-microvm/firecracker/releases/download/${FIRECRACKER_VERSION}/firecracker-${FIRECRACKER_VERSION}-x86_64.tgz | tar -xz
RUN mv /tmp/release-$FIRECRACKER_VERSION-x86_64/firecracker-$FIRECRACKER_VERSION-x86_64 /usr/local/bin/firecracker \
    && mv /tmp/release-$FIRECRACKER_VERSION-x86_64/jailer-$FIRECRACKER_VERSION-x86_64 /usr/local/bin/jailer \
    && chmod +x /usr/local/bin/firecracker /usr/local/bin/jailer

COPY <<"EOT" /etc/ceph/ceph.conf
# minimal ceph.conf for e00ab455-a3b6-11f0-8b5d-16ffc5ba0655
[global]
	fsid = e00ab455-a3b6-11f0-8b5d-16ffc5ba0655
	mon_host = [v2:172.31.10.135:3300/0,v1:172.31.10.135:6789/0] [v2:172.31.7.46:3300/0,v1:172.31.7.46:6789/0] [v2:172.31.10.40:3300/0,v1:172.31.10.40:6789/0]

EOT

# This key is invalid, replace it when get Chelsea dockerized
COPY <<"EOT" /etc/ceph/ceph.client.chelsea.keyring
[client.chelsea]
	key = XXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXXX

EOT

WORKDIR /srv

COPY vmlinux.gz /var/lib/chelsea/kernels/
RUN cd /var/lib/chelsea/kernels/ && gunzip vmlinux.gz
RUN mv /var/lib/chelsea/kernels/vmlinux /var/lib/chelsea/kernels/default.bin

COPY chelsea chelsea
COPY .env .env

# Chelsea hasn't decided on its ports yet
EXPOSE 80     # Future Chelsea default?
EXPOSE 8111   # Current Chelsea default
EXPOSE 51820  # Wireguard

CMD ["./chelsea"]
