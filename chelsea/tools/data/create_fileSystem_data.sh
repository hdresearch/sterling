#!/bin/bash

scriptdir=`dirname "$BASH_SOURCE"`
uname_r=$1

if [[ -z $uname_r ]]; then
  uname_r=$(uname -r)
fi

pushd "$scriptdir/." || exit 1

mkdir -p fs.in/etc/modules-load.d
echo "user_entropy" > fs.in/etc/modules-load.d/10-user_entropy.conf

mkdir -p "fs.in/usr/lib/modules/${uname_r}/build"
cp "../../kernel/drivers/user_entropy/user_entropy.ko" "fs.in/usr/lib/modules/${uname_r}/build/user_entropy.ko"

popd || exit 1
