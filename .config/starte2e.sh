#!/bin/sh

# SPDX-FileCopyrightText: 2025 Famedly GmbH (info@famedly.com)
#
# SPDX-License-Identifier: Apache-2.0

set -eu

# Make sure the containers can write some files that need to be shared
mkdir -p docker/zitadel
touch docker/zitadel/service-account.json
chmod a+rw docker/zitadel/service-account.json

# Shut down any still running test-setup first
docker compose down -v --remove-orphans || true
docker compose up -d --quiet-pull --wait
