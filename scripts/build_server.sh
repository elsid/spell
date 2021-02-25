#!/bin/bash -e

docker run --rm --tty --interactive --user "$(id -u)":"$(id -g)" --volume "${PWD}":/code --workdir /code rust:1.50 \
  cargo build --release --target-dir=docker_target
