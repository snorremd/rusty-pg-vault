FROM rust:1.75-slim-bullseye

# Install build dependencies including nettle
RUN apt-get update && apt-get install -y \
    pkg-config \
    libnettle-dev \
    && rm -rf /var/lib/apt/lists/*

# Create a new empty project
WORKDIR /usr/src/app

# Copy over your manifests
COPY Cargo.toml Cargo.lock ./

# Copy your source code
COPY src ./src

# Build your application
RUN cargo build

# Run the binary
CMD ["cargo", "run"] 