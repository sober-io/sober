# #055: Evolution Delete — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add permanent deletion of evolution events with artifact cleanup for active evolutions.

**Architecture:** New `delete` repo method + service method + API route + frontend buttons. Active deletions reuse existing `RevertEvolution` gRPC for artifact cleanup before DB delete.

**Tech Stack:** Rust (sqlx, axum, tonic), Svelte 5, TypeScript

---

### Task 1: Add `delete` to `EvolutionRepo` Trait and `PgEvolutionRepo`

**Files:**
- Modify: `backend/crates/sober-core/src/types/repo.rs:897-957`
- Modify: `backend/crates/sober-db/src/repos/evolution.rs`

- [ ] **Step 1: Add `delete` method to `EvolutionRepo` trait**

In `backend/crates/sober-core/src/types/repo.rs`, add after the `update_result` method (after line 933):

```rust
    /// Hard-deletes an evolution event by ID.
    fn delete(
        &self,
        id: EvolutionEventId,
    ) -> impl Future<Output = Result<(), AppError>> + Send;
```

- [ ] **Step 2: Implement `delete` in `PgEvolutionRepo`**

In `backend/crates/sober-db/src/repos/evolution.rs`, add after the `update_result` method (after line 180):

```rust
    async fn delete(&self, id: EvolutionEventId) -> Result<(), AppError> {
        let result = sqlx::query("DELETE FROM evolution_events WHERE id = $1")
            .bind(id.as_uuid())
            .execute(&self.pool)
            .await
            .map_err(|e| AppError::Internal(e.into()))?;

        if result.rows_affected() == 0 {
            return Err(AppError::NotFound("evolution event".into()));
        }

        Ok(())
    }
```

- [ ] **Step 3: Verify it compiles**

Run: `cd /home/harri/Projects/Repos/sober && cargo build -q -p sober-db 2>&1 | tail -5`
Expected: clean build (no errors)

- [ ] **Step 4: Commit**

```bash
git add backend/crates/sober-core/src/types/repo.rs backend/crates/sober-db/src/repos/evolution.rs
git commit -m "feat(db): add delete method to EvolutionRepo"
```

---

### Task 2: Add `delete_event` to `EvolutionService`

**Files:**
- Modify: `backend/crates/sober-api/src/services/evolution.rs`

- [ ] **Step 1: Add `validate_deletable` helper**

Add after the existing `validate_status_transition` function at the bottom of the file (after line 194):

```rust
/// Statuses from which an evolution event can be permanently deleted.
const DELETABLE_STATUSES: &[EvolutionStatus] = &[
    EvolutionStatus::Proposed,
    EvolutionStatus::Rejected,
    EvolutionStatus::Failed,
    EvolutionStatus::Reverted,
    EvolutionStatus::Active,
];

fn validate_deletable(status: &EvolutionStatus) -> Result<(), AppError> {
    if DELETABLE_STATUSES.contains(status) {
        Ok(())
    } else {
        Err(AppError::Validation(format!(
            "cannot delete evolution in '{}' status — wait for execution to complete",
            serde_json::to_string(status)
                .unwrap_or_default()
                .trim_matches('"'),
        )))
    }
}
```

- [ ] **Step 2: Add `delete_event` method to `EvolutionService`**

Add after the `update_event` method (after line 113):

```rust
    /// Permanently deletes an evolution event.
    ///
    /// For active evolutions, reverts artifacts via gRPC first.
    /// Blocked for approved/executing statuses.
    #[instrument(skip(self), fields(evolution.id = %id))]
    pub async fn delete_event(&self, id: EvolutionEventId) -> Result<(), AppError> {
        let repo = PgEvolutionRepo::new(self.db.clone());
        let event = repo.get_by_id(id).await?;

        validate_deletable(&event.status)?;

        // Active evolutions need artifact cleanup before deletion.
        if event.status == EvolutionStatus::Active {
            let mut client = self.agent_client.clone();
            let mut request = tonic::Request::new(proto::RevertEvolutionRequest {
                evolution_event_id: id.to_string(),
            });
            sober_core::inject_trace_context(request.metadata_mut());
            client
                .revert_evolution(request)
                .await
                .map_err(|e| AppError::Internal(e.into()))?;
        }

        repo.delete(id).await?;

        metrics::counter!("sober_evolution_deletes_total").increment(1);

        Ok(())
    }
```

