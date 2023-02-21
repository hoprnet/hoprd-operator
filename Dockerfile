ARG RUST_IMAGE=${RUST_IMAGE:-rust:1.67}

FROM ${RUST_IMAGE} as build

# shell project to cache dependencies
RUN USER=root cargo new --bin hopr_operator
WORKDIR /hopr_operator

COPY ./Cargo.lock ./Cargo.lock
COPY ./Cargo.toml ./Cargo.toml

RUN cargo build --release
RUN rm src/*.rs

# build project sources
COPY ./src ./src

RUN rm ./target/release/deps/hopr_operator*
RUN cargo build --release

FROM scratch
LABEL name="hoprd operator" \
      maintainer="tech@hoprnet.org" \
      vendor="HOPR" \
      summary="Operator managing hoprd instances" \
      description="Automation to introduce a hoprd network into a Kubernetes cluster using a dedicated operator"
COPY --from=build /hopr_operator/target/release/hopr_operator .

CMD ["./hopr_operator"]

# Build Image command
# docker build -t gcr.io/hoprassociation/hopr-operator:latest .
