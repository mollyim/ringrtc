# Copyright 2026 Molly Instant Messenger
# SPDX-License-Identifier: AGPL-3.0-only
#
# Docker builder entrypoint.
#
# This Makefile is the entrypoint of the Docker builder image. It exposes the
# same stable set of build targets across platforms; the platform-specific
# commands live in mk/<platform>.mk and are selected with TARGET_PLATFORM.
#
# It replaces the upstream Makefile, which is archived as Makefile-orig.

TARGET_PLATFORM ?= android

ifeq ($(TARGET_PLATFORM),android)
include mk/android.mk
else
$(error Unsupported TARGET_PLATFORM='$(TARGET_PLATFORM)')
endif

.DEFAULT_GOAL := help
.PHONY: help assemble test publish clean

# In "make help", variables are self-documented with a "## text" comment
# on the preceding line.
help:
	@echo "Docker builder entrypoint for platform: $(TARGET_PLATFORM)"
	@echo
	@echo "Targets:"
	@echo "  help       Show this help"
	@echo "  assemble   Compile and install artifacts for export"
	@echo "  test       Run tests"
	@echo "  publish    Publish artifacts"
	@echo "  clean      Remove build artifacts"
	@echo
	@echo "Variables:"
	@awk ' \
		/^## / { desc = substr($$0, 4); next } \
		desc { printf "  %-18s %s\n", $$1, desc; desc = "" } \
	' $(MAKEFILE_LIST)

assemble:
	$(do_assemble)

test:
	$(do_test)

publish:
	$(do_publish)

clean:
	$(do_clean)
