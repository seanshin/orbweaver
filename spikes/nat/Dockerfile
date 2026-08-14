# Container probe for risk R7 — UNRUN. See run.sh.
#
# Builds only `spike-nat` out of the workspace. Nothing here installs an ORB
# fixture: omniORB, TAO and JacORB are LGPL/GPL/DOC and an image containing
# them may not be published (PLAN §10). This image contains our code and the
# Rust toolchain's, so it could be published — and still is not, because
# nothing in this probe needs a registry.
FROM rust:1.85-slim AS build
WORKDIR /src
# The whole workspace is copied because cargo needs every member's manifest to
# resolve the workspace, even to build one binary out of it.
COPY . .
RUN cargo build --release -p orbweaver-giop --bin spike-nat

FROM debian:bookworm-slim
# `hostname -i` prints the container's own address, which is the address a
# naive ORB publishes and the whole subject of this probe.
RUN apt-get update && apt-get install -y --no-install-recommends hostname \
    && rm -rf /var/lib/apt/lists/*
COPY --from=build /src/target/release/spike-nat /usr/local/bin/spike-nat
COPY spikes/nat/entrypoint.sh /usr/local/bin/entrypoint.sh
RUN chmod +x /usr/local/bin/entrypoint.sh
ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]
