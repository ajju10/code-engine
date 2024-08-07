FROM rust:1.79 as builder
WORKDIR /usr/src/code-engine
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo "fn main() {}" > src/main.rs
RUN cargo build --release
RUN rm -rf src
COPY . .
RUN cargo build --release

FROM ubuntu:22.04
RUN apt-get update && apt-get install -y \
    libssl-dev \
    g++ \
    && rm -rf /var/lib/apt/lists/* \

# Create the user
RUN useradd -ms /bin/bash runner
RUN mkdir -p /home/runner/workdir && chown -R runner:runner /home/runner
USER runner
WORKDIR /home/runner/workdir
COPY --from=builder /usr/src/code-engine/target/release/code-engine .
EXPOSE 4000
CMD ["./code-engine"]
