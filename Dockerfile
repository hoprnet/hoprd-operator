ARG RUST_IMAGE=${RUST_IMAGE:-rust:1.68}

FROM ${RUST_IMAGE} as builder

# musl toolchain for static binaries
RUN apt update && apt install -y pkg-config libssl-dev musl-tools
ENV SYSROOT=/dummy
ENV OPENSSL_STATIC=1
ENV OPENSSL_INCLUDE_DIR=/usr/include/openssl

# build project sources
RUN mkdir -p /hoprd_operator
WORKDIR /hoprd_operator

COPY ./Cargo.lock ./Cargo.lock
COPY ./Cargo.toml ./Cargo.toml
COPY ./src ./src

RUN rustup target install $(uname -m)-unknown-linux-musl
RUN OPENSSL_LIB_DIR=/usr/lib/$(uname -m)-linux-gnu RUSTFLAGS="-C target-feature=+crt-static" cargo build --release --target $(uname -m)-unknown-linux-musl --features vendored
RUN mv target/$(uname -m)-unknown-linux-musl/release/hoprd_operator target/



FROM scratch

LABEL name="hoprd operator" \
      maintainer="tech@hoprnet.org" \
      vendor="HOPR" \
      summary="Operator managing hoprd instances" \
      description="Automation to introduce a hoprd network into a Kubernetes cluster using a dedicated operator"
COPY --from=builder /hoprd_operator/target/hoprd_operator /bin/hoprd_operator

ENV OPERATOR_ENVIRONMENT=production
ENV RUST_BACKTRACE=full
ENTRYPOINT ["/bin/hoprd_operator"]
