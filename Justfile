set dotenv-load

default:
    just --list

server:
    cargo run -p monitor-server

cli *args:
    cargo run -p monitor-cli -- {{args}}

check:
    cargo check --workspace

test:
    cargo test --workspace

fmt:
    cargo fmt --all
