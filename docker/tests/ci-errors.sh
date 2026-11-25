#!/usr/bin/env bash
# Failure-path regression test for docker/ci. Runs the real ci script against a
# stubbed `docker` binary in a temporary fixture repo; Docker is not required.
set -euo pipefail
cd "$(dirname "$0")/../.."
repo=$(pwd)

fixture=$(mktemp -d "${TMPDIR:-/tmp}/ci-errors.XXXXXX")
trap 'rm -rf "$fixture"' EXIT
mkdir -p "$fixture/repo/docker" "$fixture/bin"
cp docker/ci "$fixture/repo/docker/ci"

cat > "$fixture/bin/docker" <<'STUB'
#!/usr/bin/env bash
printf '%s\n' "$*" >> "${STUB_CALLS:?}"
[ "$1" = compose ] || exit 2
shift
cmd=$1; shift
case $cmd in
  up|down)
    if [[ ${STUB_NOISY:-0} == 1 ]]; then
      echo 'error: primary build failure'
      awk 'BEGIN { for(i=0; i<20000; i++) printf "x"; print "END_OF_LOG" }'
    fi
    exit "${STUB_UP_RC:-0}" ;;
  logs) echo 'stub docker logs'; exit 0 ;;
  cp) exit "${STUB_CP_RC:-0}" ;;
  exec)
    while [ $# -gt 0 ]; do
      case $1 in
        -T|--user|abc|desktop|-dT) shift ;;
        env) shift 2 ;;
        *) break ;;
      esac
    done
    case ${1:-} in
      ./tools/verify.sh)
        [ "${STUB_VERIFY_MARKER:-0}" = 1 ] && echo 'VERIFY_ALL_OK (evidence saved in /workspace/verification/evidence/latest)'
        exit "${STUB_VERIFY_RC:-0}" ;;
      cargo)
        echo "${STUB_TEST_RESULT:-test result: ok. 1 passed; 0 failed; 0 ignored; 0 measured; 9 filtered out}"
        exit 0 ;;
      bash)
        [ "${STUB_SMOKE_MARKER:-0}" = 1 ] && echo 'DESKTOP_SMOKE_OK'
        exit "${STUB_SMOKE_RC:-0}" ;;
      *) exit 0 ;;
    esac ;;
  *) exit 0 ;;
esac
STUB
chmod +x "$fixture/bin/docker"

export PATH="$fixture/bin:$PATH"
export STUB_CALLS="$fixture/calls.txt"
: > "$STUB_CALLS"

run_dir() {
  local dir
  dir=$(sed -n 's/^logs: //p' "$1")
  printf '%s/%s\n' "$fixture/repo" "$dir"
}

fail=0
note() { if [ "$1" = "$2" ]; then echo "PASS $3"; else echo "FAIL $3 (expected $1, got $2)"; fail=1; fi; }

# Case 1: cp exits 17 -> check exits 17, no evidence path printed.
STUB_UP_RC=0 STUB_VERIFY_RC=0 STUB_VERIFY_MARKER=1 STUB_SMOKE_RC=0 STUB_SMOKE_MARKER=1 STUB_CP_RC=17 \
  "$fixture/repo/docker/ci" check > "$fixture/case1.out" 2>&1 && rc1=0 || rc1=$?
note 17 "$rc1" 'cp failure propagates exit code'
run1=$(run_dir "$fixture/case1.out")
if grep -q '^evidence:' "$fixture/case1.out"; then echo 'FAIL evidence path printed on cp failure'; fail=1; else echo 'PASS no evidence path on cp failure'; fi
[ -s "$run1/docker-logs.txt" ] && echo 'PASS docker-logs.txt saved' || { echo 'FAIL docker-logs.txt missing'; fail=1; }

# Case 2: verify prints marker but exits 23 -> check exits 23, no smoke/evidence.
: > "$STUB_CALLS"
STUB_UP_RC=0 STUB_VERIFY_RC=23 STUB_VERIFY_MARKER=1 \
  "$fixture/repo/docker/ci" check > "$fixture/case2.out" 2>&1 && rc2=0 || rc2=$?
