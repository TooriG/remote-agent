# Sidebar drag and drop

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

The owning implementation is `agent-core::state::projects`; presentation and both
native adapters consume its results. Regression coverage includes persisted and
Host-scoped ordering, refresh deduplication, cross-provider session identities,
search, archive/restore and GPUI mouse-driven drops in both directions.
