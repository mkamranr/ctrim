# Build
FROM rust:1-alpine AS build
RUN apk add --no-cache musl-dev
WORKDIR /src

# Cache dependencies separately from the source.
COPY Cargo.toml Cargo.lock ./
RUN mkdir -p src benches examples \
    && echo 'fn main() {}' > src/main.rs \
    && echo '' > src/lib.rs \
    && echo 'fn main() {}' > benches/throughput.rs \
    && echo 'fn main() {}' > examples/calibrate.rs \
    && cargo build --release --bin ctrim 2>/dev/null || true
COPY . .
RUN touch src/main.rs src/lib.rs && cargo build --release --bin ctrim

# Run
FROM alpine:3
RUN adduser -D -u 10001 ctrim
COPY --from=build /src/target/release/ctrim /usr/local/bin/ctrim
USER ctrim
ENTRYPOINT ["ctrim"]
