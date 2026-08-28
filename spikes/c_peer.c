/* c_peer.c — a C program that speaks GIOP over a socket. Not a C ORB.
 *
 * `docs/decisions/D033-the-programme.md` §6 item 3.4 says why this file is C's
 * first batch and not an emitter: *"No C ORB exists here — omniORB is C++. The
 * candidate that fits this project's licence position and its own precedent is
 * a hand-written C peer speaking GIOP, as `ssliop_peer.py` was for TLS: the
 * peer a binding needs is one that speaks the protocol, not another ORB."*
 *
 * D030 §3 decides whether a language is a target: *measured against a peer that
 * is not us, in both byte orders, and its refusals say the same sentences ours
 * do.* A C emitter written before this file existed would have been measured
 * against itself, which that rule refuses by name. So the peer comes first.
 *
 * *D033 §6 3.4 — C의 첫 배치는 방출기가 아니라 피어다. 이 트리에 C ORB는 없고
 * omniORB는 C++다. `ssliop_peer.py`가 TLS에 대해 그러했듯, 바인딩에 필요한 피어는
 * 또 하나의 ORB가 아니라 프로토콜을 말하는 프로그램이다.*
 *
 * ── The licence boundary is the design, not a caveat ────────────────────────
 *
 * TEST FIXTURE ONLY, and deliberately not an ORB. Nothing is included beyond
 * C99 and POSIX sockets: no omniORB, no TAO, no JacORB, not their headers, not
 * their generated stubs, not their IDL. Every GIOP and IOR octet below is built
 * here from the published OMG specification, which CLAUDE.md's licensing
 * boundary says is logic we implement ourselves and owe nobody for. A C peer
 * that wrapped `libomniORB` would be our code calling a fixture and could not
 * be an independent measurement of anything.
 *
 * The point of writing it by hand is the same point `ssliop_peer.py` makes:
 * **bytes produced by the encoder under test cannot agree with this file by
 * construction.** It shares no line of code, no constant and no table with
 * `crates/`.
 *
 * ── Alignment origin, which is where a hand-written CDR encoder goes wrong ───
 *
 * CLAUDE.md has a rule about this because the project has got it wrong: *a GIOP
 * message aligns from the first byte of its 12-byte header; an encapsulation
 * restarts alignment at its own first byte.* Both are honoured here by the same
 * device the Python peer uses — every buffer carries its own origin at offset
 * zero, and `align()` is computed against that buffer's length. A GIOP message
 * writes its header into the same buffer it aligns in, so the origin *is* the
 * magic. An encapsulation gets a fresh buffer whose offset zero is its
 * byte-order octet (§9.3.3).
 *
 * ── The byte order is read, never assumed ───────────────────────────────────
 *
 * `spikes/bindings/AXES` says a cell reports the order it READ out of GIOP
 * §15.4.1's flag byte of what the peer actually wrote, and that a cell which
 * asserts an order from the peer's language reports it as `claimed`, counted
 * separately and never as met. So this peer reports two orders per exchange and
 * labels each: the order it wrote itself (`written`, which it knows because it
 * chose it) and the order the reply came back in, taken from bit 0 of octet 6
 * of the reply's own header (`observed`). Our `Connection` writes its native
 * order, so on any one machine an echoing peer would leave one order unmeasured
 * — which is why `--request-endian` is an axis and not a default.
 *
 * ── What it does not do, so the next reader does not have to find out ───────
 *
 *   - No IDL. Operations and argument shapes are given on the command line;
 *     nothing here parses a contract, and nothing here is generated.
 *   - No TypeCode, no Any, no `valuetype`, no object references as arguments.
 *     Arguments and results are `long`, `string` and `void` — the shapes a
 *     hand-written peer can carry without becoming a marshalling library.
 *   - No fragmentation (GIOP 1.1+ `Fragment`): a request this fixture writes is
 *     never big enough to need one, and a reply that arrives fragmented is
 *     reported as such rather than reassembled.
 *   - No GIOP 1.1 codeset negotiation and no wide text; `jacorb_wchar11.sh`
 *     already measures that against a peer that is genuinely not us.
 *   - Not a POA and not a servant framework. `--role server` answers exactly
 *     one hard-coded operation, so that the *client* direction of a binding has
 *     something to dial.
 *
 * ── Usage ───────────────────────────────────────────────────────────────────
 *
 *   c_peer --role client --ior-file <path> [--op add] [--arg-long N]...
 *          [--arg-string S] [--expect long|string|void|any]
 *          [--request-endian little|big] [--giop 1.0|1.1|1.2]
 *          [--object-key-hex <hex>] [--magic <4 chars>] [--deadline-s 20]
 *
 *   c_peer --role server --port-file <p> --ior-file <e>
 *          [--reply-endian little|big] [--requests 1] [--deadline-s 20]
 *
 * Prints one JSON object on stdout when it has run to the end, reporting **what
 * it observed**. It does not decide whether that was right: the runner knows
 * what was asked for and judges. **The exit code is the verdict** on whether
 * this program ran to the end — 0 ran, 1 did not, 2 its fixture is absent.
 *
 * *관찰한 것을 JSON으로 보고하고 판정은 러너가 한다. 종료 코드가 판정이다.*
 */

#include <arpa/inet.h>
#include <errno.h>
#include <netdb.h>
#include <netinet/in.h>
#include <netinet/tcp.h>
#include <stdarg.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/time.h>
#include <unistd.h>

#define EXIT_RAN 0     /* ran to the end; what it saw is on stdout */
#define EXIT_FAILED 1  /* did not run to the end */
#define EXIT_ABSENT 2  /* its fixture is absent — the suite's SKIPPED code */

/* GIOP §15.4.1 and the message types of §15.4. Named here rather than
 * included from anywhere: this file may not depend on the crate under test,
 * and there is no header to take them from that is not somebody's ORB. */
#define GIOP_HEADER_LEN 12
#define FLAG_LITTLE_ENDIAN 0x01
#define FLAG_MORE_FRAGMENTS 0x02

#define MSG_REQUEST 0
#define MSG_REPLY 1
#define MSG_CANCEL_REQUEST 2
#define MSG_LOCATE_REQUEST 3
#define MSG_LOCATE_REPLY 4
#define MSG_CLOSE_CONNECTION 5
#define MSG_MESSAGE_ERROR 6
#define MSG_FRAGMENT 7

/* §15.4.3.1 ReplyStatusType. */
#define REPLY_NO_EXCEPTION 0
#define REPLY_USER_EXCEPTION 1
#define REPLY_SYSTEM_EXCEPTION 2
#define REPLY_LOCATION_FORWARD 3
#define REPLY_LOCATION_FORWARD_PERM 4
#define REPLY_NEEDS_ADDRESSING_MODE 5

#define TAG_INTERNET_IOP 0

/* The contract `--role server` answers, so that a binding's *client* direction
 * has something of ours-that-is-not-ours to dial. Deliberately tiny. */
#define SERVER_TYPE_ID "IDL:orbweaver/CPeerEcho:1.0"
#define SERVER_OBJECT_KEY "c-peer-echo"

/* ── a growable octet buffer whose offset zero is its alignment origin ─────── */

typedef struct {
    unsigned char *d;
    size_t len;
    size_t cap;
    int little;
} buf_t;

static void buf_init(buf_t *b, int little)
{
    b->d = NULL;
    b->len = 0;
    b->cap = 0;
    b->little = little;
}

static void buf_free(buf_t *b)
{
    free(b->d);
    b->d = NULL;
    b->len = b->cap = 0;
}

