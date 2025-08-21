# Bonsai Local

A REST API server that provides an API-compatible alternative to RISC Zero's Bonsai proving service. By pointing Bonsai client applications to this local server's URL instead of the actual Bonsai service endpoint, developers can run and test their Bonsai-dependent applications entirely offline. The server implements the same REST API as Bonsai, ensuring seamless compatibility with existing client code.

## Prerequisites

- Docker (required for stark-to-snark proving)
- r0vm - Install via rzup: `rzup install cargo-risczero 1.2.6` (or your preferred version)
- CUDA (optional, required for `cuda` feature)

## Installation

```bash
git clone https://github.com/datachainlab/bonsai-local.git
cd bonsai-local
cargo build --release
```

If you enable the `cuda` feature, you will need to install CUDA.

Then, build the server:

```bash
cargo build --release --features cuda
```

## Usage

```bash
Usage: bonsai-local [OPTIONS] <URL>

Arguments:
  <URL>  Server URL (must be http:// or https://)

Options:
      --listen-address <ADDRESS>    Address to listen on (e.g., "127.0.0.1:8080", "0.0.0.0:8080") [default: 127.0.0.1:8080]
      --ttl <SECONDS>               Time-to-live for cached entries in seconds (default: 14400 = 4 hours) [default: 14400]
      --channel-buffer-size <SIZE>  Channel buffer size for prover queue [default: 8]
      --r0vm-version <VERSION>      Required r0vm version (format: <major>.<minor>, e.g., "1.0", "1.2")
  -h, --help                        Print help
```

Start the server with a URL that will be returned to clients:

```bash
bonsai-local http://localhost:8080
```

## License

Licensed under the Apache License, Version 2.0. See [LICENSE](LICENSE) for details.

## Acknowledgments

This project is built to be compatible with [RISC Zero](https://github.com/risc0/risc0)'s Bonsai API specification.
