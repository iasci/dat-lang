#!/bin/bash
# bench_dispatch.sh K M : compares dyn vs enum lowering
set -e
cd "$(dirname "$0")/.."
export RUSTUP_TOOLCHAIN=1.99.0 RUSTFLAGS=-Awarnings
K=$1; M=$2
for mode in dyn enum; do
  d=disp_${mode}_${K}_${M}
  rm -rf $d
  python3 -I scripts/gen_dispatch.py $d $K $M $mode
  cd $d
  t0=$(python3 -c 'import time;print(time.time())')
  cargo build -q
  t1=$(python3 -c 'import time;print(time.time())')
  cargo build -q --release
  t2=$(python3 -c 'import time;print(time.time())')
  dsz=$(stat -f %z target/debug/disp); rsz=$(stat -f %z target/release/disp)
  # incremental: add one more implementor (regenerate with K+1, only shapes.rs + main.rs change)
  python3 -I ../scripts/gen_dispatch.py . $((K+1)) $M $mode
  t3=$(python3 -c 'import time;print(time.time())')
  cargo build -q
  t4=$(python3 -c 'import time;print(time.time())')
  ./target/release/disp 1 >/dev/null
  run=$(./target/release/disp 2000000)
  python3 -c "print('mode=$mode K=$K M=$M  debug_clean=%.2fs release_clean=%.2fs  debug_bin=%.2fMB release_bin=%.2fMB  add_implementor_rebuild=%.2fs' % ($t1-$t0, $t2-$t1, $dsz/1e6, $rsz/1e6, $t4-$t3))"
  echo "   release run (2M shapes): $run"
  cd ..
done
