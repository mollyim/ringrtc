#
# Copyright 2023 Signal Messenger, LLC
# SPDX-License-Identifier: AGPL-3.0-only
#

# Rebuild the pipewire-pulse module from Ubuntu's PipeWire source with the upstream fix for
# capture stream starvation (in PipeWire 1.6.9, not yet in Ubuntu 26.04).
FROM ubuntu:26.04 AS pipewire-pulse-build

ARG DEBIAN_FRONTEND=noninteractive
RUN sed -i 's/^Types: deb$/Types: deb deb-src/' /etc/apt/sources.list.d/ubuntu.sources \
    && apt-get update \
    && apt-get build-dep -y pipewire

WORKDIR /build
COPY call_sim/docker/ringrtc/pipewire-pulse-capture-starvation.patch .
RUN apt-get source pipewire \
    && cd pipewire-*/ \
    && patch -p1 < ../pipewire-pulse-capture-starvation.patch \
    && meson setup build -Dsession-managers= -Dtests=disabled -Dexamples=disabled \
    && ninja -C build src/modules/libpipewire-module-protocol-pulse.so \
    && cp build/src/modules/libpipewire-module-protocol-pulse.so /build/

FROM ubuntu:26.04

RUN apt-get update \
    && apt-get install -y curl \
        iproute2 \
        iperf \
        iptables \
        iputils-ping \
        protobuf-compiler \
        libpulse-dev \
        linux-perf \
        build-essential \
        pulseaudio-utils \
        libpulse0 \
        pipewire \
        dbus \
        pipewire-pulse \
        wireplumber \
        psmisc \
        dbus-user-session \
    && curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y \
    &&  . ~/.cargo/env \
    && cargo install --features=bin addr2line && cargo install inferno

RUN curl https://raw.githubusercontent.com/torvalds/linux/refs/heads/master/tools/perf/perf-archive.sh > /usr/local/bin/perf-archive && chmod +x /usr/local/bin/perf-archive

ENV XDG_RUNTIME_DIR=/tmp
ENV DBUS_SESSION_BUS_ADDRESS=unix:path=/tmp/bus

COPY --from=pipewire-pulse-build /build/libpipewire-module-protocol-pulse.so /tmp/
RUN mv /tmp/libpipewire-module-protocol-pulse.so \
    "$(dpkg -L libpipewire-0.3-modules | grep '/libpipewire-module-protocol-pulse.so$')"

COPY call_sim/docker/ringrtc/wireplumber-no-hardware.conf /etc/wireplumber/wireplumber.conf.d/90-sim-no-hardware.conf

COPY target/release/call_sim-cli /usr/local/bin/
