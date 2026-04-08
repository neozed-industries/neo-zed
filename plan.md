# Code Review

## Verbatim Review Items

### 1. Broken Cancellation Check Mid-Archive (Correctness)
In `sidebar.rs`'s `archive_worktree` loop, you check for cancellation mid-archive like this:
```rust
// Check for cancellation before each root
if cancel_rx.try_recv().is_ok() {
    // ...
}
```
This will never trigger. The sender `cancel_tx` is never sent a message; it is simply dropped when `ThreadMetadataStore::unarchive` removes it from `in_flight_archives`. When a channel's sender is dropped, `try_recv()` returns `Err(smol::channel::TryRecvError::Closed)`. Because it returns an `Err`, `is_ok()` evaluates to `false`. Therefore, the loop will fail to abort if the user clicks "Unarchive" while archiving is in progress.

**Suggestion:**
Change the condition to check if the channel is closed:
```rust
if cancel_rx.is_closed() {
    // ...
}
```

### 2. Incomplete Worktree Linking for Multi-Worktree Threads (Correctness)
In `persist_worktree_state` (inside `thread_worktree_archive.rs`), you link other threads to the archived worktree using `all_session_ids_for_path(folder_paths)`. The problem is that `folder_paths` here is the *exact* `PathList` of the archiving thread. 

If Thread A has `["/a", "/b"]` and Thread B has just `["/a"]`:
1. Thread B is archived first. It doesn't archive the worktree because `path_is_referenced_by_other_unarchived_threads` sees Thread A still using it.
2. Thread A is archived. It archives both `/a` and `/b`. 
3. When it links threads to `/a`'s archive record, it looks for threads with the exact `PathList` `["/a", "/b"]`. Thread B has `["/a"]`, so it is **not** linked.
4. When Thread B is later unarchived, it will fail to find its worktree backup.

**Suggestion:**
Instead of matching the exact `PathList`, iterate over all threads in the store and link any thread whose `folder_paths` *contains* the path of the worktree currently being archived (`root.root_path`).
```rust
let session_ids: Vec<acp::SessionId> = store.read_with(cx, |store, _cx| {
    store
        .entries()
        .filter(|thread| thread.folder_paths.paths().iter().any(|p| p.as_path() == root.root_path))
        .map(|thread| thread.session_id.clone())
        .collect()
});
```

### 3. Permanent Leak of Git Refs & DB Records on Thread Deletion (Brittleness & Performance)
When a thread is permanently deleted (e.g. by pressing Backspace or clicking the trash icon in the Archive view), it calls `ThreadMetadataStore::delete`, which deletes the thread from the `sidebar_threads` table. 

However, it completely ignores the `archived_git_worktrees` and `thread_archived_worktrees` tables. Crucially, the git refs (e.g., `refs/archived-worktrees/<id>`) are left in the main repository forever. This prevents git from ever garbage-collecting the WIP commits and their potentially large file blobs, permanently leaking disk space.

**Suggestion:**
In `ThreadMetadataStore::delete` (or a new async method orchestrating the deletion), after removing the thread from `sidebar_threads`, fetch its associated `archived_git_worktrees`. Remove the mapping in `thread_archived_worktrees`. For any archived worktree that is no longer referenced by *any* thread, you must:
1. Delete its DB row in `archived_git_worktrees`.
2. Delete the git ref via `find_or_create_repository` + `repo.delete_ref(...)`.

### 4. Silently Discarding Errors on Fallible Operations (Maintainability)
The Zed project `.rules` explicitly state: *"Never silently discard errors with `let _ =` on fallible operations."* 

This rule is violated extensively in `thread_worktree_archive.rs` during rollbacks and cleanup (e.g., lines 250, 303, 318, 344, 361, 392, 429, 477, 486, 649, 654). While it is correct to not halt a rollback if a single step fails, the errors should still be logged for visibility to aid in debugging.

**Suggestion:**
Since many of these are `oneshot::Receiver<Result<()>>`, you can handle them cleanly like this:
```rust
rx.await.ok().and_then(|r| r.log_err());
```
Or, if you want custom error contexts:
```rust
if let Err(e) = rx.await { 
    log::error!("rollback failed: {e:#}"); 
}
```

### 5. Silent Task Cancellation in `remove_root_after_worktree_removal` (Brittleness)
In `remove_root_after_worktree_removal`, you await a list of tasks in a loop:
```rust
for task in release_tasks {
    task.await?;
}
```
If the first task errors out, the function returns early. Because Zed `Task`s cancel when dropped, the remaining `wait_for_worktree_release` tasks are instantly canceled. This might be fine because `project.remove_worktree` was already called synchronously, but using `futures::future::try_join_all` would be a more idiomatic way to await them all and handle errors cleanly, or simply logging the error and continuing to wait for the others.

**Suggestion:**
Consider logging the error and continuing to wait for the rest to ensure all projects actually release the worktree before proceeding to delete it from disk:
```rust
for task in release_tasks {
    if let Err(e) = task.await {
        log::error!("Failed waiting for worktree release: {e:#}");
    }
}
```

---

## Plan to Address Issues

### 1. Fix Broken Cancellation Check
- **File:** `crates/sidebar/src/sidebar.rs`
- **Action:** Update the `if cancel_rx.try_recv().is_ok()` check in `Sidebar::archive_worktree` to use `if cancel_rx.is_closed()`. This correctly detects when the sender is dropped by the unarchiving flow.

### 2. Fix Incomplete Worktree Linking
- **File:** `crates/agent_ui/src/thread_worktree_archive.rs`
- **Action:** In `persist_worktree_state`, replace the call to `store.all_session_ids_for_path(folder_paths)` with an iteration over all `store.entries()`. Filter for any threads where `folder_paths.paths()` contains the currently-archiving `root.root_path`. Collect and return these `session_id`s so they are all correctly linked to the archived worktree record.

### 3. Prevent Git Ref & DB Leaks on Thread Deletion
- **Files:** `crates/agent_ui/src/thread_metadata_store.rs`, `crates/agent_ui/src/threads_archive_view.rs` (and potentially `thread_history_view.rs`)
- **Action:** 
  1. Add a method to `ThreadMetadataStore` or `thread_worktree_archive.rs` to handle "deep deletion" of a thread.
  2. This method will query the DB for all `ArchivedGitWorktree` entries linked to the thread being deleted.
  3. It will delete the mapping from `thread_archived_worktrees`.
  4. For each worktree that now has exactly 0 threads mapped to it, delete the row from `archived_git_worktrees` and use the git API (via `find_or_create_repository`) to delete the archived-worktree git ref (`refs/archived-worktrees/<id>`).
  5. Update the UI actions that currently call the shallow `ThreadMetadataStore::delete` to call this new deep cleanup method.

### 4. Remove Silent Discards of Fallible Operations
- **File:** `crates/agent_ui/src/thread_worktree_archive.rs`
- **Action:** Scan for all `let _ = ...` instances where fallible git operations (like resets, branch creation, or branch deletion) occur during rollbacks or fallbacks. Replace them with proper `.log_err()` chains or explicit `if let Err(e) = ...` logging statements to comply with Zed's `.rules` file and improve debuggability.

### 5. Ensure All Worktree Release Tasks Complete
- **File:** `crates/agent_ui/src/thread_worktree_archive.rs`
- **Action:** In the `remove_root_after_worktree_removal` function, change the `for` loop that `.await?`s release tasks. Modify it to await every task and log any errors that occur (`if let Err(error) = task.await { log::error!(...); }`), preventing the early return from silently dropping/canceling the remaining await tasks.