static void buf_reserve(buf_t *b, size_t extra)
{
    if (b->len + extra <= b->cap)
        return;
    size_t want = b->cap ? b->cap : 64;
    while (want < b->len + extra)
        want *= 2;
    unsigned char *n = realloc(b->d, want);
    if (!n) {
        fprintf(stderr, "c_peer: out of memory growing a buffer to %zu\n", want);
        exit(EXIT_FAILED);
    }
    b->d = n;
    b->cap = want;
}

static void buf_raw(buf_t *b, const void *p, size_t n)
{
    buf_reserve(b, n);
    memcpy(b->d + b->len, p, n);
    b->len += n;
}

/* Alignment is against THIS buffer's origin. For a GIOP message that origin is
 * the first octet of the 12-byte header, because the header is written into
 * this same buffer; for an encapsulation it is the byte-order octet. */
static void buf_align(buf_t *b, size_t n)
{
    while (b->len % n) {
        unsigned char z = 0;
        buf_raw(b, &z, 1);
    }
}

static void buf_u8(buf_t *b, unsigned v)
{
    unsigned char c = (unsigned char)(v & 0xFF);
    buf_raw(b, &c, 1);
}

static void buf_u16(buf_t *b, unsigned v)
{
    unsigned char t[2];
    buf_align(b, 2);
    if (b->little) {
        t[0] = (unsigned char)(v & 0xFF);
        t[1] = (unsigned char)((v >> 8) & 0xFF);
    } else {
        t[0] = (unsigned char)((v >> 8) & 0xFF);
        t[1] = (unsigned char)(v & 0xFF);
    }
    buf_raw(b, t, 2);
}

static void buf_u32(buf_t *b, uint32_t v)
{
    unsigned char t[4];
    buf_align(b, 4);
    if (b->little) {
        t[0] = (unsigned char)(v & 0xFF);
        t[1] = (unsigned char)((v >> 8) & 0xFF);
        t[2] = (unsigned char)((v >> 16) & 0xFF);
        t[3] = (unsigned char)((v >> 24) & 0xFF);
    } else {
        t[0] = (unsigned char)((v >> 24) & 0xFF);
        t[1] = (unsigned char)((v >> 16) & 0xFF);
        t[2] = (unsigned char)((v >> 8) & 0xFF);
        t[3] = (unsigned char)(v & 0xFF);
    }
    buf_raw(b, t, 4);
}

static void buf_i32(buf_t *b, int32_t v)
{
    buf_u32(b, (uint32_t)v);
}

/* §9.3.2.7: a CDR string's length counts the terminating NUL. */
static void buf_string(buf_t *b, const char *s)
{
    size_t n = strlen(s) + 1;
    buf_u32(b, (uint32_t)n);
    buf_raw(b, s, n);
}

static void buf_octets(buf_t *b, const void *p, size_t n)
{
    buf_u32(b, (uint32_t)n);
    buf_raw(b, p, n);
}

/* §9.3.3: an encapsulation is a fresh alignment origin whose offset zero is
 * its own byte-order octet. */
static void encap_init(buf_t *b, int little)
{
    buf_init(b, little);
    buf_u8(b, little ? 1 : 0);
}

/* ── the reader, alignment origin at index zero ────────────────────────────── */

typedef struct {
    const unsigned char *d;
    size_t len;
    size_t p;
    int little;
    int bad;          /* sticky: set on truncation, so callers may check once */
    char why[160];
} rdr_t;

static void rdr_init(rdr_t *r, const unsigned char *d, size_t len, int little)
{
    r->d = d;
    r->len = len;
    r->p = 0;
    r->little = little;
    r->bad = 0;
    r->why[0] = '\0';
}

static int rdr_need(rdr_t *r, size_t n)
{
    if (r->bad)
        return 0;
    if (r->p + n > r->len) {
        r->bad = 1;
        snprintf(r->why, sizeof r->why,
                 "truncated: wanted %zu octets at %zu of %zu", n, r->p, r->len);
        return 0;
    }
    return 1;
}

static void rdr_align(rdr_t *r, size_t n)
{
    if (r->bad)
        return;
    r->p = ((r->p + n - 1) / n) * n;
    if (r->p > r->len) {
        r->bad = 1;
        snprintf(r->why, sizeof r->why, "alignment ran past the end of %zu", r->len);
    }
}

static unsigned rdr_u8(rdr_t *r)
{
    if (!rdr_need(r, 1))
        return 0;
    return r->d[r->p++];
}

static unsigned rdr_u16(rdr_t *r)
{
    rdr_align(r, 2);
    if (!rdr_need(r, 2))
        return 0;
    const unsigned char *t = r->d + r->p;
    r->p += 2;
    return r->little ? (unsigned)(t[0] | (t[1] << 8))
                     : (unsigned)((t[0] << 8) | t[1]);
}

static uint32_t rdr_u32(rdr_t *r)
{
    rdr_align(r, 4);
    if (!rdr_need(r, 4))
        return 0;
    const unsigned char *t = r->d + r->p;
    r->p += 4;
    return r->little ? ((uint32_t)t[0] | ((uint32_t)t[1] << 8) |
                        ((uint32_t)t[2] << 16) | ((uint32_t)t[3] << 24))
                     : (((uint32_t)t[0] << 24) | ((uint32_t)t[1] << 16) |
                        ((uint32_t)t[2] << 8) | (uint32_t)t[3]);
}

static int32_t rdr_i32(rdr_t *r)
{
    return (int32_t)rdr_u32(r);
}

/* Returns a malloc'd copy of the octet sequence, and its length. */
static unsigned char *rdr_octets(rdr_t *r, size_t *out_len)
{
    uint32_t n = rdr_u32(r);
    *out_len = 0;
    if (r->bad)
        return NULL;
    /* A length field is attacker-controlled in the general case and merely
     * wrong in this one; either way it is not a reason to call malloc with it.
     * CLAUDE.md's IMPLAUSIBLE_LENGTH class, in the peer rather than the ORB. */
    if (n > r->len) {
        r->bad = 1;
        snprintf(r->why, sizeof r->why,
                 "implausible length %u in a message of %zu octets",
                 (unsigned)n, r->len);
        return NULL;
    }
    if (!rdr_need(r, n))
        return NULL;
    unsigned char *out = malloc((size_t)n + 1);
    if (!out) {
        fprintf(stderr, "c_peer: out of memory reading %u octets\n", (unsigned)n);
        exit(EXIT_FAILED);
    }
    memcpy(out, r->d + r->p, n);
    out[n] = '\0';
    r->p += n;
    *out_len = n;
    return out;
}

/* A CDR string, returned NUL-terminated. NULL if the stream is exhausted or the
 * final octet is not the NUL the specification requires. */
static char *rdr_string(rdr_t *r)
{
    size_t n = 0;
    unsigned char *raw = rdr_octets(r, &n);
    if (!raw)
        return NULL;
    if (n == 0 || raw[n - 1] != 0) {
        r->bad = 1;
        snprintf(r->why, sizeof r->why, "a CDR string must end in a NUL");
        free(raw);
        return NULL;
    }
    return (char *)raw;
}

/* ── JSON, by hand, because a fixture may not take a dependency either ─────── */

static void json_escape(FILE *f, const char *s)
{
    fputc('"', f);
    for (; *s; s++) {
        unsigned char c = (unsigned char)*s;
        if (c == '"' || c == '\\')
            fprintf(f, "\\%c", c);
        else if (c == '\n')
            fputs("\\n", f);
        else if (c == '\r')
            fputs("\\r", f);
        else if (c == '\t')
            fputs("\\t", f);
        else if (c < 0x20 || c == 0x7F)
            fprintf(f, "\\u%04x", c);
        else
            fputc((char)c, f);
    }
    fputc('"', f);
}

/* ── hex ───────────────────────────────────────────────────────────────────── */

