.DEFAULT_GOAL := build-only

.PHONY: all-clean api-docs build-drivers build-drivers-firecracker \
        build-only build build-debug-only build-debug build-release-only \
        build-release build-and-run chelsea-api-docs chelsea-docs \
		clean clean-customize-fs \
		clean-docker clean-drivers clean-externals clean-install-debug \
        clean-install-release clean-resources clean-test \
        clean-tmp-and-test-logs clone-and-build-eagle \
        clone-and-build-firecracker clone-and-build-firecracker-kernel \
        clone-and-build-firecracker-rootfs clone-and-build-sds \
		clone-and-install-firecracker \
        clone-and-patch-firecracker configure-docker create-externals \
        copy-env-debug copy-env-release docs \
        force-uninstall-all force-uninstall-shared install \
        install-packages nop run run-debug run-install run-release \
        stop super-clean super-duper-clean terminate test \
        test-customize-fs-only test-deploy-fs test-firecracker \
        test-install-debug test-install-release test-only-preclean \
        test-only test-only-quick test-only-quick-pool test-shell metrics-server-test \
        chelsea-docker-run

nop:
	@echo nop

terminate:
	killall -9 chelsea || true

install-packages:
	apt-get update
	apt-get install -y libelf-dev
	apt-get install -y libsqlite3-dev
	apt-get install -y dotnet-sdk-8.0
	apt-get install -y mono-devel
	apt-get install -y zlib1g-dev
	apt-get install -y gnupg
	apt-get install -y docker.io
	apt-get install -y build-essential
	apt-get install -y clang
	apt-get install -y llvm
	apt-get install -y libclang-dev

create-externals:
	mkdir -p ./externals
	mkdir -p ./externals/sqlite
	mkdir -p ./externals/kernel
	mkdir -p ./externals/rootfs

clone-eagle: create-externals
	test -d ./externals/eagle/ || git clone https://github.com/mistachkin/eagle ./externals/eagle
	rm -rf ./externals/eagle/.git
	rm -f ./externals/eagle/lib/Eagle1.0/vendor.eagle
	test -d ./externals/extra/ || git clone https://github.com/mistachkin/extra ./externals/extra
	rm -rf ./externals/extra/.git

