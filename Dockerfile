# SPDX-FileCopyrightText: 2025 Famedly GmbH (info@famedly.com)
#
# SPDX-License-Identifier: Apache-2.0

FROM registry.famedly.net/docker-oss/rust-container:nightly AS builder

COPY . /app
WORKDIR /app
RUN cargo auditable build --features cli --release --bins

FROM debian:bookworm-slim AS zitadel-actions-manager
RUN apt update && apt install ca-certificates -y
WORKDIR /opt/zitadel-actions-sync
COPY --from=builder /app/target/release/zitadel-actions-sync /usr/local/bin/zitadel-actions-sync
ENTRYPOINT ["/usr/local/bin/zitadel-actions-sync"]