note 23 "$rc2" 'verify nonzero status wins over marker'
run2=$(run_dir "$fixture/case2.out")
if ! grep -q 'desktop-smoke' "$STUB_CALLS" && ! grep -q ' cp ' "$STUB_CALLS"; then echo 'PASS no smoke/evidence after verify failure'; else echo 'FAIL smoke/evidence ran after verify failure'; fail=1; fi
[ -s "$run2/docker-logs.txt" ] && echo 'PASS docker-logs.txt saved' || { echo 'FAIL docker-logs.txt missing'; fail=1; }

# Case 3: verify exits 0 without marker -> check exits 1, no smoke/evidence.
: > "$STUB_CALLS"
STUB_UP_RC=0 STUB_VERIFY_RC=0 STUB_VERIFY_MARKER=0 \
  "$fixture/repo/docker/ci" check > "$fixture/case3.out" 2>&1 && rc3=0 || rc3=$?
note 1 "$rc3" 'missing marker fails verify'
run3=$(run_dir "$fixture/case3.out")
if ! grep -q 'desktop-smoke' "$STUB_CALLS" && ! grep -q ' cp ' "$STUB_CALLS"; then echo 'PASS no smoke/evidence without marker'; else echo 'FAIL smoke/evidence ran without marker'; fail=1; fi
[ -s "$run3/docker-logs.txt" ] && echo 'PASS docker-logs.txt saved' || { echo 'FAIL docker-logs.txt missing'; fail=1; }

# Case 4: all green -> check exits 0 and prints both markers and evidence path.
: > "$STUB_CALLS"
STUB_UP_RC=0 STUB_VERIFY_RC=0 STUB_VERIFY_MARKER=1 STUB_SMOKE_RC=0 STUB_SMOKE_MARKER=1 STUB_CP_RC=0 \
  "$fixture/repo/docker/ci" check > "$fixture/case4.out" 2>&1 && rc4=0 || rc4=$?
note 0 "$rc4" 'full check passes'
grep -q 'VERIFY_ALL_OK' "$fixture/case4.out" && grep -q 'DESKTOP_SMOKE_OK' "$fixture/case4.out" && grep -q '^evidence:' "$fixture/case4.out" \
  && echo 'PASS markers and evidence path printed' || { echo 'FAIL markers/evidence missing'; fail=1; }

# An exact-name run must execute the requested case, including in the final gate.
: > "$STUB_CALLS"
STUB_VERIFY_MARKER=1 STUB_SMOKE_MARKER=1 STUB_TEST_RESULT='test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 10 filtered out' \
  "$fixture/repo/docker/ci" check ij-app target typo > "$fixture/zero.out" 2>&1 && rc=0 || rc=$?
note 1 "$rc" 'zero matching tests fail check'
grep -Fq -- '--test target -- typo --exact' "$STUB_CALLS" || { echo 'FAIL exact test not requested'; fail=1; }

"$fixture/repo/docker/ci" test ij-app target case_name > "$fixture/one.out" 2>&1 && rc=0 || rc=$?
note 0 "$rc" 'one passing test accepted'

# A long Docker line stays on disk. The early error and tail fit in one response.
STUB_NOISY=1 STUB_UP_RC=42 "$fixture/repo/docker/ci" up > "$fixture/noisy.out" 2>&1 && rc=0 || rc=$?
note 42 "$rc" 'noisy Docker failure propagates exit code'
noisy_dir=$(run_dir "$fixture/noisy.out")
if [[ $(wc -c < "$fixture/noisy.out") -lt 4096 &&
      $(wc -c < "$noisy_dir/up.log") -gt 20000 ]] &&
    grep -q 'primary build failure' "$fixture/noisy.out" && grep -q 'END_OF_LOG' "$fixture/noisy.out"; then
  echo 'PASS bounded output retains cause and tail, full log on disk'
else
  echo 'FAIL diagnostic output budget or full log'
  fail=1
fi

exit "$fail"
