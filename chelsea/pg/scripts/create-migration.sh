#!/bin/bash

dbmate \
    --migrations-dir "$(dirname "$0")/../migrations/" \
    new $1
