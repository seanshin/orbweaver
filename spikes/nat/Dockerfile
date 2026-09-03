# Container probe for risk R7 — UNRUN. See run.sh.
#
# Nothing is compiled here. The image is a base system plus ONE binary copied
# in from the build context: the debug `spike-nat` that `spikes/nat_rewrite.sh`
# builds on the host before this probe runs. The first draft of this file
# compiled the workspace inside the image, cold, on every run — and the probe's
# claim is about *routing*: which address a servant publishes and whether a
# client in another routing domain can dial it. A binary built on the host
# measures that identically, and a toolchain in the image measures nothing the
# probe is about (docs/PLAN-NAT-PROBE.md §3, lane B).
#
# The consequence, stated so that run.sh can check it: the copied binary must
# be executable on the image's platform. compose.yaml pins that platform and
# run.sh refuses — exit 2, a sentence, no image — when the host's `uname -m`
# does not match it or the file at the path below is not an ELF for it.
#
# Nothing here installs an ORB fixture: omniORB, TAO and JacORB are
# LGPL/GPL/DOC and an image containing them may not be published (PLAN §10).
# This image contains only our code and Debian's, so it could be published —
# and still is not, because nothing in this probe needs a registry.
#
# The build context is the repository root (compose.yaml: `context: ../..`),
# filtered by the root `.dockerignore`, which run.sh derives from `.gitignore`
# before every build. `target/` is ignored there; the single path below is the
# one exception the derivation re-includes.
FROM debian:bookworm-slim
# `hostname -i` prints the container's own address, which is the address a
# naive ORB publishes and the whole subject of this probe.
RUN apt-get update && apt-get install -y --no-install-recommends hostname \
    && rm -rf /var/lib/apt/lists/*
COPY target/debug/spike-nat /usr/local/bin/spike-nat
COPY spikes/nat/entrypoint.sh /usr/local/bin/entrypoint.sh
RUN chmod +x /usr/local/bin/entrypoint.sh /usr/local/bin/spike-nat
ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]
