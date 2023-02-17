ARG RUST_IMAGE=${RUST_IMAGE:-rust:1.67}

# 1. Create the build container to compile the operator
FROM ${RUST_IMAGE} as build

# 2. Create a new empty shell project
RUN USER=root cargo new --bin hopr_operator
WORKDIR /hopr_operator

# 3. Copy over your manifests
COPY ./Cargo.lock ./Cargo.lock
COPY ./Cargo.toml ./Cargo.toml

# 4. This build step will cache your dependencies
RUN cargo build --release
RUN rm src/*.rs

# 5. Copy your source tree
COPY ./src ./src

# 6. Build for release
RUN rm ./target/release/deps/hopr_operator*
RUN cargo build --release

# our final base
FROM debian:11.6-slim

# copy the build artifact from the build stage
COPY --from=build /hopr_operator/target/release/hopr_operator .

# set the startup command to run your binary
CMD ["./hopr_operator"]

# Build Image command
# docker build -t gcr.io/hoprassociation/hopr-operator:latest .