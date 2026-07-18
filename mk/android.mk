# Copyright 2026 Molly Instant Messenger
# SPDX-License-Identifier: AGPL-3.0-only

## CPU architectures to build [default: arm arm64 x64]
ARCHS ?= arm arm64 x64

## Parallel build jobs [default: CPU count]
JOBS ?= $(shell nproc)

## Extra arguments forwarded to build-aar
BUILD_AAR_ARGS ?=

BUILD_AAR := ./bin/build-aar

do_assemble = $(strip $(BUILD_AAR) --arch $(ARCHS) --jobs $(JOBS) --install-local $(BUILD_AAR_ARGS))
do_test     = cd src/rust && ./scripts/run-tests
do_stage    = $(strip $(BUILD_AAR) --arch $(ARCHS) --jobs $(JOBS) --publish $(BUILD_AAR_ARGS))
do_clean    = $(BUILD_AAR) --clean
