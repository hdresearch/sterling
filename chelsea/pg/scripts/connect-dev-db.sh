#!/bin/bash

set -eou pipefail

POSTGRES_PASSWORD=opensesame
POSTGRES_USER=postgres
POSTGRES_DB=vers
PG=postgresql://${POSTGRES_USER}:${POSTGRES_PASSWORD}@127.0.0.1:5432/${POSTGRES_DB}?sslmode=disable

psql $PG