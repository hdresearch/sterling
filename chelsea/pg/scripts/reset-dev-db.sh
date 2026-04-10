#!/bin/bash

set -eou pipefail

cd "$(dirname "$0")/.."

nuke() {
    export POSTGRES_PASSWORD=opensesame
    export POSTGRES_USER=postgres
    export POSTGRES_DB=vers
    sudo docker-compose down --volumes
}

interactive() {
    echo 'This script wipes the DB, readying it to be rebuilt with ./setup-dev-db.sh'
    echo ""

    while true; do
        read -p "$*[y/n]: " yn
        case $yn in
            [Nn]*) echo "Aborting"; return 1 ;;
            [Yy]*) nuke; break ;;
        esac
    done
}

if [ $# -eq 0 ]; then
    interactive
elif [ "$1" = '-y' ]; then
    nuke
else
    echo "Aborting"
    exit 1
fi
