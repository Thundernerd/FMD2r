# Run module hooks on blocking worker threads, not async coroutines

The FMD2r server is async (tokio), but module hooks don't run on it. Each job (a download task, each of its page slots, an update run's workers, a favorites check, an interactive request) gets its own OS thread with its own Lua state, as upstream does. Host API calls like `HTTP.GET`, `sleep` and `fmd.subprocess` block that thread and wait on the async stack underneath. The async/sync boundary sits in one place, the Host API, and upstream's state-per-thread behaviour comes for free: globals and `require`d tables persist between hooks within a job, the state is rebuilt on a module switch, and the anti-bot bypass runs inside the worker's own state.

## Considered Options

- **Async hooks via mlua coroutines**: fewer threads, but every blocking Host API call has to yield across `pcall`, XPath callbacks, `require` loaders and the JS bridge. Some of those boundaries can't yield in Lua 5.4, and upstream's per-thread state semantics would have to be emulated instead of mirrored.

## Consequences

Thread count is bounded by the concurrency ceilings (8 tasks × 32 threads per task, plus the other job limits). Cancellation is cooperative first (`HTTP.Terminated`, early wake-up of blocking host calls), then enforced by a Lua interrupt after a grace period, so a stuck hook can't pin a thread forever.
