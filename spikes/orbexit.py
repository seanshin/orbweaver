"""The one home for *how an omniORB fixture leaves*.

A fixture that unwinds into ``Py_Finalize`` races omniORB's own C++ threads:
the thread scavenger calls back into an interpreter that is being torn down.
It has crashed that way twice here, on two platforms, with two signals and one
cause — and the second time it **failed a run**:

* **macOS, SIGSEGV.** ``omnipyThreadScavenger::run_undetached`` ->
  ``_PyObject_Call`` -> ``_PyType_LookupStackRefAndVersion``, null deref, while
  the main thread sat in ``_Py_Finalize`` -> ``PyInterpreterState_Delete``.
  Python 3.14.6, omniORBpy 4.3.4. Recorded in commit ``e2918ec``, which added
  ``rc_says`` to the harness so a status of 128+N would at least *read* as a
  crash rather than as a failed measurement — and deliberately did not change
  any verdict, because nothing had made it fail a run in 60 attempts.

* **Linux, SIGABRT.** ``terminate called without an active exception``, CI run
  33126673869, in the event-channel pull leg — **after the script had printed
  its own ``PASS``**. The harness read 134 and reported a measurement that had
  succeeded as a failed one. That is the run that moved this from *make it
  legible* to *make it not happen*.

``leave()`` skips finalization. What it does **not** skip is the fixture's own
teardown: an ORB that must be shut down is shut down before this is called, and
nothing here is a substitute for that.

**Why the flush is not optional.** ``os._exit`` does not drain Python's
buffers, and the harness reads what these scripts *print* — several of them
decide a verdict in their output and not only in their status. Losing that
would trade a rare crash for a routine silence, which is the worse of the two.

**What this file is not.** It is not a reason to stop reading exit codes: the
status is passed through unchanged, so a fixture that failed still fails. And
it is not for scripts that do not create an ORB — ``coverage_tables.py`` and
``service_sweep.py`` matched an early, broader sweep and have no ``ORB_init``
at all, which is why they are not here.

*omniORB의 C++ 스레드는 파이썬의 finalization보다 오래 산다. 두 플랫폼에서 두
신호로 같은 원인이 두 번 터졌고, 두 번째는 **PASS를 찍은 뒤** 실행을 실패시켰다.
``leave()``는 finalization을 건너뛴다. 종료 코드는 그대로 통과시키므로 진짜 실패는
여전히 실패다. flush는 선택이 아니다 — 하네스는 이 스크립트들이 **찍은 것**을
읽는다.*
"""

import os
import sys


def leave(rc=0):
    """Flush, then leave without running interpreter finalization.

    ``rc`` is whatever ``main()`` returned: ``None`` means success, the way a
    Python function that falls off its end does, and any non-int truthy value
    is treated as failure rather than silently becoming 0.
    """
    for stream in (sys.stdout, sys.stderr):
        try:
            stream.flush()
        except Exception:  # noqa: BLE001 — a closed stream must not mask the verdict
            pass
    if rc is None:
        code = 0
    elif isinstance(rc, bool):
        code = 1 if rc else 0
    elif isinstance(rc, int):
        code = rc
    else:
        code = 1
    os._exit(code)
