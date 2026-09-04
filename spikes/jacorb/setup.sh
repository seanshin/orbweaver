#!/usr/bin/env bash
# Fetches the JacORB interop fixture and generates its Java stubs.
#
# TEST FIXTURE. JacORB is LGPL and GlassFish/JBoss RMI-IIOP is EPL/LGPL; none of
# it is linked into Orbweaver. These processes speak to us over TCP using the
# published GIOP specification, which is why a second peer costs a download
# rather than a licence. See docs/PLAN.md §10.
#
# Needs JDK 21. JDK 24+ removed java.applet.Applet, which JacORB 3.9's
# ORB.init signature still references, so it will not even compile there.
#
# `--jars-only` stops after the download. The differential conformance check
# uses JacORB's IDL compiler as a second oracle and needs no wire peer, so it
# has no reason to pay for stub generation or a Java compile.
set -euo pipefail
cd "$(dirname "$0")"

JARS_ONLY=0
[ "${1:-}" = --jars-only ] && JARS_ONLY=1

JAVA_HOME_21=${JAVA_HOME_21:-/opt/homebrew/opt/openjdk@21/libexec/openjdk.jdk/Contents/Home}
[ -x "$JAVA_HOME_21/bin/javac" ] || { echo "need JDK 21 at $JAVA_HOME_21 (brew install openjdk@21)"; exit 2; }
export PATH="$JAVA_HOME_21/bin:$PATH"

mkdir -p lib gen classes
M=https://repo1.maven.org/maven2
# `--retry` covers the transient 5xx/timeout class; a single 503 from the
# mirror once failed the whole interop job with curl exit 22 (CI 2026-08-18).
fetch() { [ -s "lib/$2" ] || curl -sfL --retry 3 --retry-delay 2 --max-time 120 -o "lib/$2" "$M/$1"; }

fetch org/jacorb/jacorb/3.9/jacorb-3.9.jar                                            jacorb.jar
fetch org/jacorb/jacorb-omgapi/3.9/jacorb-omgapi-3.9.jar                              jacorb-omgapi.jar
fetch org/jacorb/jacorb-idl-compiler/3.9/jacorb-idl-compiler-3.9.jar                  jacorb-idl-compiler.jar
fetch org/slf4j/slf4j-api/1.7.36/slf4j-api-1.7.36.jar                                 slf4j-api-1.7.36.jar
# JEP 320 removed javax.rmi.CORBA from the JDK and JacORB still needs it — the
# very migration this project exists to automate, met in our own test rig.
fetch org/jboss/spec/javax/rmi/jboss-rmi-api_1.0_spec/1.0.6.Final/jboss-rmi-api_1.0_spec-1.0.6.Final.jar jboss-rmi-api.jar

[ "$JARS_ONLY" -eq 0 ] || { echo "jacorb jars ready"; exit 0; }

CP="lib/jacorb.jar:lib/jacorb-omgapi.jar:lib/jacorb-idl-compiler.jar:lib/jboss-rmi-api.jar:lib/slf4j-api-1.7.36.jar"
java -cp "$CP" org.jacorb.idl.parser -d gen ../echo.idl
# wide.idl is the wchar contract (D010 B5): echo.idl has no wchar operation and
# every fixture implements echo.idl, so the single wide character got its own.
java -cp "$CP" org.jacorb.idl.parser -d gen ../wide.idl
javac -nowarn -cp "$CP" -d classes $(find gen -name '*.java') Client.java Client11.java Server.java \
      WideServer.java WideClient.java SslServer.java CsiServer.java
echo "jacorb fixture ready"
