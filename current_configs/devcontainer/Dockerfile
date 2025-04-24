FROM espressif/idf-rust:esp32c6_latest

USER root

RUN apt-get update && apt-get install -y libudev-dev gdb
RUN groupmod -g 18 dialout && usermod -aG dialout esp
RUN groupmod -g 964 plugdev && usermod -aG plugdev esp

USER esp

# Theres a bug in the current release
RUN cargo install espflash --git https://github.com/esp-rs/espflash --force
RUN cargo install probe-rs-tools --locked




# ARG VARIANT=bookworm-slim
# FROM debian:${VARIANT}
# ENV DEBIAN_FRONTEND=noninteractive
# ENV LC_ALL=C.UTF-8
# ENV LANG=C.UTF-8

# RUN apt-get update \
#     && apt-get install -y git wget flex bison gperf python3 python3-pip python3-venv cmake ninja-build ccache libffi-dev libssl-dev dfu-util libusb-1.0-0\
#     && apt-get clean -y && rm -rf /var/lib/apt/lists/* /tmp/library-scripts


# RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# ENV PATH=${PATH}:${HOME}/.cargo/bin

# RUN rustup toolchain install nightly --component rust-src

# # for ESP32-C6 no_std
# RUN rustup target add riscv32imac-unknown-none-elf

# RUN cargo install ldproxy

