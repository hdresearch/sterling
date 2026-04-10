#!/bin/bash

set -eou pipefail

cd "$(dirname "$0")/.."

if ! command -v dbmate >/dev/null 2>&1
then
    echo "You need dbmate installed"
    echo "https://github.com/amacneil/dbmate"
    exit 1
fi

if ! command -v docker >/dev/null 2>&1
then
    echo "Please install docker for local development"
    exit 1
fi

sudo docker-compose up -d && sleep 2 # Need just a bit for pg to settle

POSTGRES_PASSWORD=opensesame
POSTGRES_USER=postgres
POSTGRES_DB=vers
PG=postgresql://${POSTGRES_USER}:${POSTGRES_PASSWORD}@127.0.0.1:5432/${POSTGRES_DB}?sslmode=disable

dbmate --url $PG \
       --migrations-dir ./migrations \
       --no-dump-schema \
       up \
       --strict

./scripts/insert-vers-tls-db.sh