static int hex_nibble(int c)
{
    if (c >= '0' && c <= '9')
        return c - '0';
    if (c >= 'a' && c <= 'f')
        return c - 'a' + 10;
    if (c >= 'A' && c <= 'F')
        return c - 'A' + 10;
    return -1;
}

static unsigned char *hex_decode(const char *s, size_t *out_len)
{
    size_t n = strlen(s);
    if (n % 2) {
        *out_len = 0;
        return NULL;
    }
    unsigned char *out = malloc(n / 2 + 1);
    if (!out) {
        fprintf(stderr, "c_peer: out of memory decoding %zu hex digits\n", n);
        exit(EXIT_FAILED);
    }
    for (size_t i = 0; i < n; i += 2) {
        int hi = hex_nibble(s[i]);
        int lo = hex_nibble(s[i + 1]);
        if (hi < 0 || lo < 0) {
            free(out);
            *out_len = 0;
            return NULL;
        }
        out[i / 2] = (unsigned char)((hi << 4) | lo);
    }
    *out_len = n / 2;
    return out;
}

static void hex_print(FILE *f, const unsigned char *d, size_t n)
{
    fputc('"', f);
    for (size_t i = 0; i < n; i++)
        fprintf(f, "%02x", d[i]);
    fputc('"', f);
}

/* ── the reference, parsed by hand (§7.6.9, §9.7.2) ────────────────────────── */

typedef struct {
    char *type_id;
    char host[256];
    unsigned port;
    unsigned char key[512];
    size_t key_len;
    unsigned iiop_major;
    unsigned iiop_minor;
    unsigned profiles;
    unsigned components;
    int ior_little;         /* the order the IOR's own encapsulation used */
    int profile_little;     /* the profile encapsulation's — a separate axis */
    int ok;
    char why[200];
} ior_t;

static void ior_parse(ior_t *o, const char *text)
{
    memset(o, 0, sizeof *o);
    o->ok = 0;

    const char *hex = text;
    while (*hex == ' ' || *hex == '\t' || *hex == '\n' || *hex == '\r')
        hex++;
    if (strncmp(hex, "IOR:", 4) != 0) {
        snprintf(o->why, sizeof o->why, "not a stringified IOR: no IOR: prefix");
        return;
    }
    hex += 4;

    /* Trim any trailing whitespace the file may carry. */
    char *clean = strdup(hex);
    if (!clean) {
        fprintf(stderr, "c_peer: out of memory copying an IOR\n");
        exit(EXIT_FAILED);
    }
    size_t cl = strlen(clean);
    while (cl && (clean[cl - 1] == '\n' || clean[cl - 1] == '\r' ||
                  clean[cl - 1] == ' ' || clean[cl - 1] == '\t'))
        clean[--cl] = '\0';

    size_t blen = 0;
    unsigned char *body = hex_decode(clean, &blen);
    free(clean);
    if (!body) {
        snprintf(o->why, sizeof o->why, "the IOR body is not even-length hex");
        return;
    }

    /* The body is an encapsulation: offset zero is its byte-order octet. */
    rdr_t r;
    rdr_init(&r, body, blen, 0);
    unsigned bo = rdr_u8(&r);
    r.little = (bo & 1) != 0;
    o->ior_little = r.little;

    o->type_id = rdr_string(&r);
    o->profiles = rdr_u32(&r);
    if (r.bad || o->profiles == 0) {
        snprintf(o->why, sizeof o->why, "%s",
                 r.bad ? r.why : "the IOR carries no profile");
        free(body);
        return;
    }

    int found = 0;
    for (unsigned i = 0; i < o->profiles && !r.bad; i++) {
        uint32_t tag = rdr_u32(&r);
        size_t plen = 0;
        unsigned char *pdata = rdr_octets(&r, &plen);
        if (!pdata)
            break;
        if (tag == TAG_INTERNET_IOP && !found) {
            /* A fresh alignment origin, and its own byte order, which is not
             * required to be the enclosing IOR's. */
            rdr_t p;
            rdr_init(&p, pdata, plen, 0);
            unsigned pbo = rdr_u8(&p);
            p.little = (pbo & 1) != 0;
            o->profile_little = p.little;
            o->iiop_major = rdr_u8(&p);
            o->iiop_minor = rdr_u8(&p);
            char *host = rdr_string(&p);
            o->port = rdr_u16(&p);
            size_t klen = 0;
            unsigned char *key = rdr_octets(&p, &klen);
            if (!p.bad && host && key && klen <= sizeof o->key) {
                snprintf(o->host, sizeof o->host, "%s", host);
                memcpy(o->key, key, klen);
                o->key_len = klen;
                /* Components only exist from IIOP 1.1 on. */
                if (o->iiop_major > 1 || (o->iiop_major == 1 && o->iiop_minor >= 1))
                    o->components = rdr_u32(&p);
                found = 1;
            } else if (!p.bad) {
                snprintf(o->why, sizeof o->why,
                         "an object key of %zu octets is larger than this fixture carries",
                         klen);
            } else {
                snprintf(o->why, sizeof o->why, "%s", p.why);
            }
            free(host);
            free(key);
        }
        free(pdata);
    }
    free(body);

    if (!found) {
        if (!o->why[0])
            snprintf(o->why, sizeof o->why,
                     "no TAG_INTERNET_IOP profile in %u profile(s)", o->profiles);
        return;
    }
    o->ok = 1;
}

/* ── sockets ───────────────────────────────────────────────────────────────── */

static void set_deadline(int fd, double seconds)
{
    struct timeval tv;
    /* `tv_usec` is `suseconds_t`, which is `int` on Darwin and `long` on glibc.
     * Casting through the field's own type rather than through `long` is what
     * keeps `-Wconversion` quiet on both, and the fraction is bounded by
     * construction so nothing is lost either way. */
    tv.tv_sec = (time_t)seconds;
    tv.tv_usec = (suseconds_t)((seconds - (double)tv.tv_sec) * 1e6);
    setsockopt(fd, SOL_SOCKET, SO_RCVTIMEO, &tv, sizeof tv);
    setsockopt(fd, SOL_SOCKET, SO_SNDTIMEO, &tv, sizeof tv);
}

/* `%.*s` with HOST_IN_WHY, not a bare `%s`, in every message this function
   writes.

   `host` can be 255 characters and `why` is 256, so the fixed text plus a
   maximal host does not fit — GCC proves it once `dial` is inlined into its
   caller and refuses the build:

     c_peer.c:608:42: error: '%s' directive output may be truncated writing up
     to 255 bytes into a region of size 244 [-Werror=format-truncation=]

   `snprintf` truncates safely, so this was never a memory defect; it is a
   message that could silently lose its tail. Bounding the host says which part
   is allowed to be lost, and says it to the compiler as well as to the reader.

   **This never fired on macOS.** clang's fortify does not see through the
   inline the way glibc's does, and until 2026-08-28 `spikes/c_peer.sh` was in
   no harness group at all — `grep -c c_peer spikes/run_checks.sh` was 0 — so
   the peer had never been compiled on Linux. The group that found this was
   written the same day, and this was its first run. */
#define HOST_IN_WHY 180

