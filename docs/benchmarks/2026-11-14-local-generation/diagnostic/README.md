# Additional diagnostic, excluded from primary results

After the 36 primary observations were complete, `commit_select_partial` was
rerun using 27B medium thinking to retain the full error chain. It reproduced:

`Invalid AI recommendation JSON: invalid type: string "1", expected usize at line 1 column 16`

The early single-case filter covered the selection loop but accidentally let
the three message cases run too. The runner correctly rejected the expected
observation count and did not write a completed diagnostic manifest. The filter
was fixed for future use. All four observations in this subdirectory are extra
diagnostics; none replace or contribute to the primary scores or latencies.

The primary raw JSONL files remain unchanged. Original case source snapshots
match the hashes saved before the initial run in the parent manifest.
