#!/usr/bin/env bash
# What memory looked like while a run was happening, and what the kernel killed
# for it.
#
# ── Why this exists ──────────────────────────────────────────────────────────
#
# 2026-08-27, 15:17:50 KST: this machine stopped. The unified log has zero
# entries between 15:17:50.736 and the boot banner at 15:19:07.732, and
# `ResetCounter-2026-08-27-151943.diag` reads `Boot faults: btn_rst,
# finger_reset force_off` — seventy-seven seconds of nothing, then somebody
# held the power button. There is no `.panic` report, so the kernel never got
# far enough to write one.
#
# What could be established afterwards: the machine has 16 GB, the kernel had
# already jetsam-killed processes twice in two days, and in the eighty minutes
# before the freeze the pid counter wrapped past 100,000 — 5,284 distinct
# `node` pids, 397 `python3.14`, 69 `cargo`. What could NOT be established:
# **what memory looked like at 15:17:50, or which process took the last page.**
# Nothing was recording. So the freeze is unattributable, and the honest thing
# to say about it is a hypothesis with a census attached rather than a cause.
#
# That is the gap this closes. It is a flight recorder, not a gate: it appends
# one line per tick and flushes as it goes, so the file survives a machine that
# dies without unwinding. A run that ends normally gets a summary out of it; a
# run that ends with the power button leaves the file behind for whoever boots
# the machine next.
#
# **No threshold here is a gate.** There is no defensible number for "too
# little memory left" — the same reason `entry_cost.py` reports and does not
# gate, and the same reason the CI disk steps print a margin instead of
# asserting one. What IS a gate lives in the harness and is a different claim:
# if the kernel killed something for memory while a group was measuring, that
# group's verdict is not evidence, and an unmeasured check is a failure, never
# a pass.
#
# Usage:
#   memlog.sh record  --out F [--interval N]   sample until killed
#   memlog.sh summary --out F                  peak pressure + who held it
#   memlog.sh kills   --since EPOCH            kernel memory kills since EPOCH
#   memlog.sh kills   --from-file F            the same parser over a fixture,
#                                              so the group can prove it can
#                                              see a kill before its silence
#                                              over the real system means
#                                              anything
set -uo pipefail

mode=${1:-}; shift || true
out=""; interval=5; since=""; from_file=""
while [ $# -gt 0 ]; do
  case "$1" in
    --out)       out=$2; shift 2 ;;
    --interval)  interval=$2; shift 2 ;;
    --since)     since=$2; shift 2 ;;
    --from-file) from_file=$2; shift 2 ;;
    *) echo "memlog.sh: unknown argument $1" >&2; exit 2 ;;
  esac
done

