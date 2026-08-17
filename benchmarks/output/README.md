# Benchmark output

This directory contains generated benchmark artifacts. A result set is suitable for comparison only
when its `environment.txt`, `server.txt`, workload settings, and Redis implementation are recorded
and the transaction and Lua variants were run independently with the same workload.

The current canonical result sets are:

- `20260816T134548Z` (`transaction`)
- `20260816T141050Z` (`lua`)

Both record clean source commit `0f9cf6215df780dffbc2306764367e76ecd3bfdd` and use the
Docker-internal topology, 4 load-generator threads, concurrency 16/64/256, a 5-second warmup
followed by a 30-second measurement, 3 runs per case, and a 10,000-key many-key workload. The
one-hour rate-limit window keeps this steady-state comparison away from a fixed-window expiry
boundary. Each result set contains 45 summary rows and records zero request errors.
