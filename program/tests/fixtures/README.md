# Test Fixtures
## Sereum DEX
Must be named `serum_dex.so`.
```
# Be in serum-dex/dex with the desired version of serum-dex source code
cargo build-bpf
cp target/deploy/serum_dex.so mango/program/tests/fixtures/serum_dex.so
```