static int dial(const char *host, unsigned port, double deadline, char *why, size_t whyn)
{
    char portstr[16];
    snprintf(portstr, sizeof portstr, "%u", port);

    struct addrinfo hints;
    memset(&hints, 0, sizeof hints);
    hints.ai_family = AF_UNSPEC;
    hints.ai_socktype = SOCK_STREAM;

    struct addrinfo *res = NULL;
    int rc = getaddrinfo(host, portstr, &hints, &res);
    if (rc != 0) {
        snprintf(why, whyn, "getaddrinfo(%.*s:%u): %s", HOST_IN_WHY, host, port,
                 gai_strerror(rc));
        return -1;
    }
    int fd = -1;
    for (struct addrinfo *a = res; a; a = a->ai_next) {
        fd = socket(a->ai_family, a->ai_socktype, a->ai_protocol);
        if (fd < 0)
            continue;
        set_deadline(fd, deadline);
        if (connect(fd, a->ai_addr, a->ai_addrlen) == 0)
            break;
        snprintf(why, whyn, "connect(%.*s:%u): %s", HOST_IN_WHY, host, port,
                 strerror(errno));
        close(fd);
        fd = -1;
    }
    freeaddrinfo(res);
    if (fd < 0 && !why[0])
        snprintf(why, whyn, "no address for %.*s:%u could be dialed", HOST_IN_WHY, host,
                 port);
    if (fd >= 0) {
        int one = 1;
        setsockopt(fd, IPPROTO_TCP, TCP_NODELAY, &one, sizeof one);
    }
    return fd;
}

static int write_all(int fd, const unsigned char *d, size_t n)
{
    size_t sent = 0;
    while (sent < n) {
        ssize_t k = send(fd, d + sent, n - sent, 0);
        if (k <= 0) {
            if (k < 0 && errno == EINTR)
                continue;
            return -1;
        }
        sent += (size_t)k;
    }
    return 0;
}

/* Reads exactly n octets, or reports how far it got. 0 on success. */
static int read_exactly(int fd, unsigned char *d, size_t n, size_t *got)
{
    *got = 0;
    while (*got < n) {
        ssize_t k = recv(fd, d + *got, n - *got, 0);
        if (k == 0)
            return -1; /* the peer hung up */
        if (k < 0) {
            if (errno == EINTR)
                continue;
            return -2; /* error or deadline */
        }
        *got += (size_t)k;
    }
    return 0;
}

/* ── the GIOP message a client writes ──────────────────────────────────────── */

typedef struct {
    const char *op;
    const unsigned char *key;
    size_t key_len;
    int32_t longs[8];
    int nlongs;
    const char *str;      /* NULL if the call takes no string */
    unsigned major, minor;
    int little;
    const char *magic;    /* "GIOP" unless a refusal is being provoked */
    int response_expected;
} call_t;

static void build_request(buf_t *m, const call_t *c, uint32_t request_id)
{
    buf_init(m, c->little);

    /* The header goes into the same buffer that alignment is computed in, so
     * offset zero — the magic — IS the alignment origin. §15.4.1. */
    buf_raw(m, c->magic, 4);
    buf_u8(m, c->major);
    buf_u8(m, c->minor);
    /* GIOP 1.0 calls octet 6 `boolean byte_order`; 1.1 onward call it `flags`
     * with bit 0 carrying the same meaning. One octet either way. */
    buf_u8(m, c->little ? FLAG_LITTLE_ENDIAN : 0);
    buf_u8(m, MSG_REQUEST);
    buf_u32(m, 0); /* message_size, patched once the body is known */

    if (c->major == 1 && c->minor >= 2) {
        /* RequestHeader_1_2: id, response_flags, three reserved octets, a
         * TargetAddress union whose KeyAddr arm is discriminant 0, the operation,
         * then the service context list. */
        buf_u32(m, request_id);
        buf_u8(m, c->response_expected ? 0x03 : 0x00);
        buf_u8(m, 0);
        buf_u8(m, 0);
        buf_u8(m, 0);
        buf_u16(m, 0); /* TargetAddress: KeyAddr */
        buf_octets(m, c->key, c->key_len);
        buf_string(m, c->op);
        buf_u32(m, 0); /* no service contexts */
        /* §15.4.2: in 1.2 the request body starts on an 8-octet boundary,
         * counted from the first octet of the message header. */
        buf_align(m, 8);
    } else {
        /* RequestHeader_1_0 / _1_1: the service context list comes FIRST, and
         * 1.1 inserts three reserved octets after `response_expected`. There is
         * no 8-alignment of the body in either. */
        buf_u32(m, 0); /* no service contexts */
        buf_u32(m, request_id);
        buf_u8(m, c->response_expected ? 1 : 0);
        if (c->major == 1 && c->minor == 1) {
            buf_u8(m, 0);
            buf_u8(m, 0);
            buf_u8(m, 0);
        }
        buf_octets(m, c->key, c->key_len);
        buf_string(m, c->op);
        buf_u32(m, 0); /* requesting_principal, an empty sequence<octet> */
    }

    for (int i = 0; i < c->nlongs; i++)
        buf_i32(m, c->longs[i]);
    if (c->str)
        buf_string(m, c->str);

    /* message_size counts everything after the 12-octet header. */
    uint32_t size = (uint32_t)(m->len - GIOP_HEADER_LEN);
    unsigned char *p = m->d + 8;
    if (c->little) {
        p[0] = (unsigned char)(size & 0xFF);
        p[1] = (unsigned char)((size >> 8) & 0xFF);
        p[2] = (unsigned char)((size >> 16) & 0xFF);
        p[3] = (unsigned char)((size >> 24) & 0xFF);
    } else {
        p[0] = (unsigned char)((size >> 24) & 0xFF);
        p[1] = (unsigned char)((size >> 16) & 0xFF);
        p[2] = (unsigned char)((size >> 8) & 0xFF);
        p[3] = (unsigned char)(size & 0xFF);
    }
}

/* ── a whole GIOP message, read off the wire ───────────────────────────────── */

typedef struct {
    int present;
    int truncated;
    char why[200];
    unsigned char header[GIOP_HEADER_LEN];
    unsigned char *body;
    size_t body_len;
    unsigned major, minor, flags, type;
    int little;            /* READ from bit 0 of octet 6, never assumed */
    int more_fragments;
    uint32_t size;
} msg_t;

static void msg_free(msg_t *m)
{
    free(m->body);
    m->body = NULL;
}

static void read_message(int fd, msg_t *m)
{
    memset(m, 0, sizeof *m);
    size_t got = 0;
    int rc = read_exactly(fd, m->header, GIOP_HEADER_LEN, &got);
    if (rc != 0) {
        m->present = 0;
        m->truncated = got > 0;
        snprintf(m->why, sizeof m->why,
                 rc == -1 ? "the peer hung up after %zu of 12 header octets"
                          : "no header: %zu of 12 octets, %s",
                 got, rc == -1 ? "" : strerror(errno));
        return;
    }
    m->present = 1;
    m->major = m->header[4];
    m->minor = m->header[5];
    m->flags = m->header[6];
    m->type = m->header[7];

    /* THIS is the observation the acceptance suite counts: §15.4.1's flag byte
     * of what the peer actually wrote, not a belief about the peer's language. */
    m->little = (m->flags & FLAG_LITTLE_ENDIAN) != 0;
    m->more_fragments = (m->flags & FLAG_MORE_FRAGMENTS) != 0;

    const unsigned char *s = m->header + 8;
    m->size = m->little ? ((uint32_t)s[0] | ((uint32_t)s[1] << 8) |
                           ((uint32_t)s[2] << 16) | ((uint32_t)s[3] << 24))
                        : (((uint32_t)s[0] << 24) | ((uint32_t)s[1] << 16) |
                           ((uint32_t)s[2] << 8) | (uint32_t)s[3]);

    if (m->size > (32u << 20)) {
        m->truncated = 1;
        snprintf(m->why, sizeof m->why,
                 "a declared message_size of %u is larger than this fixture reads",
                 (unsigned)m->size);
        return;
    }
    /* The message buffer holds the header too, because CDR offsets inside a
     * GIOP message are counted from the header's first octet. */
    m->body = malloc(GIOP_HEADER_LEN + (size_t)m->size + 1);
    if (!m->body) {
        fprintf(stderr, "c_peer: out of memory reading a %u octet message\n",
                (unsigned)m->size);
        exit(EXIT_FAILED);
    }
    memcpy(m->body, m->header, GIOP_HEADER_LEN);
    m->body_len = GIOP_HEADER_LEN + (size_t)m->size;
    if (m->size) {
        rc = read_exactly(fd, m->body + GIOP_HEADER_LEN, m->size, &got);
        if (rc != 0) {
            m->truncated = 1;
            snprintf(m->why, sizeof m->why,
                     "the body stopped at %zu of %u octets", got, (unsigned)m->size);
            m->body_len = GIOP_HEADER_LEN + got;
        }
    }
}

