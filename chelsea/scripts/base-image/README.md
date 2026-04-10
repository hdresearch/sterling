This subdir contains scripts related to base image customization. This mainly refers to the config files, scripts, and services we need to insert on the VMs.  

- Prerequisite: `create-default-image.sh` automatically builds `chelsea-agent` via `cargo` (run as the invoking user or, when executed through `sudo`, the original user). Set `SKIP_CHELSEA_AGENT_BUILD=1` to skip or provide your own `CHELSEA_AGENT_BIN`.

- `configure-image.sh` The "core" of the base image customization. Contains the main operations we need to perform on any base image.
- `create-base-image.sh` Expects two positional parameters, `<image-name>` and `<source-dir>`. Configures `source-dir` and creates an image on Ceph via CLI with name `image-name`.
- `create-default-image.sh` Fetches the default rootfs from S3, customizes it, and creates an image on Ceph with name `default`. This is, as one might suspect, the default value assumed by the chelsea API when an image name is not otherwise specified in the body. NOTE: Unlike `create-base-image.sh`, this script is intended to be run on a Ceph node rather than a client machine. This is an oversight and [will be addressed](https://github.com/hdresearch/chelsea/issues/519).
- `fetch-fs.sh` Mostly unused at the moment, but preserved for reference. This used to be the script chelsea ran in order to fetch and customize the rootfs. Notably, this script contains the usage of the FS customization tool, which, as explained in https://github.com/hdresearch/chelsea/issues/516, needs to be re-introduced at some point (unless deprecated.)
