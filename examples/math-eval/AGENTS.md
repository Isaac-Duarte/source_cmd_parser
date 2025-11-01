# Agents

## Shared State Pattern
- `State` is a typed key-value store stored behind `Arc<RwLock<_>>`. Use `state::StateHandleExt` helpers such as `with_read`, `with_write`, `get_async`, and `set_async` to access data without sprinkling lock code.
- Insert new shared components with `state.set(value)` (inside a `with_write` block) so they can be retrieved later using their concrete type via `state.get_or_insert_with::<Type, _>(...)`.
- Default entries include `Cooldowns`, which track per-user rate limiting. Add any additional defaults in `State::with_defaults` to ensure they exist before commands run.

## Commands

### Eval Command
- Location: `src/commands/eval.rs`
- Trigger: Evaluates chat messages routed to the `eval` global command.
- Behavior: Sanitizes the raw message (replaces `x` with `*`) and evaluates it with `meval`. Successful results are typed on the configured keyboard.
- Cooldown: Limits each user to `MESSAGE_LIMIT` (currently 5) executions within `COOLDOWN_DURATION` (currently 120 seconds) using the shared `Cooldowns` state.
- Failure Handling: Messages that already parse as plain numbers or cannot be evaluated are ignored without side effects.

## Adding New Commands
- Create a new module under `src/commands/` that follows the `handle` signature `(ChatMessage, StateHandle, Keyboard<_, _>) -> Result<(), SourceCmdError>`.
- Register shared state requirements in `State::with_defaults` or via a `with_write` call using `get_or_insert_with`.
- Document each new command and its state dependencies in this file to keep operators aligned.