/* ── what a Reply says, decoded ────────────────────────────────────────────── */

typedef struct {
    uint32_t request_id;
    uint32_t status;
    int contexts;
    char *exception_id;      /* set for USER_ and SYSTEM_EXCEPTION */
    uint32_t minor_code;
    uint32_t completed;
    int have_long;
    int32_t result_long;
    char *result_string;
    char *forward_ior_type;  /* set for LOCATION_FORWARD */
    int decoded;
    char why[200];
} decoded_reply_t;

static void decode_reply(const msg_t *m, const char *expect, decoded_reply_t *out)
{
    memset(out, 0, sizeof *out);
    rdr_t r;
    rdr_init(&r, m->body, m->body_len, m->little);
    r.p = GIOP_HEADER_LEN;

    if (m->major == 1 && m->minor >= 2) {
        out->request_id = rdr_u32(&r);
        out->status = rdr_u32(&r);
        out->contexts = (int)rdr_u32(&r);
        for (int i = 0; i < out->contexts && !r.bad; i++) {
            rdr_u32(&r);
            size_t n = 0;
            free(rdr_octets(&r, &n));
        }
        rdr_align(&r, 8); /* §15.4.3, from the header's first octet */
    } else {
        out->contexts = (int)rdr_u32(&r);
        for (int i = 0; i < out->contexts && !r.bad; i++) {
            rdr_u32(&r);
            size_t n = 0;
            free(rdr_octets(&r, &n));
        }
        out->request_id = rdr_u32(&r);
        out->status = rdr_u32(&r);
    }

    if (r.bad) {
        snprintf(out->why, sizeof out->why, "%s", r.why);
        return;
    }

    if (out->status == REPLY_SYSTEM_EXCEPTION) {
        /* §15.4.3.2 SystemExceptionReplyBody. */
        out->exception_id = rdr_string(&r);
        out->minor_code = rdr_u32(&r);
        out->completed = rdr_u32(&r);
    } else if (out->status == REPLY_USER_EXCEPTION) {
        /* The repository id is the first thing in the body; the members after
         * it need the contract to decode and this peer does not have one. */
        out->exception_id = rdr_string(&r);
    } else if (out->status == REPLY_LOCATION_FORWARD ||
               out->status == REPLY_LOCATION_FORWARD_PERM) {
        /* §15.4.3.1: the body is an `IOR`, marshalled INLINE in the reply's own
         * CDR stream — not an encapsulation, so it carries no byte-order octet
         * of its own and no fresh alignment origin. Its first member is the
         * type id, which is as far as this peer reads it. */
        out->forward_ior_type = rdr_string(&r);
    } else if (out->status == REPLY_NO_EXCEPTION) {
        if (strcmp(expect, "long") == 0) {
            out->result_long = rdr_i32(&r);
            out->have_long = !r.bad;
        } else if (strcmp(expect, "string") == 0) {
            out->result_string = rdr_string(&r);
        } else if (strcmp(expect, "any") == 0) {
            /* Read whatever fits as a long, without claiming to know the shape. */
            if (r.p + 4 <= r.len) {
                out->result_long = rdr_i32(&r);
                out->have_long = !r.bad;
            }
        }
        /* "void" reads nothing, which is the honest thing for a `void` op. */
    }

    if (r.bad)
        snprintf(out->why, sizeof out->why, "%s", r.why);
    out->decoded = !r.bad;
}

static void free_reply(decoded_reply_t *d)
{
    free(d->exception_id);
    free(d->result_string);
    free(d->forward_ior_type);
}

static const char *type_name(unsigned t)
{
    switch (t) {
    case MSG_REQUEST: return "Request";
    case MSG_REPLY: return "Reply";
    case MSG_CANCEL_REQUEST: return "CancelRequest";
    case MSG_LOCATE_REQUEST: return "LocateRequest";
    case MSG_LOCATE_REPLY: return "LocateReply";
    case MSG_CLOSE_CONNECTION: return "CloseConnection";
    case MSG_MESSAGE_ERROR: return "MessageError";
    case MSG_FRAGMENT: return "Fragment";
    default: return "unknown";
    }
}

static const char *status_name(uint32_t s)
{
    switch (s) {
    case REPLY_NO_EXCEPTION: return "NO_EXCEPTION";
    case REPLY_USER_EXCEPTION: return "USER_EXCEPTION";
    case REPLY_SYSTEM_EXCEPTION: return "SYSTEM_EXCEPTION";
    case REPLY_LOCATION_FORWARD: return "LOCATION_FORWARD";
    case REPLY_LOCATION_FORWARD_PERM: return "LOCATION_FORWARD_PERM";
    case REPLY_NEEDS_ADDRESSING_MODE: return "NEEDS_ADDRESSING_MODE";
    default: return "unknown";
    }
}

static const char *completed_name(uint32_t c)
{
    switch (c) {
    case 0: return "COMPLETED_YES";
    case 1: return "COMPLETED_NO";
    case 2: return "COMPLETED_MAYBE";
    default: return "unknown";
    }
}

/* ── publishing, for the server role ──────────────────────────────────────── */

/* A wait loop that can read a half-written file reports a phantom failure, so
 * the file appears complete or not at all. CLAUDE.md's harness rules. */
static int publish(const char *path, const char *text)
{
    char tmp[1024];
    snprintf(tmp, sizeof tmp, "%s.partial", path);
    FILE *f = fopen(tmp, "w");
    if (!f)
        return -1;
    fputs(text, f);
    if (text[0] == '\0' || text[strlen(text) - 1] != '\n')
        fputc('\n', f);
    if (fclose(f) != 0)
        return -1;
    return rename(tmp, path);
}

/* ── the client role ──────────────────────────────────────────────────────── */

typedef struct {
    const char *role;
    const char *ior_file;
    const char *ior_text;
    const char *port_file;
    const char *op;
    const char *expect;
    const char *magic;
    const char *object_key_hex;
    const char *arg_string;
    int32_t longs[8];
    int nlongs;
    int little;         /* the order WE write */
    unsigned major, minor;
    int requests;
    double deadline;
    int no_response;
} opts_t;