- [ ] **Step 3: Add unit tests for `validate_deletable`**

Add to the existing `#[cfg(test)] mod tests` block at the bottom of the file:

```rust
    #[test]
    fn deletable_statuses() {
        assert!(validate_deletable(&EvolutionStatus::Proposed).is_ok());
        assert!(validate_deletable(&EvolutionStatus::Rejected).is_ok());
        assert!(validate_deletable(&EvolutionStatus::Failed).is_ok());
        assert!(validate_deletable(&EvolutionStatus::Reverted).is_ok());
        assert!(validate_deletable(&EvolutionStatus::Active).is_ok());
    }

    #[test]
    fn non_deletable_statuses() {
        assert!(validate_deletable(&EvolutionStatus::Approved).is_err());
        assert!(validate_deletable(&EvolutionStatus::Executing).is_err());
    }
```

- [ ] **Step 4: Run tests**

Run: `cd /home/harri/Projects/Repos/sober && cargo test -q -p sober-api -- services::evolution 2>&1 | tail -10`
Expected: all tests pass

- [ ] **Step 5: Commit**

```bash
git add backend/crates/sober-api/src/services/evolution.rs
git commit -m "feat(api): add delete_event to EvolutionService"
```

---

### Task 3: Add `DELETE /evolution/{id}` Route

**Files:**
- Modify: `backend/crates/sober-api/src/routes/evolution.rs`

- [ ] **Step 1: Add delete handler and wire route**

In `backend/crates/sober-api/src/routes/evolution.rs`, update the route for `"/evolution/{id}"` to include `delete`:

Change:
```rust
        .route("/evolution/{id}", get(get_event).patch(update_event))
```
To:
```rust
        .route("/evolution/{id}", get(get_event).patch(update_event).delete(delete_event))
```

Add the handler function after `update_event`:

```rust
async fn delete_event(
    State(state): State<Arc<AppState>>,
    RequireAdmin(_user): RequireAdmin,
    Path(id): Path<uuid::Uuid>,
) -> Result<axum::http::StatusCode, AppError> {
    state
        .evolution
        .delete_event(EvolutionEventId::from_uuid(id))
        .await?;
    Ok(axum::http::StatusCode::NO_CONTENT)
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cd /home/harri/Projects/Repos/sober && cargo build -q -p sober-api 2>&1 | tail -5`
Expected: clean build

- [ ] **Step 3: Run clippy**

Run: `cd /home/harri/Projects/Repos/sober && cargo clippy -q -p sober-api -- -D warnings 2>&1 | tail -5`
Expected: no warnings

- [ ] **Step 4: Commit**

```bash
git add backend/crates/sober-api/src/routes/evolution.rs
git commit -m "feat(api): add DELETE /evolution/{id} route"
```

---

### Task 4: Update sqlx Prepared Queries

**Files:**
- Modify: `backend/.sqlx/` (auto-generated)

- [ ] **Step 1: Rebuild Docker and prepare sqlx**

```bash
cd /home/harri/Projects/Repos/sober && docker compose up -d --build --quiet-pull 2>&1 | tail -15
```

- [ ] **Step 2: Run sqlx prepare**

```bash
cd /home/harri/Projects/Repos/sober/backend && cargo sqlx prepare 2>&1 | tail -5
```

- [ ] **Step 3: Commit**

```bash
git add backend/.sqlx/
git commit -m "chore(db): update sqlx prepared queries for evolution delete"
```

---

### Task 5: Add `delete` to Frontend Evolution Service

**Files:**
- Modify: `frontend/src/lib/services/evolution.ts`

- [ ] **Step 1: Add delete method**

