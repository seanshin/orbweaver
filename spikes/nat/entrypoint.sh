#!/bin/sh
# Container entrypoint for the R7 probe — UNRUN. See run.sh.
#
# Three modes, matching the three things the probe has to show:
#
#   naive      bind to this container's own address and publish it. This is
#              what an ORB does by default, and it is assumption D's finding:
#              the address is real, routable *here*, and useless to a client
#              in another routing domain.
#   published  bind wide and publish through ORBWEAVER_PUBLISH_MAP. Refuses to
#              publish at all if the map is missing, because 0.0.0.0 is
#              bindable and unpublishable.
#   call       dial whatever the reference says.
set -eu

case "${NAT_MODE:-}" in
  naive)
    own=$(hostname -i | awk '{print $1}')
    echo "naive publish: binding and publishing ${own}:5555"
    exec spike-nat serve "${own}:5555" /shared/server.ior
    ;;
  published)
    echo "published: binding 0.0.0.0:5555, ORBWEAVER_PUBLISH_MAP=${ORBWEAVER_PUBLISH_MAP:-<unset>}"
    exec spike-nat serve 0.0.0.0:5555 /shared/server.ior
    ;;
  call)
    exec spike-nat call /shared/server.ior
    ;;
  *)
    echo "set NAT_MODE to naive, published or call" >&2
    exit 2
    ;;
esac
