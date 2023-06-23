```bash
docker build -t gnu-alsa .
cross build --bin server --release --target=x86_64-unknown-linux-gnu
cross build --bin discurse --release --target=x86_64-unknown-linux-gnu
```