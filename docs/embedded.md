# Embedded desktop capture

`usahp_daemon::embedded::EmbeddedBroker` provides managed in-process keyboard capture on Windows and macOS. It does not bind a socket or change the standalone broker wire protocol. Constructing it captures nothing. Configure keyboard mappings, then call `enable` or `learn`. Both acknowledge native startup and return a generation. Call `heartbeat` every 500 ms independently of rendering and drain typed events regularly.

`Event::Switch` contains only physical logical transitions, using `SwitchStateMachine` aggregation. `Event::Stopped` cancels every pending gesture; it is never a release. Pending edges are discarded on cancellation. Escape stops immediately; the default minimum sustained-hold escape is four seconds, and the embedding application supplies a longer duration when needed. Missing heartbeats, queue overflow and known capture failures stop capture. Already consumed keys drain their releases while capture is stopped. Newly pressed keys pass through.

`learn` captures the first supported complete press/release. Escape cancels learning. `configure` is allowed only while off. `stop` pauses capture; `shutdown` removes the native hook/tap and joins its thread. Drop also shuts down native resources. Map supported canonical keyboard names; Windows supports F1–F24, macOS F1–F20. The public daemon's older input paths and protocol remain unchanged.

The library reports permission/startup failures instead of claiming capture is active. macOS requires Accessibility permission for the host application. Physical suppression, focus retention, and UIAccess/elevated Windows behavior require native manual testing. Automated tests exercise the state machine without hooks or input injection.
