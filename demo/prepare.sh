#!/bin/bash
# Off-camera prep for demo.tape: seed the sandbox HOME and put a `cachewipe`
# shim on PATH so the recorded commands read exactly as a user would type them.
set -eu
cd "$(dirname "$0")/.."

rm -rf /tmp/cwdemo
./demo/setup-demo.sh /tmp/cwdemo/home >/dev/null

mkdir -p /tmp/cwdemo-bin
cat > /tmp/cwdemo-bin/cachewipe <<EOF
#!/bin/bash
exec "$PWD/target/release/cachewipe" "\$@"
EOF
chmod +x /tmp/cwdemo-bin/cachewipe
