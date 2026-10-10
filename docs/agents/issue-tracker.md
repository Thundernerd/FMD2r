# Issue tracker

Tickets are GitHub issues on this repo, labelled `ticket`. T01–T91 came from the old `docs/tickets/` folder (issues #132–#222) and keep their `T<nn>:` title prefix; newer tickets have plain titles. A ticket's "Blocked by" section lists the issues that must be closed first.

A PR implements exactly one ticket and closes it with `Closes #<n>` in its body. For `/code-review`, the spec is the issue (`gh issue view <n> --comments`), read together with `docs/plan.md` and `CODING_STANDARDS.md`.

A ticket with an `Owner: user` line at the top of its body is for the maintainer (decisions, sign-offs). Agents are never launched for it.
