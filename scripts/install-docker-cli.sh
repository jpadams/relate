#!/usr/bin/env bash
# Install the static Docker CLI into the current container (the gosonic
# stages that must drive the HOST daemon through its socket: the runner
# image has no docker binary). One canonical copy — the stages that need
# it (loadtest-baselines, query-latency) both call this script.
set -euo pipefail

# The pinned digests (27.5.1): an unsigned tarball from a CDN is
# verified, never trusted. Bump procedure: set the version in the URL,
# then for each arch
#   curl -fsSL -o docker.tgz https://download.docker.com/linux/static/stable/<arch>/docker-<ver>.tgz
#   shasum -a 256 docker.tgz
# and update the digest beside it below (Docker publishes no .sha256
# sidecar, so the digests are computed by hand at bump time).
case "$(uname -m)" in
  x86_64)
    docker_arch=x86_64
    pinned=4f798b3ee1e0140eab5bf30b0edc4e84f4cdb53255a429dc3bbae9524845d640
    ;;
  aarch64|arm64)
    docker_arch=aarch64
    pinned=e6b53725a73763ab3f988c73f8772eaed429754c1a579db5ff11f21990fd1817
    ;;
  *) echo "install-docker-cli: unsupported architecture: $(uname -m)" >&2; exit 1 ;;
esac
curl -fsSL -o /tmp/docker.tgz "https://download.docker.com/linux/static/stable/${docker_arch}/docker-27.5.1.tgz"
echo "${pinned}  /tmp/docker.tgz" | sha256sum --check --quiet
tar -xzf /tmp/docker.tgz -C /tmp
mv /tmp/docker/docker /usr/local/bin/docker