static int run_client(const opts_t *o)
{
    char iorbuf[65536];
    const char *text = o->ior_text;
    if (!text) {
        FILE *f = fopen(o->ior_file, "r");
        if (!f) {
            fprintf(stderr, "c_peer: cannot read --ior-file %s: %s\n",
                    o->ior_file, strerror(errno));
            return EXIT_ABSENT; /* its fixture is absent */
        }
        size_t n = fread(iorbuf, 1, sizeof iorbuf - 1, f);
        iorbuf[n] = '\0';
        fclose(f);
        text = iorbuf;
    }

    ior_t ref;
    ior_parse(&ref, text);
    if (!ref.ok) {
        fprintf(stderr, "c_peer: the reference did not parse: %s\n", ref.why);
        free(ref.type_id);
        return EXIT_FAILED;
    }

    /* An override exists so a refusal can be provoked without inventing a
     * second fixture: an object key nobody activated is the shape that gets an
     * OBJECT_NOT_EXIST rather than a connection error. */
    unsigned char keybuf[512];
    const unsigned char *key = ref.key;
    size_t key_len = ref.key_len;
    if (o->object_key_hex) {
        size_t n = 0;
        unsigned char *k = hex_decode(o->object_key_hex, &n);
        if (!k || n > sizeof keybuf) {
            fprintf(stderr, "c_peer: --object-key-hex is not usable hex\n");
            free(k);
            free(ref.type_id);
            return EXIT_FAILED;
        }
        memcpy(keybuf, k, n);
        free(k);
        key = keybuf;
        key_len = n;
    }

    char why[256];
    why[0] = '\0';
    int fd = dial(ref.host, ref.port, o->deadline, why, sizeof why);
    if (fd < 0) {
        /* Reported, not judged: for some refusals a refused connection IS the
         * outcome the runner is looking for. Still exits 0, having run. */
        printf("{\"role\":\"client\",\"connected\":false,\"dial_error\":");
        json_escape(stdout, why);
        printf(",\"host\":");
        json_escape(stdout, ref.host);
        printf(",\"port\":%u}\n", ref.port);
        fflush(stdout);
        free(ref.type_id);
        return EXIT_RAN;
    }

    printf("{\"role\":\"client\"");
    printf(",\"ior\":{\"type_id\":");
    json_escape(stdout, ref.type_id ? ref.type_id : "");
    printf(",\"host\":");
    json_escape(stdout, ref.host);
    printf(",\"port\":%u,\"iiop\":\"%u.%u\",\"profiles\":%u,\"components\":%u",
           ref.port, ref.iiop_major, ref.iiop_minor, ref.profiles, ref.components);
    printf(",\"ior_encapsulation_endian\":\"%s\"", ref.ior_little ? "little" : "big");
    printf(",\"profile_encapsulation_endian\":\"%s\"", ref.profile_little ? "little" : "big");
    printf(",\"object_key\":");
    hex_print(stdout, ref.key, ref.key_len);
    printf("}");
    printf(",\"connected\":true");
    printf(",\"wrote\":{\"giop\":\"%u.%u\",\"magic\":", o->major, o->minor);
    json_escape(stdout, o->magic);
    printf(",\"order\":\"%s\",\"order_source\":\"written\",\"operation\":",
           o->little ? "little" : "big");
    json_escape(stdout, o->op);
    printf(",\"object_key\":");
    hex_print(stdout, key, key_len);
    printf(",\"long_args\":[");
    for (int i = 0; i < o->nlongs; i++)
        printf("%s%d", i ? "," : "", o->longs[i]);
    printf("]}");
    printf(",\"exchanges\":[");

    int rc = EXIT_RAN;
    for (int n = 0; n < o->requests; n++) {
        call_t c;
        memset(&c, 0, sizeof c);
        c.op = o->op;
        c.key = key;
        c.key_len = key_len;
        c.nlongs = o->nlongs;
        for (int i = 0; i < o->nlongs; i++)
            c.longs[i] = o->longs[i];
        c.str = o->arg_string;
        c.major = o->major;
        c.minor = o->minor;
        c.little = o->little;
        c.magic = o->magic;
        c.response_expected = !o->no_response;

        buf_t msg;
        build_request(&msg, &c, (uint32_t)(n + 1));

        printf("%s{\"request_id\":%d,\"request_octets\":%zu", n ? "," : "",
               n + 1, msg.len);

        if (write_all(fd, msg.d, msg.len) != 0) {
            printf(",\"write_error\":");
            json_escape(stdout, strerror(errno));
            printf("}");
            buf_free(&msg);
            break;
        }
        buf_free(&msg);

        if (o->no_response) {
            printf(",\"response_expected\":false}");
            continue;
        }

        msg_t reply;
        read_message(fd, &reply);
        if (!reply.present) {
            /* A server that closes rather than answering is a measurement, not
             * an error: it is what a refusal at the framing layer looks like. */
            printf(",\"reply\":null,\"peer_closed\":true,\"why\":");
            json_escape(stdout, reply.why);
            printf("}");
            msg_free(&reply);
            break;
        }

        printf(",\"reply\":{\"magic\":");
        json_escape(stdout, memcmp(reply.header, "GIOP", 4) == 0 ? "GIOP" : "not-GIOP");
        printf(",\"giop\":\"%u.%u\"", reply.major, reply.minor);
        printf(",\"message_type\":%u,\"message_type_name\":", reply.type);
        json_escape(stdout, type_name(reply.type));
        printf(",\"flag_byte\":%u", reply.flags);
        /* `observed`, in the acceptance suite's exact sense: read out of the
         * flag byte of what the peer wrote. Never `claimed` on this path. */
        printf(",\"order\":\"%s\",\"order_source\":\"observed\"",
               reply.little ? "little" : "big");
        printf(",\"more_fragments\":%s", reply.more_fragments ? "true" : "false");
        printf(",\"message_size\":%u,\"octets_read\":%zu",
               (unsigned)reply.size, reply.body_len);
        if (reply.truncated) {
            printf(",\"truncated\":true,\"why\":");
            json_escape(stdout, reply.why);
        }

        if (reply.type == MSG_REPLY && !reply.truncated) {
            decoded_reply_t d;
            decode_reply(&reply, o->expect, &d);
            printf(",\"request_id\":%u,\"reply_status\":%u,\"reply_status_name\":",
                   (unsigned)d.request_id, (unsigned)d.status);
            json_escape(stdout, status_name(d.status));
            printf(",\"service_contexts\":%d", d.contexts);
            if (d.exception_id) {
                printf(",\"exception_id\":");
                json_escape(stdout, d.exception_id);
                if (d.status == REPLY_SYSTEM_EXCEPTION) {
                    printf(",\"minor_code\":%u,\"completion_status\":%u,"
                           "\"completion_status_name\":",
                           (unsigned)d.minor_code, (unsigned)d.completed);
                    json_escape(stdout, completed_name(d.completed));
                }
            }
            if (d.forward_ior_type) {
                printf(",\"forward_type_id\":");
                json_escape(stdout, d.forward_ior_type);
            }
            if (d.have_long)
                printf(",\"result_long\":%d", d.result_long);
            if (d.result_string) {
                printf(",\"result_string\":");
                json_escape(stdout, d.result_string);
            }
            if (!d.decoded) {
                printf(",\"decode_error\":");
                json_escape(stdout, d.why);
            }
            free_reply(&d);
        }
        printf("}}");
        msg_free(&reply);
    }

    printf("]}\n");
    fflush(stdout);
    close(fd);
    free(ref.type_id);
    return rc;
}

/* ── the server role ──────────────────────────────────────────────────────── */

/* `IOR:<hex>` for one IIOP 1.2 profile, every octet built here. */
static char *stringified_ior(const char *host, unsigned port, int little)
{
    buf_t prof;
    encap_init(&prof, little);
    buf_u8(&prof, 1); /* IIOP major */
    buf_u8(&prof, 2); /* IIOP minor */
    buf_string(&prof, host);
    buf_u16(&prof, port);
    buf_octets(&prof, SERVER_OBJECT_KEY, strlen(SERVER_OBJECT_KEY));
    buf_u32(&prof, 0); /* no components */

    buf_t body;
    encap_init(&body, little);
    buf_string(&body, SERVER_TYPE_ID);
    buf_u32(&body, 1); /* one profile */
    buf_u32(&body, TAG_INTERNET_IOP);
    buf_octets(&body, prof.d, prof.len);

    size_t n = 4 + body.len * 2 + 1;
    char *out = malloc(n);
    if (!out) {
        fprintf(stderr, "c_peer: out of memory building an IOR\n");
        exit(EXIT_FAILED);
    }
    memcpy(out, "IOR:", 4);
    for (size_t i = 0; i < body.len; i++)
        snprintf(out + 4 + i * 2, 3, "%02x", body.d[i]);
    buf_free(&prof);
    buf_free(&body);
    return out;
}