In `frontend/src/lib/services/evolution.ts`, add after the `update` method (after line 19):

```typescript
	delete: (id: string) =>
		api<void>(`/evolution/${id}`, {
			method: 'DELETE'
		}),
```

Note: The API returns 204 No Content. The `api()` utility should handle empty responses. Check `frontend/src/lib/utils/api.ts` to verify — if it tries to parse JSON on 204, this will need a small adjustment (pass `{ raw: true }` or similar).

- [ ] **Step 2: Verify frontend compiles**

Run: `cd /home/harri/Projects/Repos/sober/frontend && pnpm check 2>&1 | tail -5`
Expected: no errors

- [ ] **Step 3: Commit**

```bash
git add frontend/src/lib/services/evolution.ts
git commit -m "feat(frontend): add evolution delete to API service"
```

---

### Task 6: Add Delete Button to Evolution Settings Page

**Files:**
- Modify: `frontend/src/routes/(app)/settings/evolution/+page.svelte`

- [ ] **Step 1: Add `deleteConfirmId` state and `deleteEvent` function**

In the `<script>` block, add after line 14 (`let actionInProgress = ...`):

```typescript
	let deleteConfirmId = $state<string | null>(null);
```

Add after the `retryEvent` function (after line 137):

```typescript
	async function deleteEvent(id: string) {
		actionInProgress = id;
		error = null;
		try {
			await evolutionService.delete(id);
			events = events.filter((e) => e.id !== id);
			deleteConfirmId = null;
		} catch (err) {
			error = err instanceof ApiError ? err.message : 'Failed to delete evolution';
		} finally {
			actionInProgress = null;
		}
	}
```

- [ ] **Step 2: Add Delete button to Pending Proposals section**

In the pending proposals section, add a Delete button after the Reject button (after line 352):

```svelte
								<button
									onclick={() => deleteEvent(event.id)}
									disabled={actionInProgress === event.id}
									class="rounded-md border border-red-300 px-3 py-1.5 text-sm text-red-700 hover:bg-red-50 disabled:opacity-50 dark:border-red-800 dark:text-red-400 dark:hover:bg-red-950"
								>
									Delete
								</button>
```

- [ ] **Step 3: Add Delete button to Active Evolutions section**

In the active evolutions section, add a Delete button with confirmation. Find the action buttons area for each event (the `<div class="flex shrink-0 gap-2">` block starting around line 440).

Replace the entire actions `<div>` block (lines 440-474) with:

```svelte
							<div class="flex shrink-0 gap-2">
								{#if event.status === 'failed'}
									<button
										onclick={() => retryEvent(event.id)}
										disabled={actionInProgress === event.id}
										class="rounded-md border border-amber-300 px-3 py-1.5 text-sm text-amber-700 hover:bg-amber-50 disabled:opacity-50 dark:border-amber-700 dark:text-amber-300 dark:hover:bg-amber-950"
									>
										Retry
									</button>
								{/if}
								{#if event.status === 'active'}
									{#if revertConfirmId === event.id}
										<button
											onclick={() => revertEvent(event.id)}
											disabled={actionInProgress === event.id}
											class="rounded-md bg-red-600 px-3 py-1.5 text-sm font-medium text-white hover:bg-red-500 disabled:opacity-50"
										>
											Confirm Revert
										</button>
										<button
											onclick={() => (revertConfirmId = null)}
											class="rounded-md border border-zinc-300 px-3 py-1.5 text-sm text-zinc-700 hover:bg-zinc-100 dark:border-zinc-700 dark:text-zinc-300 dark:hover:bg-zinc-800"
										>
											Cancel
										</button>
									{:else}
										<button
											onclick={() => (revertConfirmId = event.id)}
											class="rounded-md border border-red-300 px-3 py-1.5 text-sm text-red-700 hover:bg-red-50 dark:border-red-800 dark:text-red-400 dark:hover:bg-red-950"
										>
											Revert
										</button>
									{/if}
								{/if}
								{#if deleteConfirmId === event.id}
									<button
										onclick={() => deleteEvent(event.id)}
										disabled={actionInProgress === event.id}
										class="rounded-md bg-red-600 px-3 py-1.5 text-sm font-medium text-white hover:bg-red-500 disabled:opacity-50"
									>
										Confirm Delete
									</button>
									<button
										onclick={() => (deleteConfirmId = null)}
										class="rounded-md border border-zinc-300 px-3 py-1.5 text-sm text-zinc-700 hover:bg-zinc-100 dark:border-zinc-700 dark:text-zinc-300 dark:hover:bg-zinc-800"
									>
										Cancel
									</button>
								{:else if event.status !== 'approved' && event.status !== 'executing'}
									<button
										onclick={() => (deleteConfirmId = event.id)}
										class="rounded-md border border-zinc-300 px-3 py-1.5 text-sm text-zinc-500 hover:text-red-700 hover:border-red-300 hover:bg-red-50 disabled:opacity-50 dark:border-zinc-700 dark:text-zinc-400 dark:hover:text-red-400 dark:hover:border-red-800 dark:hover:bg-red-950"
									>
										Delete
									</button>
								{/if}
							</div>
```

