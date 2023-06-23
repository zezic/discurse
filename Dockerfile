FROM ghcr.io/cross-rs/x86_64-unknown-linux-gnu:latest
RUN apt update
RUN apt -y install libasound2-dev libdbus-1-dev libjack-jackd2-dev