typedef struct {
    uint32_t request_id;
    char *operation;
    unsigned char *key;
    size_t key_len;
    int32_t longs[8];
    int nlongs;
    int ok;
    char why[200];
} request_t;

static void decode_request(const msg_t *m, request_t *out)
{
    memset(out, 0, sizeof *out);
    rdr_t r;
    rdr_init(&r, m->body, m->body_len, m->little);
    r.p = GIOP_HEADER_LEN;

    if (m->major == 1 && m->minor >= 2) {
        out->request_id = rdr_u32(&r);
        rdr_u8(&r);          /* response_flags */
        rdr_u8(&r);
        rdr_u8(&r);
        rdr_u8(&r);          /* three reserved octets */
        unsigned target = rdr_u16(&r);
        if (target != 0) {
            snprintf(out->why, sizeof out->why,
                     "TargetAddress discriminant %u is not KeyAddr", target);
            return;
        }
        out->key = rdr_octets(&r, &out->key_len);
        out->operation = rdr_string(&r);
        uint32_t contexts = rdr_u32(&r);
        for (uint32_t i = 0; i < contexts && !r.bad; i++) {
            rdr_u32(&r);
            size_t n = 0;
            free(rdr_octets(&r, &n));
        }
        rdr_align(&r, 8);
    } else {
        uint32_t contexts = rdr_u32(&r);
        for (uint32_t i = 0; i < contexts && !r.bad; i++) {
            rdr_u32(&r);
            size_t n = 0;
            free(rdr_octets(&r, &n));
        }
        out->request_id = rdr_u32(&r);
        rdr_u8(&r); /* response_expected */
        if (m->major == 1 && m->minor == 1) {
            rdr_u8(&r);
            rdr_u8(&r);
            rdr_u8(&r);
        }
        out->key = rdr_octets(&r, &out->key_len);
        out->operation = rdr_string(&r);
        size_t pn = 0;
        free(rdr_octets(&r, &pn)); /* requesting_principal */
    }

    while (!r.bad && out->nlongs < 8 && r.p + 4 <= r.len) {
        size_t before = r.p;
        int32_t v = rdr_i32(&r);
        if (r.bad || r.p == before)
            break;
        out->longs[out->nlongs++] = v;
    }
    out->ok = !r.bad && out->operation != NULL;
    if (!out->ok && !out->why[0])
        snprintf(out->why, sizeof out->why, "%s", r.bad ? r.why : "no operation name");
}

static void free_request(request_t *q)
{
    free(q->operation);
    free(q->key);
}

/* A Reply carrying either a long or a SYSTEM_EXCEPTION, in `little`'s order,
 * chosen independently of the request's. */
static void build_reply(buf_t *m, uint32_t request_id, int little, unsigned major,
                        unsigned minor, uint32_t status, int32_t result,
                        const char *exception_id)
{
    buf_init(m, little);
    buf_raw(m, "GIOP", 4);
    buf_u8(m, major);
    buf_u8(m, minor);
    buf_u8(m, little ? FLAG_LITTLE_ENDIAN : 0);
    buf_u8(m, MSG_REPLY);
    buf_u32(m, 0);

    if (major == 1 && minor >= 2) {
        buf_u32(m, request_id);
        buf_u32(m, status);
        buf_u32(m, 0); /* no service contexts */
        buf_align(m, 8);
    } else {
        buf_u32(m, 0); /* no service contexts */
        buf_u32(m, request_id);
        buf_u32(m, status);
    }

    if (status == REPLY_SYSTEM_EXCEPTION) {
        buf_string(m, exception_id);
        buf_u32(m, 0); /* minor code */
        buf_u32(m, 1); /* COMPLETED_NO */
    } else if (status == REPLY_NO_EXCEPTION) {
        buf_i32(m, result);
    }

    uint32_t size = (uint32_t)(m->len - GIOP_HEADER_LEN);
    unsigned char *p = m->d + 8;
    if (little) {
        p[0] = (unsigned char)(size & 0xFF);
        p[1] = (unsigned char)((size >> 8) & 0xFF);
        p[2] = (unsigned char)((size >> 16) & 0xFF);
        p[3] = (unsigned char)((size >> 24) & 0xFF);
    } else {
        p[0] = (unsigned char)((size >> 24) & 0xFF);
        p[1] = (unsigned char)((size >> 16) & 0xFF);
        p[2] = (unsigned char)((size >> 8) & 0xFF);
        p[3] = (unsigned char)(size & 0xFF);
    }
}

static int run_server(const opts_t *o)
{
    int lfd = socket(AF_INET, SOCK_STREAM, 0);
    if (lfd < 0) {
        fprintf(stderr, "c_peer: socket: %s\n", strerror(errno));
        return EXIT_FAILED;
    }
    int one = 1;
    setsockopt(lfd, SOL_SOCKET, SO_REUSEADDR, &one, sizeof one);

    struct sockaddr_in addr;
    memset(&addr, 0, sizeof addr);
    addr.sin_family = AF_INET;
    addr.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
    addr.sin_port = 0;
    if (bind(lfd, (struct sockaddr *)&addr, sizeof addr) != 0 || listen(lfd, 4) != 0) {
        fprintf(stderr, "c_peer: bind/listen: %s\n", strerror(errno));
        close(lfd);
        return EXIT_FAILED;
    }
    socklen_t alen = sizeof addr;
    getsockname(lfd, (struct sockaddr *)&addr, &alen);
    unsigned port = ntohs(addr.sin_port);

    char *ior = stringified_ior("127.0.0.1", port, o->little);
    char portstr[16];
    snprintf(portstr, sizeof portstr, "%u", port);

    /* The IOR before the port file: the runner waits on the port file, so
     * publishing it last means everything it names already exists. */
    if (o->ior_file && publish(o->ior_file, ior) != 0) {
        fprintf(stderr, "c_peer: cannot publish --ior-file %s: %s\n",
                o->ior_file, strerror(errno));
        free(ior);
        close(lfd);
        return EXIT_FAILED;
    }
    if (o->port_file && publish(o->port_file, portstr) != 0) {
        fprintf(stderr, "c_peer: cannot publish --port-file %s: %s\n",
                o->port_file, strerror(errno));
        free(ior);
        close(lfd);
        return EXIT_FAILED;
    }

    printf("{\"role\":\"server\",\"port\":%u,\"ior\":", port);
    json_escape(stdout, ior);
    printf(",\"type_id\":");
    json_escape(stdout, SERVER_TYPE_ID);
    printf(",\"object_key\":");
    json_escape(stdout, SERVER_OBJECT_KEY);
    printf(",\"reply_order\":\"%s\",\"served\":[", o->little ? "little" : "big");
    fflush(stdout);
    free(ior);

    set_deadline(lfd, o->deadline);
    /* A blocking accept on a listener bound before the address was published.
     * CLAUDE.md's missed-accept rule is about a NON-blocking single accept,
     * which this is not; the deadline is what keeps it from hanging. */
    int fd = accept(lfd, NULL, NULL);
    if (fd < 0) {
        printf("],\"accepted\":false,\"why\":");
        json_escape(stdout, strerror(errno));
        printf("}\n");
        fflush(stdout);
        close(lfd);
        return EXIT_RAN;
    }
    set_deadline(fd, o->deadline);

    int served = 0;
    for (int n = 0; n < o->requests; n++) {
        msg_t m;
        read_message(fd, &m);
        if (!m.present) {
            msg_free(&m);
            break;
        }
        if (memcmp(m.header, "GIOP", 4) != 0 || m.type != MSG_REQUEST) {
            printf("%s{\"unexpected\":", served ? "," : "");
            json_escape(stdout, type_name(m.type));
            printf(",\"magic\":");
            hex_print(stdout, m.header, 4);
            printf("}");
            served++;
            msg_free(&m);
            break;
        }
        request_t q;
        decode_request(&m, &q);
        if (!q.ok) {
            printf("%s{\"decode_error\":", served ? "," : "");
            json_escape(stdout, q.why);
            printf("}");
            served++;
            free_request(&q);
            msg_free(&m);
            break;
        }

        int32_t sum = 0;
        for (int i = 0; i < q.nlongs; i++)
            sum += q.longs[i];

        buf_t out;
        int known = strcmp(q.operation, "add") == 0;
        build_reply(&out, q.request_id, o->little, m.major, m.minor,
                    known ? REPLY_NO_EXCEPTION : REPLY_SYSTEM_EXCEPTION, sum,
                    "IDL:omg.org/CORBA/BAD_OPERATION:1.0");

        printf("%s{\"request_id\":%u,\"operation\":", served ? "," : "",
               (unsigned)q.request_id);
        json_escape(stdout, q.operation);
        /* The order the CALLER wrote, read off its own flag byte. */
        printf(",\"request_order\":\"%s\",\"request_order_source\":\"observed\"",
               m.little ? "little" : "big");
        printf(",\"request_giop\":\"%u.%u\",\"long_args\":[", m.major, m.minor);
        for (int i = 0; i < q.nlongs; i++)
            printf("%s%d", i ? "," : "", q.longs[i]);
        printf("],\"answered\":%s", known ? "\"NO_EXCEPTION\"" : "\"BAD_OPERATION\"");
        if (known)
            printf(",\"result_long\":%d", sum);
        printf("}");
        fflush(stdout);
        served++;

        (void)write_all(fd, out.d, out.len);
        buf_free(&out);
        free_request(&q);
        msg_free(&m);
    }

    printf("],\"accepted\":true}\n");
    fflush(stdout);

    /* Held open until the caller hangs up, so a clean close cannot reach it as
     * a reset. An expired deadline here is not a failure. */
    unsigned char scratch;
    (void)recv(fd, &scratch, 1, 0);
    close(fd);
    close(lfd);
    return EXIT_RAN;
}

