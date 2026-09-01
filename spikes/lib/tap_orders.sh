# read_reply_orders — the byte orders a foreign peer wrote, off §15.4.1's flag byte.
#
# `. spikes/lib/tap_orders.sh` and then:
#   read_reply_orders <tap-log>        # prints `observed<TAB>giop=…<TAB>order=…`
#   note_request_orders <tap-log>      # prints our own orders, as a NOTE
#
# **The replies, never the requests.** In the client direction the peer is the
# one answering, so its order is the order of what IT wrote. Our requests are
# ours; D032 §4 calls an order read off the flag byte `observed` and one inferred
# from the peer's host `claimed`, and only `observed` counts toward a cell. Our
# own order is not evidence about a peer, which is why the requests come back
# through a separate function that labels them `note`.
#
# AN ABSENT READING IS A FAILURE, NOT A QUIET PASS. A tap that recorded no reply
# means the calls may have completed and the order was still never read, and a
# cell that printed nothing there would be covered on paper. `read_reply_orders`
# returns 1 and says so.
#
# TWO PARSING BUGS ARE FROZEN HERE BECAUSE THEY BOTH SHIPPED. The tap writes
# `... Reply size=16 BE id=1 status=0` — the order is in the MIDDLE, so an
# end-anchored `sed` found nothing; and the obvious repair,
# `sed -n 's/.*\(BE\|LE\).*/\1/p'`, is a GNU extension that BSD sed on this
# machine reads as a literal `\|`. Both spellings turned a cell that had reached
# the wire into one that looked unable to parse it. A `case` has neither
# problem, and having it in one file means the next cell cannot rediscover them.
#
# *답신만 읽는다 — 클라이언트 방향에서 피어는 답하는 쪽이고, 우리 요청의 순서는
# 피어에 대한 증거가 아니다. 읽히지 않은 것은 조용한 통과가 아니라 실패다. 두
# 파싱 버그를 여기 얼려 둔다: 순서는 줄 가운데 있고, BSD sed는 `\|`를 리터럴로
# 읽는다. 둘 다 실제로 배포됐었다.*

read_reply_orders() {
  local log="$1" replies line v order key seen=""
  replies=$(grep "S->C GIOP" "$log" 2>/dev/null | grep " Reply ")
  if [ -z "$replies" ]; then
    echo "FAIL	the calls completed but the tap recorded no reply, so the byte order"
    echo "FAIL	was NOT read off the wire. An absent reading cannot count as covered."
    return 1
  fi
  while IFS= read -r line; do
    v=$(sed -n 's/.*GIOP \([0-9]\.[0-9]\).*/\1/p' <<<"$line")
    case "$line" in
      *" BE "*|*" BE") order=big ;;
      *" LE "*|*" LE") order=little ;;
      *) echo "FAIL	a tap line names no byte order: $line"; return 1 ;;
    esac
    [ -n "$v" ] || { echo "FAIL	a tap line names no GIOP version: $line"; return 1; }
    key="$v/$order"
    case " $seen " in *" $key "*) continue ;; esac
    seen="$seen $key"
    printf 'observed\tgiop=%s\torder=%s\n' "$v" "$order"
  done <<<"$replies"
  return 0
}

note_request_orders() {
  local log="$1" orders
  orders=$(grep "C->S GIOP" "$log" 2>/dev/null | grep " Request " \
           | grep -oE ' (BE|LE) ' | tr -d ' ' | sort -u | tr '\n' ' ')
  printf 'note\tour own requests were written %s— reported as a note, because our order is not evidence about a peer\n' "$orders"
}