- [ ] **Step 4: Verify frontend compiles**

Run: `cd /home/harri/Projects/Repos/sober/frontend && pnpm check 2>&1 | tail -5`
Expected: no errors

- [ ] **Step 5: Commit**

```bash
git add frontend/src/routes/\(app\)/settings/evolution/+page.svelte
git commit -m "feat(frontend): add delete button to evolution settings page"
```

---

### Task 7: Add Delete Button to Timeline Page

**Files:**
- Modify: `frontend/src/routes/(app)/settings/evolution/timeline/+page.svelte`

- [ ] **Step 1: Add delete state and function**

In the `<script>` block, add after line 24 (`let actionLoading = ...`):

```typescript
	let deleteConfirmId = $state<string | null>(null);
```

Add after the `updateStatus` function (after line 144):

```typescript
	async function deleteEvolution(id: string) {
		actionLoading = id;
		error = null;
		try {
			await evolutionService.delete(id);
			events = events.filter((e) => e.id !== id);
			deleteConfirmId = null;
		} catch (err) {
			error = err instanceof ApiError ? err.message : 'Failed to delete evolution';
		} finally {
			actionLoading = null;
		}
	}
```

- [ ] **Step 2: Add Delete button to timeline event actions**

Find the actions section in the timeline (the `{#if event.status === 'proposed' || ...}` block starting at line 356). Replace the entire actions block (lines 356-393) with:

```svelte
						<!-- Actions -->
						{#if event.status === 'proposed' || event.status === 'active' || event.status === 'failed' || event.status === 'rejected' || event.status === 'reverted'}
							<div
								class="flex items-center gap-2 border-t border-zinc-100 px-4 py-2 dark:border-zinc-800"
							>
								{#if event.status === 'proposed'}
									<button
										onclick={() => updateStatus(event.id, 'approved')}
										disabled={isActioning}
										class="rounded px-2.5 py-1 text-xs font-medium text-emerald-700 hover:bg-emerald-50 disabled:opacity-50 dark:text-emerald-400 dark:hover:bg-emerald-950"
									>
										{isActioning ? 'Approving...' : 'Approve'}
									</button>
									<button
										onclick={() => updateStatus(event.id, 'rejected')}
										disabled={isActioning}
										class="rounded px-2.5 py-1 text-xs font-medium text-red-700 hover:bg-red-50 disabled:opacity-50 dark:text-red-400 dark:hover:bg-red-950"
									>
										Reject
									</button>
								{:else if event.status === 'active'}
									<button
										onclick={() => updateStatus(event.id, 'reverted')}
										disabled={isActioning}
										class="rounded px-2.5 py-1 text-xs font-medium text-amber-700 hover:bg-amber-50 disabled:opacity-50 dark:text-amber-400 dark:hover:bg-amber-950"
									>
										{isActioning ? 'Reverting...' : 'Revert'}
									</button>
								{:else if event.status === 'failed'}
									<button
										onclick={() => updateStatus(event.id, 'approved')}
										disabled={isActioning}
										class="rounded px-2.5 py-1 text-xs font-medium text-sky-700 hover:bg-sky-50 disabled:opacity-50 dark:text-sky-400 dark:hover:bg-sky-950"
									>
										{isActioning ? 'Retrying...' : 'Retry'}
									</button>
								{/if}
								<!-- Delete (available for all deletable statuses) -->
								{#if deleteConfirmId === event.id}
									<button
										onclick={() => deleteEvolution(event.id)}
										disabled={isActioning}
										class="rounded px-2.5 py-1 text-xs font-medium bg-red-600 text-white hover:bg-red-500 disabled:opacity-50"
									>
										Confirm Delete
									</button>
									<button
										onclick={() => (deleteConfirmId = null)}
										class="rounded px-2.5 py-1 text-xs font-medium text-zinc-600 hover:bg-zinc-100 dark:text-zinc-400 dark:hover:bg-zinc-800"
									>
										Cancel
									</button>
								{:else}
									<button
										onclick={() => (deleteConfirmId = event.id)}
										disabled={isActioning}
										class="rounded px-2.5 py-1 text-xs font-medium text-zinc-500 hover:text-red-700 hover:bg-red-50 disabled:opacity-50 dark:text-zinc-400 dark:hover:text-red-400 dark:hover:bg-red-950"
									>
										Delete
									</button>
								{/if}
							</div>
						{/if}
```

