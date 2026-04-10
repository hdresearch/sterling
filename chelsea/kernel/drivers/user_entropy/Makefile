CFLAGS_REMOVE_user_entropy.o += $(CC_FLAGS_FTRACE) -pg -mfentry -mnop-mcount -finstrument-functions
AFLAGS_REMOVE_user_entropy.o += $(CC_FLAGS_FTRACE) -pg -mfentry -mnop-mcount

KBUILD_CFLAGS := $(filter-out -pg -mfentry -mnop-mcount -finstrument-functions -fprofile-arcs -ftest-coverage,$(KBUILD_CFLAGS))
KBUILD_AFLAGS := $(filter-out -pg -mfentry -mnop-mcount,$(KBUILD_AFLAGS))

obj-m += user_entropy.o

build-standalone:
	$(MAKE) -C /usr/lib/modules/$(shell uname -r)/build M=$(PWD) modules

build-firecracker:
	$(MAKE) -C ../../../externals/firecracker/resources/linux modules_prepare
	$(MAKE) -C ../../../externals/firecracker/resources/linux M=$(PWD) modules

clean-standalone:
	$(MAKE) -C /usr/lib/modules/$(shell uname -r)/build M=$(PWD) clean

clean-firecracker:
	$(MAKE) -C ../../../externals/firecracker/resources/linux M=$(PWD) clean

all: build-standalone build-firecracker

clean: clean-standalone clean-firecracker
