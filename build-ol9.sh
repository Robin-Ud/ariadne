#!/bin/bash
# Build de release pra VM (Oracle Linux 9, glibc 2.34) dentro de um container oraclelinux:9, como o do
# argos. Monta a pasta de cima: o `argos = { path = "../argos" }` tem que estar visível no container.
# Saída: target/ol9/release/ariadne. Toolchain e registry nos mesmos volumes do argos (cache).
set -euo pipefail
DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PAI="$(dirname "$DIR")"
NOME="$(basename "$DIR")"
[ -d "$PAI/argos" ] || { echo "falta $PAI/argos (a biblioteca)"; exit 1; }
# imagem local com gcc já instalado (criada pro build do argos); sem ela, a oraclelinux:9 limpa
IMG=localhost/argos-ol9-build
podman image exists "$IMG" || IMG=docker.io/library/oraclelinux:9
podman run --rm --security-opt label=disable \
    -v "$PAI":/src -w "/src/$NOME" \
    -v argos-rustup:/root/.rustup -v argos-cargo:/root/.cargo \
    "$IMG" bash -c '
        set -e
        command -v gcc >/dev/null || dnf install -y -q gcc >/dev/null
        if [ ! -x /root/.cargo/bin/cargo ]; then
            curl -sSf https://sh.rustup.rs | sh -s -- -y -q --profile minimal
        fi
        /root/.cargo/bin/cargo build --release --target-dir target/ol9
        ldd --version | head -1
    '
ls -la "$DIR/target/ol9/release/ariadne"
objdump -T "$DIR/target/ol9/release/ariadne" | grep -o "GLIBC_[0-9.]*" | sort -uV | tail -1
