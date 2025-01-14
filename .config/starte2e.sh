#!/bin/sh

set -eu

# Make sure the containers can write some files that need to be shared
mkdir -p /tmp/zitadel-docker-test/
touch /tmp/zitadel-docker-test/service-account.json
chmod a+rw /tmp/zitadel-docker-test/service-account.json

# Shut down any still running test-setup first
docker compose down -v || true
docker compose up -d
