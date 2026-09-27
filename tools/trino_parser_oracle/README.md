# Trino parser oracle

Dev-only adapter for `io.trino:trino-parser:483`. It is not imported by the
library, included in wheels, or run by the default test suite.

Build explicitly with Maven and JDK 25, which matches the bytecode level of the
published Trino 483 parser artifact:

```bash
mvn -f tools/trino_parser_oracle/pom.xml package dependency:build-classpath \
  -Dmdep.outputFile=target/classpath.txt
```

Then run `python tools/run_trino_parser_oracle.py --oracle-dir
tools/trino_parser_oracle --input cases.json`. The input uses protocol schema 2
and contains `cases` with unique `id`, one supported `entry_point`, raw and
prepared SQL provenance, and expected parser status. Reports include per-input,
harness, class, dependency, and toolchain fingerprints. Parser rejection and
infrastructure failure are deliberately distinct states.
