FROM --platform=linux/amd64 oven/bun:1-debian AS base
WORKDIR /usr/src/app

# Install system dependencies
RUN apt-get update && apt-get install -y \
    bindfs \
    fuse \
    libfuse-dev \
    pkg-config \
    build-essential \
    curl \
    wget \
    tar \
    e2fsprogs \
    procps \
    iproute2 \
    iptables \
    iputils-ping \
    openssh-client \
    netcat \
    net-tools \
    squashfs-tools \
    && rm -rf /var/lib/apt/lists/*

# Install Firecracker
RUN wget https://github.com/firecracker-microvm/firecracker/releases/download/v1.3.0/firecracker-v1.3.0-x86_64.tgz \
    && tar -xvf firecracker-v1.3.0-x86_64.tgz \
    && mv release-v1.3.0-x86_64/firecracker-v1.3.0-x86_64 /usr/local/bin/firecracker \
    && rm -rf firecracker-v1.3.0-x86_64.tgz release-v1.3.0-x86_64

# Create VM directories and download images
RUN mkdir -p /var/lib/firecracker/kernels \
    /var/lib/firecracker/rootfs \
    && wget https://s3.amazonaws.com/spec.ccfc.min/img/quickstart_guide/x86_64/kernels/vmlinux.bin \
    -O /var/lib/firecracker/kernels/vmlinux.bin \
    && wget https://s3.amazonaws.com/spec.ccfc.min/img/quickstart_guide/x86_64/rootfs/bionic.rootfs.ext4 \
    -O /var/lib/firecracker/rootfs/rootfs.ext4

# Setup network script
COPY scripts/setup-network.sh /usr/local/bin/
RUN chmod +x /usr/local/bin/setup-network.sh

# Now handle Bun dependencies
COPY package.json .

# Install dependencies
FROM base AS install
RUN mkdir -p /temp/dev
COPY package.json  /temp/dev/
RUN cd /temp/dev && bun install

# Install production dependencies
RUN mkdir -p /temp/prod
COPY package.json /temp/prod/
RUN cd /temp/prod && bun install --production



# Final image
FROM base AS release
COPY --from=install /temp/dev/node_modules node_modules
COPY . .

COPY scripts/add-ssh-key.sh /usr/local/bin/
RUN chmod +x /usr/local/bin/add-ssh-key.sh
# Create startup script for development mode
RUN echo '#!/bin/bash\n\
    /usr/local/bin/setup-network.sh\n\
    /usr/local/bin/add-ssh-key.sh\n\
    exec bun run dev' > /usr/local/bin/start.sh \
    && chmod +x /usr/local/bin/start.sh



EXPOSE 3000/tcp
ENTRYPOINT [ "/usr/local/bin/start.sh" ]