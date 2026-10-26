# Desktop project organization

Open the `…` menu next to a project in the sidebar. Codex and Claude projects
use the same controls:

- Pin/unpin a project; pinned projects appear first.
- Edit the display name and section. Enter a new section name to create it,
  or clear the section to return to unclassified projects.
- Move to an existing section using the Section submenu.
- Open a local root in Explorer/Finder/the system file manager. Projects with
  multiple roots let you choose a root. This action is disabled for remote Hosts.
- Archive project chats; restore them using the same menu.
- Remove the project from the list; restore it using the sidebar's `一覧に戻す`
  action. This removes the sidebar entry, not its folder or native provider project.

Organization is saved on the device, keyed by Host storage identity and native
project ID. It does not rename filesystem folders or modify provider configuration,
conversation files, project membership, drafts, or pending submissions.

Archiving records a timestamp boundary and the loaded idle conversation IDs.
Older conversations fetched on later pages are also hidden when their update
timestamp is known. Running conversations remain visible while active. New
conversations after the boundary remain visible; an unseen conversation without
an update timestamp remains visible rather than being hidden accidentally.
Restoring clears the boundary and archived IDs. Native history remains readable.

## Drag and drop

Desktop project headings and top-level chat rows are drag sources. Menu buttons,
new-chat buttons and subagent tasks are not drag sources.

- Dropping a project on the upper/lower half of another heading inserts it
  before/after that project. The insertion line indicates the position. Moving
  across pinned or named sections adopts the destination project's group.
- Dropping a chat on a project heading moves its sidebar placement, expands the
  destination and reads its page when connected. The highlighted heading is the
  destination. This works for standalone chats and chats in another project.
- Ordering and placements are device preferences scoped to the Host's storage
  namespace. They survive restart and list refresh. Drag payloads include that
  namespace; switching Hosts during a drag prevents the drop from being applied.
- Placement stores only the session identity and a display label. Live metadata,
  activity, unread and worktree indicators come from core's current state. A saved
  label allows a moved chat to remain visible before its original page is fetched.
- Conversation identity, provider assignment, working directory, transcripts and
  drafts are preserved. Sidebar placement does not move files or change execution
  directories. A chat can be dragged to another project again.
- Newly moved chats are visible in an already-archived destination. Subsequent
  archive/restore actions include them, and search filters their titles.

Shared organization and persistence are owned by
[agent-core::state::projects](../crates/agent-core/src/state/projects.rs).
Native views consume its state and presentation.
