FROM docker.io/ubuntu:24.04

COPY target/release/shard_01 /usr/local/bin/shard_01

RUN chmod 0755 /usr/local/bin/shard_01
