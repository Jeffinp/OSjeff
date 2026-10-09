# W18 proof (d): `net_http_get` of an app through the real transport (fetch + netd).
# Prepare (needs the wasm32 target; the fixture is NOT bundled):
#   (cd wasm-apps/nettest && cargo build --release --target wasm32-unknown-unknown)
#   truncate -s 64M fs.img
#   cargo run --release -p kitsune_core --example fs3_inject -- fs.img \
#       wasm-apps/nettest/target/wasm32-unknown-unknown/release/nettest.wasm /nettest.wasm
#   tools/nettest-server.py &      # the fake public server (HTTP :8077, self-signed HTTPS :8078)
#   QEMU_NETDEV="user,id=n0,net=203.0.113.0/24,host=203.0.113.5,dhcpstart=203.0.113.15,dns=203.0.113.3" \
#     FS_IMG=fs.img tools/perf/run.sh <img> bios <out> 150 tools/perf/scen/w18-net.sh
# The file manager opens /nettest.wasm (installs and runs it); keys 1..9 and 0 then issue the
# requests of wasm-apps/nettest, 2.5 s apart (an app may send one request per second).
source "$(dirname "$0")/../lib.sh"
wait_first_frame
dock 829; click; sleep 2
key end; key ret; sleep 3                                 # the last row is /nettest.wasm (folders come first)
shot n0-app
for k in 1 2 3 4 5 6 7 9 8 0; do key $k; sleep 3; shot n$k-key; done
sleep 8
shot nz-end
finish
