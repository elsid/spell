#!/bin/bash -e

docker run --rm --tty --interactive --user "$(id -u)":"$(id -g)" --volume "${PWD}":/code --workdir /code rust:1.51 \
  cargo build --release --features=server --target-dir=docker_target