# ── One sample ───────────────────────────────────────────────────────────────
#
# Fields are tab-separated and fixed in order, so `summary` can read them with
# awk and a later reader can load the file into anything. Sizes are MB.
#
#   epoch  iso  total  used  avail  swap_used  top1  top2  top3
#
# `avail` is the number that matters and it is NOT "free": on macOS the pages
# that can still be handed out are free + inactive + purgeable + speculative,
# and on Linux the kernel computes it directly as MemAvailable. Reporting bare
# free pages would have this file screaming on a perfectly healthy Mac, which
# is how a reading gets ignored.
sample() {
  local epoch iso avail total used swap top
  epoch=$(date +%s)
  iso=$(date -u '+%Y-%m-%dT%H:%M:%SZ')
  if [ "$(uname -s)" = "Darwin" ]; then
    total=$(( $(sysctl -n hw.memsize) / 1048576 ))
    # vm_stat's page size is printed in its header rather than assumed: it is
    # 16 KB on Apple silicon and 4 KB on Intel, and hardcoding either makes
    # this file wrong on half the machines it runs on.
    avail=$(vm_stat | awk '
      /page size of/ { for (i=1;i<=NF;i++) if ($i=="of") { ps=$(i+1); break } }
      /^Pages free/            { f=$3 }
      /^Pages inactive/        { in_=$3 }
      /^Pages speculative/     { sp=$3 }
      /^Pages purgeable/       { pu=$3 }
      END { gsub(/\./,"",f); gsub(/\./,"",in_); gsub(/\./,"",sp); gsub(/\./,"",pu)
            printf "%d", (f+in_+sp+pu)*ps/1048576 }')
    used=$(( total - avail ))
    swap=$(sysctl -n vm.swapusage | awk '{for(i=1;i<=NF;i++) if($i=="used") {gsub(/[^0-9.]/,"",$(i+2)); printf "%d", $(i+2)}}')
  else
    total=$(awk '/^MemTotal:/{printf "%d",$2/1024}' /proc/meminfo)
    avail=$(awk '/^MemAvailable:/{printf "%d",$2/1024}' /proc/meminfo)
    used=$(( total - avail ))
    swap=$(awk '/^SwapTotal:/{t=$2} /^SwapFree:/{f=$2} END{printf "%d",(t-f)/1024}' /proc/meminfo)
  fi
  # The three largest resident processes, by name and MB. This is the half that
  # makes a peak actionable: "3 GB left" says a run was tight, "3 GB left and
  # rustc held 6" says what to do about it.
  top=$(ps -eo rss=,comm= 2>/dev/null | sort -rn | head -3 \
        | awk '{ n=$2; sub(/.*\//,"",n); printf "%s=%dMB\t", n, $1/1024 }')
  printf '%s\t%s\t%s\t%s\t%s\t%s\t%s\n' \
    "$epoch" "$iso" "${total:-0}" "${used:-0}" "${avail:-0}" "${swap:-0}" "${top%	}"
}

case "$mode" in

  record)
    [ -n "$out" ] || { echo "memlog.sh record: --out is required" >&2; exit 2; }
    # Truncate, then append. A previous run's file is moved aside rather than
    # deleted: if the machine died mid-run, that file is the only record of it,
    # and the next run must not be the thing that destroys the evidence.
    [ -s "$out" ] && mv -f "$out" "$out.prev" 2>/dev/null
    printf '# epoch\tiso\ttotal_mb\tused_mb\tavail_mb\tswap_mb\ttop3\n' >"$out"
    printf '# recorder pid %s, interval %ss, host %s\n' "$$" "$interval" "$(uname -n)" >>"$out"
    # ── Reap the sleep, or the recorder leaks one ────────────────────────────
    #
    # A recorder that is killed while blocked in `sleep` leaves that `sleep`
    # behind: the signal reaches this shell, not its child, so the child is
    # orphaned to init while keeping the process group it inherited from the
    # harness — which is precisely the "a fixture this run started outlived
    # it" class, produced by the very thing that watches for it.
    #
    # **Caught in CI, not here, and the reason is a lesson.** `run_checks.sh`
    # stops the recorder at the top of its memory group and counts leaks a few
    # lines later. On macOS the kills query in between is `log show` and takes
    # about ten seconds, so a 5-second `sleep` had always finished on its own
    # and the group said `ok` — twice. On Linux the same query is `dmesg` and
    # returns instantly, so the orphan was still there and the group reported
    # `1 process(es) this run started outlived it: 66664 sleep`. The leak was
    # local too; a slow query was hiding it.
    #
    # So the sleep runs as a job whose pid is known, and the trap kills it.
    # Backgrounding plus `wait` also makes the signal prompt: a foreground
    # `sleep` would delay the trap until the interval elapsed.
    _nap=""
    trap '[ -n "$_nap" ] && kill "$_nap" 2>/dev/null; exit 0' TERM INT HUP
    while :; do
      sample >>"$out"
      sleep "$interval" &
      _nap=$!
      wait "$_nap" 2>/dev/null
      _nap=""
    done
    ;;

  summary)
    [ -n "$out" ] || { echo "memlog.sh summary: --out is required" >&2; exit 2; }
    if [ ! -s "$out" ]; then
      echo "no samples were recorded"
      exit 1
    fi
    awk -F'\t' '
      /^#/ { next }
      { n++
        if (min == "" || $5 < min) { min=$5; min_iso=$2; min_top=$7; min_swap=$6 }
        if (max == "" || $5 > max) { max=$5 }
        total=$3
      }
      END {
        if (n == 0) { print "no samples were recorded"; exit 1 }
        printf "%d sample(s) over a %d MB machine\n", n, total
        printf "least available: %d MB at %s (swap %d MB)\n", min, min_iso, min_swap
        printf "most available:  %d MB\n", max
        printf "largest resident at that moment: %s\n", min_top
      }' "$out"
    ;;

  kills)
    # One line per kernel memory kill, or nothing. Exit 0 whether or not there
    # were any — the caller decides what a hit means; this only reports.
    # Exit 3 means the parser could not read its source at all, which is a
    # different answer from "no kills" and must never be printed as one.
    # ── What counts as a memory kill, and what emphatically does not ─────────
    #
    # `memorystatus: killing` is NOT the pattern. macOS reaps idle daemons
    # through the same subsystem and logs them the same way: measured
    # 2026-08-27, **782 `memorystatus: killing` lines in fifty minutes on an
    # idle-ish Mac**, essentially all of them `idle-exit`. A gate on that
    # pattern would be red on every run, which is the fastest way to make a
    # gate stop being read.
    #
    # So the pattern names the reasons that mean **the machine** was out of
    # memory: page shortage, compressor thrashing, jetsam picking the largest
    # resident process. An idle-exit is not one of them.
    #
    # **And neither is `per-process-limit`, which the first version of this
    # pattern included and which went red on its first real run.** Measured
    # 2026-08-27, by this gate failing its own author: `postersyncd` was killed
    # at `(per-process-limit … ) 46592KB` with
    # `memorystatus_available_pages: 629074` on the same line — **9.8 GB
    # available**. That is a daemon hitting *its own* configured jetsam limit,
    # not a machine under pressure, and it says nothing about whether this
    # run's fixtures were at risk. `highwater` is the same mechanism.
    #
    # It is also a class our fixtures cannot be in: a per-process limit is set
    # by RunningBoard for processes it manages, and everything this harness
    # starts is spawned from a shell and managed by nobody. So a
    # per-process-limit kill can never be one of ours — dropping it loses no
    # coverage and removes a red that would have arrived on an idle machine.
    #
    # The probe below therefore carries **three** lines, one per decision: a
    # machine-pressure kill that must be seen, an idle-exit that must not, and
    # a per-process-limit kill that must not. A false red kills a gate as
    # surely as a true one nobody reads.
    PRESSURE='vm-pageshortage|vm-compressor|fc-thrashing|killing_top_process|jetsam'
    if [ -n "$from_file" ]; then
      [ -r "$from_file" ] || { echo "memlog.sh kills: cannot read $from_file" >&2; exit 3; }
      raw=$(cat "$from_file")
      grep -aE "$PRESSURE|Out of memory: Kill|oom-kill:|Killed process" <<<"$raw" || true
      exit 0
    fi
    [ -n "$since" ] || { echo "memlog.sh kills: --since EPOCH is required" >&2; exit 2; }
    # Validate the argument, because getting it wrong does not fail — it HANGS.
    # `sysctl -n kern.boottime` prints `{ sec = 1787812345, usec = 12345 }` and
    # a careless sed lifted `7537` out of it; `log show --start` then walked the
    # whole store from 1970 and the caller waited until something killed it. A
    # measurement tool that hangs on a bad argument is worse than one that
    # refuses, so this refuses: exit 2 is "the caller is wrong", distinct from
    # exit 3, "the source could not be read", distinct from 0, "here is what
    # there was".
    now=$(date +%s)
    case "$since" in
      ''|*[!0-9]*) echo "memlog.sh kills: --since must be an epoch in seconds, got '$since'" >&2; exit 2 ;;
    esac
    if [ "$since" -gt "$now" ] || [ $(( now - since )) -gt 604800 ]; then
      echo "memlog.sh kills: --since $since is in the future or more than a week old;" >&2
      echo "  a window that wide makes the query walk the whole log store" >&2
      exit 2
    fi
    # Capture the producer, read ITS status, then match with a herestring.
    # Not because a `grep` here would SIGPIPE — it has no `-q` and would read
    # its whole input — but because the status that matters is the producer's:
    # a `log show` that could not run at all prints nothing, and a caller that
    # matched down a pipe would read that silence as "no kills". An unmeasured
    # check is a failure, never a pass, and the caller can only tell the two
    # apart if this exits 3 rather than 0.
    if [ "$(uname -s)" = "Darwin" ]; then
      [ -x /usr/bin/log ] || exit 3
      # `log` is also a zsh builtin that takes no arguments, so the absolute
      # path is load-bearing: `log show ...` under zsh answers
      # "too many arguments" and a caller reading only stdout sees silence.
      start=$(date -r "$since" '+%Y-%m-%d %H:%M:%S' 2>/dev/null) || exit 3
      raw=$(/usr/bin/log show --start "$start" \
              --predicate 'eventMessage CONTAINS "memorystatus: killing"' \
              --style compact 2>&1); rc=$?
      [ "$rc" -eq 0 ] || exit 3
      grep -aE "$PRESSURE" <<<"$raw" || true
    else
      # dmesg is unreadable by unprivileged users on many distributions
      # (kernel.dmesg_restrict); that is exit 3, not "no kills".
      #
      # `--time-format=iso` rather than `-T`, and its absence is exit 3 rather
      # than a wider answer. An OOM kill from last Tuesday is not something
      # this run did, and reporting one as though it were would make the gate
      # above red for a reason nobody could act on — which is how a gate stops
      # being read. If the window cannot be applied, the honest answer is that
      # the question was not measured, not that it was answered generously.
      raw=$(dmesg --time-format=iso 2>/dev/null); rc=$?
      if [ "$rc" -ne 0 ]; then
        raw=$(sudo -n dmesg --time-format=iso 2>/dev/null); rc=$?
      fi
      [ "$rc" -eq 0 ] || exit 3
      hits=$(grep -aE "Out of memory: Kill|oom-kill:|Killed process" <<<"$raw") || true
      [ -n "$hits" ] || exit 0
      while IFS= read -r line; do
        ts=${line%% *}; ts=${ts%,*}
        when=$(date -d "$ts" +%s 2>/dev/null) || exit 3
        [ "$when" -ge "$since" ] && printf '%s\n' "$line"
      done <<<"$hits"
    fi
    ;;

  *)
    sed -n '2,45p' "$0" | sed 's/^# \{0,1\}//'
    exit 2
    ;;
esac
