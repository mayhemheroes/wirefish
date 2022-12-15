FROM ghcr.io/evanrichter/cargo-fuzz as builder

ADD . /wirefish
WORKDIR /wirefish/src-tauri/fuzz
RUN cargo +nightly fuzz build 

FROM debian:bookworm
COPY --from=builder /wirefish/src-tauri/fuzz/target/x86_64-unknown-linux-gnu/release/wirefish-fuzzer /