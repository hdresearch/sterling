# Chelsea
A manager for Firecracker VMs.

### API key management
```bash
./commands.sh init-db  # If DB doesn't exist already (automatically created by daemon)
./commands.sh create-api-key
./commands.sh list-api-keys
./commands.sh delete-api-key API_KEY
```
```bash
sqlite3 /var/lib/chelsea/db/chelsea.db
```

### Making test API requests
```bash
curl 0.0.0.0/api/health  # Should return 'ok'
./api.sh  # Will display usage info
```

### Node Metadata
`/etc/chelsea-id` contains a uuidv4 that identifies the chelsea node.
This is created in Constants.rs `get_or_create_chelsea_id` line 45 as of 8/6/25.

### Testing
```bash
# Retrieve 2 valid API keys from the DB
./scripts/api-test.sh 0.0.0.0 API_KEY1 API_KEY2
```
To get into a VM using ssh:
```sh
vm_id=""
vm_ssh_private_key_file="" # get from ./public-api.sh ssh-key <vm_id>, ignore port
ssh -o HostKeyAlias="$vm_id.vm.vers.sh" -o ProxyCommand="openssl s_client -quiet -servername $vm_id.vm.vers.sh -connect localhost:443" -i $vm_ssh_private_key_file root@localhost
```

### Cleaning up dangling resources
```bash
./commands.sh cleanup
```

### Building, running, stopping the daemon
```bash
make run  # build and run release target
```
```bash
make run-debug  # build and run debug target
```
```bash
make build  # build release target
```
```bash
make build-debug  # build debug target
```
```bash
make stop
```

### (re)generate OpenAPI docs
```bash
make api-docs
```
The output will be in `./openapi`

### Default kernel and rootfs management
If the default kernel or rootfs get corrupted or deleted, they can be refetched (with the daemon running) using:
```bash
./commands.sh fetch-fs
./commands.sh fetch-kernel
```

### VM metadata
Each VM will have a file located at `/etc/vminfo` containing a JSON blob with VM metadata. As of May 15, 2025, this is only the VM ID. The struct definition for this blob is `VmMetadata`.

### SSH port forwarding
IMPORTANT: When using a custom image generated via the `/api/rootfs` endpoint, the uploaded Dockerfile is responsible for ensuring that `sshd` is running, and that the `ip` utility (found in the `iproute2` package) is installed. The network configuration scripts will fail if not, because they rely on `ip`.
On AWS, ensure that the ports specified in the NetworkManager's `network_ranges` are open in the node's security group settings.

### Data directories
All data directories are customizable via their respective env vars. For simplicity, their default values are referred to here.
`/var/lib/chelsea`: The base data directory.
`/var/lib/chelsea/commits`: Directories containing commit files, both those that have been created locally and those that have been fetched from remote sources. Contains a subdirectory for each commit ID, containing both a .sha512 file (checksum) and a .json file (commit metadata).
`/var/lib/chelsea/images`: Disk "images" for VMs. These are created only on commit/download commit. Contains .img files.
`/var/lib/chelsea/db`: The local application database; tracks resources to enable non-destructive daemon restarts. Contains a single chelsea.db SQLite database file.
`/var/lib/chelsea/kernels`: Kernel binaries. These are valid bases for new clusters. Contains a single default.bin file.
`/var/lib/chelsea/logs`: Service-level logs. Every time a service function is called, a log is created here. Contains {operation_id}.json files for each service function invocation.
`/var/lib/chelsea/rootfs`: Rootfs "images." These are valid bases for new clusters. Contains a single `default` directory and a directory for each uploaded Docker-built rootfs. (May be deprecated soon?)
`/var/lib/chelsea/snapshots`: VM snapshots, including mem, state, fs, checksum, and metadata files. Contains subdirectories for each VM/commit key containin a .mem and .state file.
`/var/lib/chelsea/sshkeys`: Private keys for cluster/VM SSH keys. Contains an id_{id} file for each cluster/commit.
Ensure that the ports specified in the NetworkManager's `network_ranges` are open in the node's security group settings.

### Dockerfile upload
(Note: Dockerfile upload is indefinitely deprecated as of 24 July, 2025)

### Rootfs Dependencies
IMPORTANT FOR ROOTFS: We currently have hard dependencies on `sshd` and `ip` (`iproute2` package.) If either of these is not installed, the rootfs will be unusable.

### Rootfs Customization Tool
The `./tools` subdirectory currently is the home for our rootfs customization tool. Each time `fetch-fs` is run (eg: via `./commands.sh fetch-fs`), all files that are in the staging directory (`$CHELSEA_BIN_DIR/tools/data/fs.in`) will be copied to the fetched rootfs as-is, preserving the directory tree. Adding or removing files to the staging directory is done as follows:
1) Create the file in `$CHELSEA_SRC_DIR/tools/data/fs.in`
2) Add the file's path (relative to `fs.in`) to `$CHELSEA_SRC_DIR/tools/data/fileSystem.lst`; each file is newline-separated.
3) When running `make run`, the `test-customize-fs` target will build the deployment database, and the `test-install-release` target will copy the files in `tools` to the bin directory (eg: `$CHELSEA_SRC_DIR/target/release`). The deployment database `tools/data/fileSystem.db` contains additional file metadata, including a PGP signature that is verified as the files are copied to the rootfs.
4) Now, on each subsequent invocation of `fetch-fs`, the updated staging directory will be copied to the new rootfs.

### VM dependencies
An SSH server
iproute2

### Node dependencies
dotnet-sdk-8.0 mono-devel zlib1g-dev

### VM Networking setup
Each VM is configured inside a netns to ensure that it is able to retain the same IP address across commit operations. A veth pair connects the netns to the default namespace. A Wireguard interface is present inside the netns, to be configured based on Wireguard params passed to any VM creation endpoint. This allows incoming traffic to be securely routed through our proxy directly to the VM. A TAP device is created with 2 constant IP addresses: `192.168.1.1/30`, and `fd00:fe11:deed:1337::1/126`. This TAP is used by Firecracker to configure a virtual networking device on the VM, which we assign the IP addresses `192.168.1.2/30` and `fd00:fe11:deed:1337::2/126`. This is done through the `vmnet-setup.sh` script located on our base images, which is generated by scripts such as `configure-image.sh` and `create-default-image.sh`. We rely on inserted systemd services to do this config currently.

This ultimately means that we're DNAT'ing all traffic into the VM's netns to `192.168.1.2` / `fd00:fe11:deed:1337::2`, which gets routed to the VM via the TAP, which is used by Firecracker to config the `eth0` device on the VM.
