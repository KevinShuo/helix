# AI commit file limit correction

The user confirmed fixing the screenshot's hard-coded 64-file rejection after
the investigation identified inconsistent checks and incorrect generation scope.

Use one validation function in the AI commit handler for both UI preparation and
handler entry points. Honor positive `max-files` values up to the existing UI
ceiling of 1024, retaining the default of 64. Recommendation counts all changed
files; message generation counts only staged files. Reject zero explicitly and
report the actual count and effective limit.

## Approaches

Increasing only the hard-coded value leaves two validators liable to diverge.
Removing all limits changes resource policy beyond this correction. A shared
validator restores the existing configuration contract with a bounded ceiling.
This is the implementation choice for the confirmed fix.

Keep complete diff batching, the 64-request ceiling, context and byte budgets,
manual review of incomplete coverage, and all explicit Git write requirements.
Raising the file limit does not guarantee that the aggregation fits the model.
No configuration files, running applications, commits, or deployments are changed.
