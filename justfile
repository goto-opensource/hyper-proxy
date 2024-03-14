# list all just commands
default:
  just --list

cargo_all *ARGS:
  cargo {{ ARGS }} --no-default-features --features=tls
  cargo {{ ARGS }} --no-default-features --features=rustls
  cargo {{ ARGS }} --no-default-features --features=rustls-webpki
  cargo {{ ARGS }} --no-default-features --features=openssl-tls

# test all sane feature-combinations
test_all: (cargo_all "test")

# clippy all sane feature-combinations
clippy_all: (cargo_all "clippy")