- [ ] **Step 3: Verify frontend compiles**

Run: `cd /home/harri/Projects/Repos/sober/frontend && pnpm check 2>&1 | tail -5`
Expected: no errors

- [ ] **Step 4: Commit**

```bash
git add frontend/src/routes/\(app\)/settings/evolution/timeline/+page.svelte
git commit -m "feat(frontend): add delete button to evolution timeline page"
```

---

### Task 8: Handle 204 No Content in API Client (if needed)

**Files:**
- Possibly modify: `frontend/src/lib/utils/api.ts`

- [ ] **Step 1: Check if `api()` handles empty responses**

Read `frontend/src/lib/utils/api.ts` and check what happens when the response has no body (HTTP 204). If it tries to `response.json()` unconditionally, it will throw.

- [ ] **Step 2: Fix if needed**

If the `api()` function doesn't handle 204, add a check before parsing JSON:

```typescript
if (response.status === 204) {
    return undefined as T;
}
```

This should go right after the success status check and before the `response.json()` call.

- [ ] **Step 3: Verify frontend compiles and tests pass**

Run: `cd /home/harri/Projects/Repos/sober/frontend && pnpm check 2>&1 | tail -5 && pnpm test --silent 2>&1 | tail -10`
Expected: no errors, all tests pass

- [ ] **Step 4: Commit (if changes were made)**

```bash
git add frontend/src/lib/utils/api.ts
git commit -m "fix(frontend): handle 204 No Content in API client"
```

---

### Task 9: Final Verification

- [ ] **Step 1: Run full backend checks**

```bash
cd /home/harri/Projects/Repos/sober && cargo fmt --check -q && cargo clippy -q -- -D warnings 2>&1 | tail -10 && cargo test --workspace -q 2>&1 | tail -10
```

Expected: format OK, no clippy warnings, all tests pass

- [ ] **Step 2: Run full frontend checks**

```bash
cd /home/harri/Projects/Repos/sober/frontend && pnpm check 2>&1 | tail -5 && pnpm test --silent 2>&1 | tail -10
```

Expected: type check passes, all tests pass

- [ ] **Step 3: Version bump**

This is a `feat/` branch, so bump MINOR version. Check current version and bump `sober-core`, `sober-db`, `sober-api` Cargo.toml versions.

- [ ] **Step 4: Final commit and move plan to active**

```bash
git mv docs/plans/pending/055-evolution-delete docs/plans/active/055-evolution-delete
git add -A
git commit -m "docs(plans): activate #055 evolution delete plan"
```