clone-and-build-eagle: clone-eagle clone-and-build-sds
	test -f ./externals/eagle/bin/DebugNetStandard21/bin/netcoreapp3.0/EagleShell.dll || dotnet build ./externals/eagle/EagleNetStandard2X.sln /maxCpuCount:1 /property:EagleBuildType=NetStandard21 /property:EaglePatchLevel=false
	mkdir -p ./lib/Extra1.0/
	cp -r ./externals/extra/LoadOnStartup/Public/* ./lib/Extra1.0/
	cp ./externals/sqlite/System.Data.SQLite.dll ./externals/eagle/bin/DebugNetStandard21/bin/netcoreapp3.0/
	cp ./externals/sqlite/SQLite.Interop.dll ./externals/eagle/bin/DebugNetStandard21/bin/netcoreapp3.0/
	cp ./externals/eagle/Shell/Configurations/dotNet/8.0/EagleShell.runtimeconfig.json ./externals/eagle/bin/DebugNetStandard21/bin/netcoreapp3.0/

clone-and-build-sds: create-externals
	test -d ./externals/sds/ || git clone https://github.com/mistachkin/System.Data.SQLite ./externals/sds
	rm -rf ./externals/sds/.git
	test -f ./externals/sds/sqlite/dotnet/bin/NetStandard21/DebugNetStandard21/bin/netstandard2.1/System.Data.SQLite.dll || (dotnet build ./externals/sds/sqlite/dotnet/SQLite.NET.NetStandard21.MSBuild.sln /maxCpuCount:1 && sn -R ./externals/sds/sqlite/dotnet/bin/NetStandard21/DebugNetStandard21/bin/netstandard2.1/System.Data.SQLite.dll ./externals/sds/sqlite/dotnet/Keys/System.Data.SQLite.snk)
	test -f ./externals/sds/sqlite/dotnet/bin/2013/Debug/bin/SQLite.Interop.dll || ./externals/sds/sqlite/dotnet/Setup/compile-interop-assembly-debug.sh
	cp ./externals/sds/sqlite/dotnet/bin/NetStandard21/DebugNetStandard21/bin/netstandard2.1/System.Data.SQLite.dll ./externals/sqlite/
	cp ./externals/sds/sqlite/dotnet/bin/2013/Debug/bin/SQLite.Interop.dll ./externals/sqlite/

clone-and-patch-firecracker: create-externals
	test -d ./externals/firecracker/ || git clone https://github.com/firecracker-microvm/firecracker ./externals/firecracker
	test -f ./externals/kernel/enable_modules_1.patch.done || (cd ./externals/firecracker && patch -p1 -u -N <../../kernel/patches/enable_modules_1.patch && echo 1 >../../externals/kernel/enable_modules_1.patch.done)
	test -f ./externals/kernel/enable_modules_2.patch.done || (cd ./externals/firecracker && patch -p1 -u -N <../../kernel/patches/enable_modules_2.patch && echo 2 >../../externals/kernel/enable_modules_2.patch.done)
	test -f ./externals/kernel/disable_cmd_fix_perms_1.patch.done || (cd ./externals/firecracker && patch -p1 -u -N <../../kernel/patches/disable_cmd_fix_perms_1.patch && echo 3 >../../externals/kernel/disable_cmd_fix_perms_1.patch.done)

clone-and-build-firecracker-kernel: clone-and-patch-firecracker
	cd ./externals/firecracker/tools && ./devtool build_ci_artifacts kernels 6.1 && sudo chown -f -R "$(shell id --user):$(shell id --group)" ../../../externals/firecracker
	cp "$$(ls ./externals/firecracker/resources/x86_64/vmlinux-6.1.* | sort -V | head -n 1)" ./externals/kernel/vmlinux

clone-and-build-firecracker-rootfs: clone-and-patch-firecracker
	cd ./externals/firecracker/tools && ./devtool build_ci_artifacts rootfs && sudo chown -f -R "$(shell id --user):$(shell id --group)" ../../../externals/firecracker
	cp ./externals/firecracker/resources/x86_64/ubuntu-24.04.squashfs ./externals/rootfs/ubuntu-24.04.squashfs
	cp ./externals/firecracker/resources/x86_64/ubuntu-24.04.manifest ./externals/rootfs/ubuntu-24.04.manifest

configure-docker:
	sudo systemctl start docker
	sudo sh -c 'test -f /etc/docker/daemon.json || echo "{}" > /etc/docker/daemon.json; jq '\''."dns"=["8.8.8.8","1.1.1.1"] | ."dns-opts"=["timeout:1"]'\'' /etc/docker/daemon.json | sudo tee /etc/docker/daemon.json >/dev/null'
	sudo systemctl restart docker

clone-and-build-firecracker: configure-docker clone-and-patch-firecracker
	cd ./externals/firecracker/tools && ./devtool build --release

clone-and-install-firecracker: clone-and-build-firecracker
	install -m0755 ./externals/firecracker/build/cargo_target/$(shell uname -m)-unknown-linux-musl/release/firecracker /usr/local/bin/
	install -m0755 ./externals/firecracker/build/cargo_target/$(shell uname -m)-unknown-linux-musl/release/jailer /usr/local/bin/

build-drivers-firecracker: configure-docker clone-and-build-firecracker-kernel
	cd ./kernel/drivers/user_entropy && make build-firecracker

clean-install-debug:
	rm -rf ./target/debug/lib/ 2>/dev/null || true
	rm -rf ./target/debug/tools/ 2>/dev/null || true

test-install-debug: clone-and-build-sds clone-and-build-eagle clean-install-debug test-customize-fs-only
	mkdir -p ./target/debug/lib
	mkdir -p ./target/debug/tools
	cp --recursive ./lib/* ./target/debug/lib/
	cp --recursive ./tools/* ./target/debug/tools/
	cp ./externals/sqlite/System.Data.SQLite.dll ./target/debug/
	cp ./externals/sqlite/SQLite.Interop.dll ./target/debug/
	cp ./externals/eagle/bin/DebugNetStandard21/bin/netcoreapp3.0/EagleShell.runtimeconfig.json ./target/debug/
	cp ./externals/eagle/bin/DebugNetStandard21/bin/netcoreapp3.0/Eagle.dll ./target/debug/
	cp ./externals/eagle/bin/DebugNetStandard21/bin/netcoreapp3.0/EagleShell.dll ./target/debug/

clean-install-release:
	rm -rf ./target/release/lib/ 2>/dev/null || true
	rm -rf ./target/release/tools/ 2>/dev/null || true

test-install-release: clone-and-build-sds clone-and-build-eagle clean-install-release test-customize-fs-only
	mkdir -p ./target/release/lib
	mkdir -p ./target/release/tools
	cp --recursive ./lib/* ./target/release/lib/
	cp --recursive ./tools/* ./target/release/tools/
	cp ./externals/sqlite/System.Data.SQLite.dll ./target/release/
	cp ./externals/sqlite/SQLite.Interop.dll ./target/release/
	cp ./externals/eagle/bin/DebugNetStandard21/bin/netcoreapp3.0/EagleShell.runtimeconfig.json ./target/release/
	cp ./externals/eagle/bin/DebugNetStandard21/bin/netcoreapp3.0/Eagle.dll ./target/release/
	cp ./externals/eagle/bin/DebugNetStandard21/bin/netcoreapp3.0/EagleShell.dll ./target/release/

clean-customize-fs:
	rm -f ./tools/data/fileSystem.db 2>/dev/null || true
	rm -rf ./tools/data/fs.in 2>/dev/null || true
	rm -rf ./tools/data/fs.out 2>/dev/null || true

test-customize-fs-only: clone-and-build-sds clone-and-build-eagle clean-customize-fs
	./tools/data/create_fileSystem_data.sh
	gpg --batch --yes --pinentry-mode loopback --passphrase-file ./tools/data/keys/passphrase.txt --import ./tools/data/keys/test-private.asc
	dotnet exec --roll-forward Major ./externals/eagle/bin/DebugNetStandard21/bin/netcoreapp3.0/EagleShell.dll -file ./tools/buildDeploymentDatabase.eagle ./tools/data/fileSystem.db ./tools/data/fs.in

build-drivers:
	cd ./kernel/drivers/user_entropy && make build-standalone

copy-env-release:
	mkdir -p ./target/release/
	cp ./scripts/single-node/dotenv/private.env ./target/release/
	cp ./scripts/single-node/dotenv/public.env ./target/release/

copy-env-debug:
	mkdir -p ./target/debug/
	cp ./scripts/single-node/dotenv/private.env ./target/debug/
	cp ./scripts/single-node/dotenv/public.env ./target/debug/

build-release-only:
	cargo build --release

build-release: build-drivers test-install-release build-release-only copy-env-release

build-debug-only:
	cargo build

build-debug: build-drivers test-install-debug build-debug-only copy-env-debug

build-only: build-release-only copy-env-release

build: build-release

run-debug: build-drivers test-install-debug
	cd ./target/debug && cargo run

run-release: build-drivers test-install-release
	cd ./target/release && cargo run --release

run: run-release

build-and-run: build run

orch-test:
	sudo -E $(which cargo) nextest r

orch-run:
	cargo build -p orchestrator
	cd crates/orchestrator && sudo ../../target/debug/orchestrator

orch-docker-build:
	sudo docker build -f crates/orchestrator/Dockerfile -t orchestrator .

orch-docker-run: orch-docker-build
	@sh ./scripts/orchestrator-docker-run.sh

orch-docker-check:
	@sh ./scripts/orchestrator-docker-check.sh


test-deploy-fs: clone-and-build-sds clone-and-build-eagle
	dotnet exec --roll-forward Major ./externals/eagle/bin/DebugNetStandard21/bin/netcoreapp3.0/EagleShell.dll -file ./tools/deployToFileSystem.eagle ./tools/data/fileSystem.db ./tools/data/fs.out

test-shell: clone-and-build-sds clone-and-build-eagle
	EAGLE_TEST_TEMP=/tmp dotnet exec --roll-forward Major ./externals/eagle/bin/DebugNetStandard21/bin/netcoreapp3.0/EagleShell.dll -initialize -postInitialize "package require Chelsea.Meta"

test-only-preclean:
	rm -rf ./target/release/.env 2>/dev/null || true
	rm -rf ./target/debug/.env 2>/dev/null || true

test-only: test-only-preclean
	EAGLE_TEST_TEMP=/tmp dotnet exec --roll-forward Major ./externals/eagle/bin/DebugNetStandard21/bin/netcoreapp3.0/EagleShell.dll -file ./tests/all.eagle

test-only-quick: test-only-preclean
	EAGLE_TEST_TEMP=/tmp dotnet exec --roll-forward Major ./externals/eagle/bin/DebugNetStandard21/bin/netcoreapp3.0/EagleShell.dll -file ./tests/all.eagle -stopOnFailure true -skip "chelsea-103.0 chelsea-104.0 chelsea-105.0 chelsea-2??.0 chelsea-407.0 domains-1??.0"

test-only-quick-pool: test-only-preclean
	CHELSEA_VM_POOL=3 EAGLE_TEST_TEMP=/tmp dotnet exec --roll-forward Major ./externals/eagle/bin/DebugNetStandard21/bin/netcoreapp3.0/EagleShell.dll -file ./tests/all.eagle -stopOnFailure true -skip "chelsea-103.0 chelsea-104.0 chelsea-105.0 chelsea-2??.0 chelsea-407.0 domains-1??.0"

test-only-parallel: test-only-preclean
	./scripts/test-parallel.sh

test: clean-drivers build-drivers test-install-release build-release-only clone-and-build-sds clone-and-build-eagle test-customize-fs-only
	$(MAKE) --no-print-directory PARENT_MAKE_TARGET=$@ test-only

test-firecracker: clean-drivers build-drivers-firecracker test-install-release build-release-only clone-and-build-sds clone-and-build-eagle clone-and-build-firecracker-rootfs test-customize-fs-only
	$(MAKE) --no-print-directory PARENT_MAKE_TARGET=$@ test-only

stop:
	./commands.sh stop

install: build-release clone-and-build-sds clone-and-build-eagle
	mkdir -p /usr/local/chelsea/bin
	mkdir -p /usr/local/chelsea/tools/data
	cp --update=none ./tools/data/*.lst /usr/local/chelsea/tools/data/
	cp ./tools/*.eagle /usr/local/chelsea/tools/
	cp --update=none ./.env.example /usr/local/chelsea/bin/.env
	ls /usr/local/bin/.env || ln -s /usr/local/chelsea/bin/.env /usr/local/bin/.env
	cp ./target/release/chelsea /usr/local/chelsea/bin/
	test -f /usr/local/bin/chelsea || ln -s /usr/local/chelsea/bin/chelsea /usr/local/bin/chelsea
	cp ./externals/sqlite/System.Data.SQLite.dll /usr/local/chelsea/bin/
	cp ./externals/sqlite/SQLite.Interop.dll /usr/local/chelsea/bin/
	cp --update=none ./externals/eagle/bin/DebugNetStandard21/bin/netcoreapp3.0/EagleShell.runtimeconfig.json /usr/local/chelsea/bin/
	cp ./externals/eagle/bin/DebugNetStandard21/bin/netcoreapp3.0/Eagle.dll /usr/local/chelsea/bin/
	cp ./externals/eagle/bin/DebugNetStandard21/bin/netcoreapp3.0/EagleShell.dll /usr/local/chelsea/bin/

run-install:
	cd /usr/local/bin && ./chelsea

force-uninstall-shared:
	rm -rf /var/lib/chelsea 2>/dev/null || true

force-uninstall-all: force-uninstall-shared
	rm -f /usr/local/bin/chelsea 2>/dev/null || true
	rm -f /usr/local/bin/.env 2>/dev/null || true
	rm -rf /usr/local/chelsea 2>/dev/null || true

api-docs:
	cargo run -p orchestrator --bin openapi

chelsea-docs:
	./crates/chelsea/docs/build-docs.tcl

chelsea-api-docs:
	cargo run -p chelsea_server2 --bin chelsea_server_openapi

docs: api-docs chelsea-docs chelsea-api-docs

clean:
	cargo clean

metrics-server-test:
	bun ./test_server/index.ts

clean-drivers:
	cd ./kernel/drivers/user_entropy && (rm -f Module.symvers modules.order .*.cmd *.cmd *.ko *.mod *.mod.c *.o .*.o 2>/dev/null || true)

clean-externals:
	rm -rf ./externals/eagle 2>/dev/null || true
	rm -rf ./externals/firecracker 2>/dev/null || true
	rm -rf ./externals/kernel 2>/dev/null || true
	rm -rf ./externals/rootfs 2>/dev/null || true
	rm -rf ./externals/sds 2>/dev/null || true
	rm -rf ./externals/sqlite 2>/dev/null || true

clean-docker:
	mv /etc/docker/daemon.json /etc/docker/daemon.json.chelsea_backup_$(shell echo $$$$) 2>/dev/null || true

clean-resources:
	./commands.sh cleanup

clean-test: clean-customize-fs
	rm -rf /tmp/chelseasnap_* 2>/dev/null || true
	rm -rf /tmp/etd_* 2>/dev/null || true
	rm -rf /tmp/.dotnet 2>/dev/null || true
	rm -rf /tmp/MSBuildTemp${USER}
	rm -rf /tmp/NuGetScratch${USER}
	rm -rf /var/run/user/$(shell id --user)/tmp/etd_* 2>/dev/null || true
	rmdir /var/run/user/$(shell id --user)/tmp 2>/dev/null || true

clean-tmp-and-test-logs:
	rm -rf /tmp/* 2>/dev/null || true
	rm -rf /var/run/user/$(shell id --user)/logs 2>/dev/null || true

super-clean: terminate clean clean-drivers clean-resources clean-test

super-duper-clean: super-clean clean-externals clean-docker

all-clean: super-duper-clean clean-tmp-and-test-logs

chelsea-docker-build:
	sudo DOCKER_BUILDKIT=1 docker build -f crates/chelsea/Dockerfile -t chelsea .

chelsea-docker-run:
	sudo docker run --privileged \
		--pid=host \
		--network=host \
		--env-file crates/chelsea/.env \
		-e RUST_LOG='chelsea=trace,orch_wg=trace' \
		-v /etc/ceph:/etc/ceph:ro \
		-v /lib/modules:/lib/modules:ro \
		-v /dev:/dev \
		-v /var/lib/chelsea:/var/lib/chelsea \
		-v /usr/local/bin/firecracker:/usr/local/bin/firecracker:ro \
		-v /usr/local/bin/jailer:/usr/local/bin/jailer:ro \
		--mount type=bind,source=/run/udev,target=/run/udev,bind-propagation=shared \
		--mount type=bind,source=/var/run/netns,target=/var/run/netns,bind-propagation=shared \
		chelsea
