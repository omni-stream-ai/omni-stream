.DEFAULT_GOAL := run

ADDR ?= 127.0.0.1:4317

.PHONY: run
run:
	cargo run -p omnistream -- serve "$(ADDR)"