/* ── arguments ────────────────────────────────────────────────────────────── */

static void usage(void)
{
    fputs(
        "c_peer — a C program that speaks GIOP over a socket. Not a C ORB.\n"
        "\n"
        "  --role client|server        (default client)\n"
        "  --ior-file PATH             client: read the target's IOR here\n"
        "                              server: publish ours here\n"
        "  --ior IOR:<hex>             client: the target's IOR inline\n"
        "  --port-file PATH            server: publish the listening port here\n"
        "  --op NAME                   the operation to invoke (default add)\n"
        "  --arg-long N                append a `long` argument (repeatable)\n"
        "  --arg-string S              append a `string` argument\n"
        "  --expect long|string|void|any   how to decode a NO_EXCEPTION body\n"
        "  --request-endian little|big the order WE write (default little)\n"
        "  --reply-endian little|big   server: the order WE answer in\n"
        "  --giop 1.0|1.1|1.2          the version WE write (default 1.2)\n"
        "  --object-key-hex HEX        override the key taken from the IOR\n"
        "  --magic ABCD                override the four magic octets\n"
        "  --no-response               set response_flags to 0 and read nothing\n"
        "  --requests N                (default 1)\n"
        "  --deadline-s S              (default 20)\n"
        "\n"
        "Exit: 0 ran to the end, 1 did not, 2 its fixture is absent.\n",
        stderr);
}

int main(int argc, char **argv)
{
    opts_t o;
    memset(&o, 0, sizeof o);
    o.role = "client";
    o.op = "add";
    o.expect = "long";
    o.magic = "GIOP";
    o.major = 1;
    o.minor = 2;
    o.little = 1;
    o.requests = 1;
    o.deadline = 20.0;

    for (int i = 1; i < argc; i++) {
        const char *a = argv[i];
        const char *next = (i + 1 < argc) ? argv[i + 1] : NULL;
#define NEED(name)                                                             \
    do {                                                                       \
        if (!next) {                                                           \
            fprintf(stderr, "c_peer: %s needs a value\n", name);                \
            return EXIT_FAILED;                                                \
        }                                                                      \
    } while (0)
        if (strcmp(a, "--role") == 0) {
            NEED("--role");
            o.role = next;
            i++;
        } else if (strcmp(a, "--ior-file") == 0) {
            NEED("--ior-file");
            o.ior_file = next;
            i++;
        } else if (strcmp(a, "--ior") == 0) {
            NEED("--ior");
            o.ior_text = next;
            i++;
        } else if (strcmp(a, "--port-file") == 0) {
            NEED("--port-file");
            o.port_file = next;
            i++;
        } else if (strcmp(a, "--op") == 0) {
            NEED("--op");
            o.op = next;
            i++;
        } else if (strcmp(a, "--arg-long") == 0) {
            NEED("--arg-long");
            if (o.nlongs >= 8) {
                fprintf(stderr, "c_peer: this fixture carries at most 8 long arguments\n");
                return EXIT_FAILED;
            }
            o.longs[o.nlongs++] = (int32_t)strtol(next, NULL, 10);
            i++;
        } else if (strcmp(a, "--arg-string") == 0) {
            NEED("--arg-string");
            o.arg_string = next;
            i++;
        } else if (strcmp(a, "--expect") == 0) {
            NEED("--expect");
            o.expect = next;
            i++;
        } else if (strcmp(a, "--request-endian") == 0 ||
                   strcmp(a, "--reply-endian") == 0) {
            NEED(a);
            if (strcmp(next, "little") == 0)
                o.little = 1;
            else if (strcmp(next, "big") == 0)
                o.little = 0;
            else {
                fprintf(stderr, "c_peer: %s takes little or big, not %s\n", a, next);
                return EXIT_FAILED;
            }
            i++;
        } else if (strcmp(a, "--giop") == 0) {
            NEED("--giop");
            unsigned mj = 0, mn = 0;
            if (sscanf(next, "%u.%u", &mj, &mn) != 2) {
                fprintf(stderr, "c_peer: --giop takes major.minor, not %s\n", next);
                return EXIT_FAILED;
            }
            o.major = mj;
            o.minor = mn;
            i++;
        } else if (strcmp(a, "--object-key-hex") == 0) {
            NEED("--object-key-hex");
            o.object_key_hex = next;
            i++;
        } else if (strcmp(a, "--magic") == 0) {
            NEED("--magic");
            if (strlen(next) != 4) {
                fprintf(stderr, "c_peer: --magic takes exactly four octets\n");
                return EXIT_FAILED;
            }
            o.magic = next;
            i++;
        } else if (strcmp(a, "--no-response") == 0) {
            o.no_response = 1;
        } else if (strcmp(a, "--requests") == 0) {
            NEED("--requests");
            o.requests = (int)strtol(next, NULL, 10);
            i++;
        } else if (strcmp(a, "--deadline-s") == 0) {
            NEED("--deadline-s");
            o.deadline = strtod(next, NULL);
            i++;
        } else if (strcmp(a, "--help") == 0 || strcmp(a, "-h") == 0) {
            usage();
            return EXIT_RAN;
        } else {
            fprintf(stderr, "c_peer: unknown argument %s\n", a);
            usage();
            return EXIT_FAILED;
        }
#undef NEED
    }

    if (o.requests < 0)
        o.requests = 0;

    if (strcmp(o.role, "client") == 0) {
        if (!o.ior_file && !o.ior_text) {
            fprintf(stderr, "c_peer: --role client needs --ior-file or --ior\n");
            return EXIT_FAILED;
        }
        return run_client(&o);
    }
    if (strcmp(o.role, "server") == 0)
        return run_server(&o);

    fprintf(stderr, "c_peer: --role takes client or server, not %s\n", o.role);
    return EXIT_FAILED;
}
