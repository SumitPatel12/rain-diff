# Gesture-assisted review foundations

Build a standalone Python gesture engine with developer-registered callbacks, prove it in a
local webcam playground, then connect it to Rain through a small application-specific adapter.
The engine recognizes gestures; Rain decides what they do.

This is a self-build learning and implementation plan for a playful, short-session,
movie-inspired review mode. It is not an implemented feature, a mouse replacement, an
accessibility guarantee, or a project to train a model. All checklists below describe future work.

## 1. Keep the Python engine independent of Rain

The Python component should be useful without Rain. It owns camera input, hand tracking,
gesture recognition, and callback dispatch. It must not import Rain code, know about files or
review state, issue keyboard shortcuts, click desktop controls, or choose application actions.

Developers register callbacks for the gestures they want to use. The same thumbs-up event
could mark a file viewed in Rain, advance a presentation in another application, or increment
a counter in the playground. Unregistered gestures have no application effect.

| Component | Owns | Must not own |
| --- | --- | --- |
| Python gesture engine | Camera lifecycle, landmarks, geometry, gesture state, callback registration and dispatch | File approval, scrolling policy, Rain commands, application targeting |
| Playground | Preview, diagnostic overlays, dummy targets, test callback implementations | Production review state |
| Rain adapter | Gesture subscriptions, transport, coordinate/mode translation, lifecycle wiring | A second implementation of gesture recognition |
| Rain UI | Target selection, action permissions, file state, persistence, rendering | Webcam inference on its UI thread |

Use gesture names such as `finger_tap`, `swipe_up`, `thumbs_up`, `palm_to_fist`,
`pinch_out`, and `pinch_in`. Do not name engine events `approve_file` or `close_window`.
Those meanings belong to the callbacks supplied by the consuming developer.

### Callbacks cross a language boundary through an adapter

A Python callback is a callable in the Python process. Rain is a native Rust application, so
it cannot simply pass a Rust closure to a separate Python process without a bridge.

For the initial integration, recommend a small Python adapter launched by Rain. It imports
the standalone engine and registers callbacks that forward gesture events over a local
process pipe. Rust receives them and runs the Rain-owned handlers on the UI thread. This
preserves the callback design without embedding Python or adding a server to the engine.

Rain's developer controls which gestures are subscribed, which handlers run, and when the
mode is enabled. The forwarding callbacks are adapter code, not a dependency of the engine.
A future Python consumer can register ordinary in-process callbacks without any transport.

Rain currently documents a single-native-process architecture in [decisions.md](decisions.md).
An optional camera helper is a deliberate experimental extension of that boundary. Keep it
off by default, avoid runtime downloads, and revisit the architecture documentation if this
experiment becomes a supported feature. This plan does not change Rain's current behavior.

### Define a small callback contract before publishing a package

Do not build a general plugin system. Begin with registration of one handler per gesture and
an explicit way to unregister it. Multiple subscribers and async handlers can wait until a
real consumer needs them. Distinguish registration from recognition configuration: a gesture
with no handler may still participate in ambiguity suppression, but it triggers no action.

Document these rules as the implementation develops:

- A discrete gesture commits once per completed interaction. Holding a pose does not repeatedly
  invoke its completion callback; a neutral reset is required before rearming.
- A callback receives a gesture observation, not an application command. Include a monotonic
  timestamp, interaction ID, phase, and relevant normalized position or movement data.
- Use `started`, `updated`, `completed`, and `cancelled` phases where progress matters. Only
  `completed` performs a discrete application action. A pointer sample is a continuous update.
- Progress is elapsed hold time or another measured quantity, not an invented probability.
  Landmark confidence, when available, is not confidence that the user intended an approval.
- Initially invoke handlers serially on the engine's processing thread, outside internal locks.
  Keep handlers short: update a counter or enqueue a message. Do not block on UI work or I/O.
- Surface callback exceptions to the host and stop processing safely, releasing the camera.
  Never silently swallow an exception or retry a callback that may already have acted.
- Provide start, pause, resume, and stop behavior. Pause cancels in-flight interactions;
  resume requires fresh neutral input. Stop releases the camera even after an error.
- Report tracking loss separately and cancel candidates. A newly detected hand must not
  complete an interaction started by a previously tracked hand.

Keep the recognizer itself pure: timestamped observations in, gesture events out. A small
engine layer dispatches those events to callbacks. This makes replay tests possible without
a webcam and lets you change capture or UI code without rewriting the recognizer.

## 2. Define the first Rain gesture vocabulary

The experience is to open a review, explicitly enable gesture mode, point at something,
tap fingertips to activate it, swipe through the diff, and hold thumbs-up to mark a file
reviewed. Visible feedback should make Rain's interpretation clear. Mouse and keyboard remain
available throughout.

| Gesture | Rain callback behavior | Initial rule |
| --- | --- | --- |
| Index finger extended | Move an in-app pointer | No operating-system cursor control; begin in a bounded playground area. |
| Brief thumb–index fingertip contact, then release | Click the targeted control | One click on valid release; freeze the target while fingers close. |
| Index and middle fingers extended, swipe upward | Scroll down through the review | Content moves upward, revealing later lines; one impulse per swipe. |
| Same pose, swipe downward | Scroll up through the review | Content moves downward, revealing earlier lines. |
| Thumbs-up held briefly | Mark the current file viewed | Set viewed, never toggle it back or accept a group. |
| Open palm followed by fist | Dismiss the top supported floating panel | Require the transition, not a fist entering frame; protect unsent text. |
| Pinch out | Expand file, then show whole file | Later milestone; one expansion step per completed gesture, then no-op. |
| Pinch in | Collapse the current file to its header | Later milestone; reopening preserves the previous content extent. |

“Approve” means Rain's local viewed state, not a GitHub review approval. Rain already folds
viewed files; initially preserve that behavior. These mappings live in Rain's integration,
not as built-in meanings in the Python engine.

### Resolve click-versus-pinch ambiguity explicitly

A brief fingertip tap and the beginning of a pinch can look identical. A distance threshold
alone cannot infer intention.

First implement click with sizing gestures disabled. For the sizing experiment, add an explicit
keyboard-controlled **Size mode** with a visible indicator. Disable click actions there;
pinches change file size instead. Mode changes cancel candidates and require a neutral reset.
The engine should expose generic gesture enablement/arbitration controls, not a Rain-specific
file-sizing mode.

The manual switch is experimental scaffolding. Once each interaction works independently,
try held-pinch arming for expansion or a second-hand sizing modifier. Pinch-in starts with
separated fingers, so it needs its own eligibility rule. Keep the switch until trials show
that click, expansion, and collapse are reliably distinguishable.

## 3. Learn the foundations as you need them

You do not need neural-network training theory to begin. MediaPipe supplies a pretrained
deep-learning model; most of your work is geometry, timing, state, and interaction design.

| Learn | Why | Small exercise |
| --- | --- | --- |
| Python environments, functions, records, and callbacks | Build a reusable component with host-supplied behavior | Register a handler and invoke it with a dummy gesture event. |
| Webcam frames and color formats | Video is a sequence of images | Show the camera and release it cleanly. |
| Normalized coordinates and mirroring | Map camera observations predictably | Draw a fingertip dot and deliberately mirror the preview. |
| Distances, vectors, joint angles | Describe pinches, extended fingers, and swipes | Display fingertip separation divided by palm width. |
| State machines and monotonic time | Recognize an interaction over time | Hold a simulated pose and emit one completion event. |
| Smoothing and hysteresis | Avoid jitter and rapid state flipping | Compare a pointer with and without smoothing. |
| Local process communication | Connect Python callbacks to Rust handlers | Forward a harmless diagnostic event to Rain. |

A **landmark** is an estimated joint or fingertip position. A **pose** is a hand shape now;
a **gesture** is a meaningful pose or movement over time. **Inference** runs an already-trained
model. **Hysteresis** uses different enter and exit thresholds to avoid noisy state flipping.

## 4. Set up a local webcam playground

Assume Windows initially, matching Rain's documented primary build target. Start with one
person, one selected hand, a normal webcam, and a well-lit desk. The orb used to prepare this
plan cannot validate your physical camera or local permissions.

Use Python 3.11 or 3.12 in a virtual environment. These are conservative starting choices,
not mandatory versions. If installation fails, check MediaPipe wheel support for your exact
Python version, OS, and architecture before changing libraries.

In a new local playground directory, initially separate from Rain:

```powershell
py -3.11 -m venv .venv
.\.venv\Scripts\python.exe -m pip install --upgrade pip
.\.venv\Scripts\python.exe -m pip install mediapipe opencv-python pytest
```

Use the environment's Python directly; activation and PowerShell execution-policy changes
are unnecessary. These are setup instructions, not commands tested on your computer.

Download the Hand Landmarker `.task` model from the official model page and store it locally.
Installing the library does not put a model at your application's chosen path. Record the
model source and checksum, and pin working dependency versions after the first successful run.
Check camera permissions and close applications monopolizing the camera.

Use the MediaPipe Tasks `HandLandmarker` API consistently. Older tutorials using
`mp.solutions.hands` describe a different API. Do not mix their setup and result types.

Start with one Python file for the camera experiment. Extract the pure recognizer when replay
tests become useful, and separate the callback engine from the playground before connecting
Rain. Do not publish a package or create a plugin framework yet. Keep environments, private
recordings, and downloaded model files out of source control by default.

## 5. Build in milestones with explicit exit criteria

### A. Display the camera and landmarks

- [ ] Open the webcam with OpenCV and display frames.
- [ ] Try 640 × 480 at 30 FPS and read back what the device actually provides.
- [ ] Convert BGR frames to RGB before constructing the MediaPipe image.
- [ ] Run Hand Landmarker for one hand and draw its 21 landmarks.
- [ ] Display tracking state, processed FPS, and inference duration.
- [ ] Release the camera on normal exit and exceptions.

CPU inference is a reasonable first experiment; measure before configuring GPU execution.
Begin with synchronous `VIDEO` mode and strictly increasing millisecond timestamps derived
from a monotonic clock, even when frames come from a webcam. This avoids asynchronous result
handling while learning. Do not let capture buffering make the preview increasingly delayed.

If later needed, `LIVE_STREAM` returns results asynchronously and may skip submitted frames
when busy. Its model-result callback is distinct from your public gesture callbacks. Publish
the latest observations safely rather than mutating a GUI directly from a model callback.

Choose one mirroring convention. Mirroring before inference and displaying that same image
makes the diagnostic overlay straightforward. Do not mirror the coordinates again downstream.
Test actual left and right hands: handedness labels are affected by mirroring and are not
stable tracking identities.

**Exit criterion:** landmarks follow your hand, disappearance/reappearance does not crash,
and the preview stays current. Fix perception problems before gesture tuning.

### B. Measure features without performing actions

- [ ] Label thumb tip 4, index tip 8, and middle tip 12.
- [ ] Display palm-normalized thumb–index separation.
- [ ] Estimate extended fingers using joint angles and palm-relative geometry.
- [ ] Display candidate pointing, two-finger, palm, fist, and thumbs-up labels.
- [ ] Preserve an explicit unknown state for ambiguous observations.

Measure distances in a common coordinate system. Normalized x and y use different image
dimensions: multiply by frame width and height before measuring pixel distances, then divide
by a palm size measured in those same units. Index MCP 5 to pinky MCP 17 is a possible palm
scale. Reject measurements when that span becomes unreliable edge-on.

Avoid “tip y is above knuckle y, therefore extended” rules; hand rotation breaks them.
Initially support deliberate, mostly camera-facing poses. For thumbs-up, require the other
fingers curled and the thumb extended roughly upward, not merely a visible thumb.

**Exit criterion:** you understand which measurements produce each label and how rotation,
distance, and occlusion affect them. Still no application actions.

### C. Recognize gestures and invoke playground callbacks

- [ ] Extract the pure timestamped recognizer and test it without a camera.
- [ ] Add callback registration around its output; use counters as the first handlers.
- [ ] Represent eligibility, candidate, completion/cancellation, and neutral rearm.
- [ ] Emit interaction lifecycle events where the host needs hold progress or target latching.
- [ ] Cancel on tracking loss, pause, mode changes, and hand identity discontinuity.

These timing values are calibration hypotheses, not validated defaults:

| Setting | Starting experiment | Purpose |
| --- | --- | --- |
| Pose stability | 100–150 ms | Reject a single misclassified frame. |
| Finger tap duration | 60–300 ms | Distinguish a tap from flicker or a sustained pinch. |
| Thumbs-up hold | 500–700 ms | Make approval deliberate and show progress. |
| Palm-to-fist transition window | About 1 second | Reject unrelated poses separated by a long pause. |
| Additional repeat guard | About 300 ms | Supplement neutral reset, never replace it. |

Use elapsed time, not frame counts. Calibrate separate pinch-enter and release thresholds
from your normalized measurements rather than copying a pixel threshold from a tutorial.

For a tap, require stable pointing first. Emit completion on a valid release. Excessive
motion, a long hold, a competing gesture, tracking loss, or a mode change cancels it. Never
turn a cancelled hold into a delayed tap. Include the pre-contact position so the host can
freeze its target while the index finger curls.

For swipes, require the two-finger pose first and track palm/knuckle motion, not just fingertip
articulation. Require enough vertical displacement within a time window, with vertical motion
dominating horizontal motion. Emit once and require neutral reset before the return stroke.

The engine owns gesture eligibility and arbitration. The host owns target validity. A
thumbs-up `started` event lets Rain latch the target; a later `completed` event with the same
interaction ID lets Rain act only if that target and review context are still valid.

**Exit criterion:** one intentional gesture invokes one completion callback, a held pose does
not repeat, and tracking loss never completes an action. Callback failures release the camera.

### D. Make the playground interactive

- [ ] Add large dummy buttons, a scrollable text area, and dummy file cards.
- [ ] Supply callbacks that move a dot, activate a button, scroll, and mark a card reviewed.
- [ ] Add explicit enable/pause and an always-available keyboard escape route.
- [ ] Show candidates, targets, hold progress, last event, and cancellation reasons.
- [ ] Test pinch sizing separately from clicks using the temporary mode switch.
- [ ] Register a different thumbs-up callback and verify behavior changes without editing recognition.

Drawing the playground in the OpenCV preview is sufficient; another UI framework is optional.
Use harmless counters/cards so misfires are informative. Map a comfortable camera rectangle
to the target area, calibrate corners, and clamp positions. Try a simple time-aware exponential
smoother before advanced filters; too much smoothing feels laggy.

Arbitrate competing gestures once. During an owned swipe, do not also emit pointer updates or
tap completions. The library should not silently deliver contradictory interactions and expect
every consumer to reconstruct the recognition rules.

**Exit criterion:** every action is explainable from the visible state, the callbacks are
replaceable, and the engine has no dependency on dummy-card or Rain semantics.

## 6. Connect developer-controlled callbacks to Rain

### Start with a small process adapter

Launch the Python adapter only when gesture mode is enabled. It imports the standalone engine,
registers the desired callbacks, and forwards their observations as newline-delimited JSON
on stdout. Send diagnostics to stderr. Use stdin for a minimal control channel such as pause,
resume, mode configuration, and shutdown. Keep this transport outside the reusable engine.

Document a small protocol: version, session ID, sequence number, interaction ID, gesture,
phase, observation time, and relevant coordinates or movement. Camera data and application
commands must not be mixed into that schema. Stop and release the camera on shutdown or
pipe closure; Rain must also clean up its child process if graceful shutdown fails.

Read messages off the UI thread. Coalesce pointer updates into a latest-value slot and bound
discrete-event buffering. Discard stale input and clear pending interactions after a helper
restart. Wake Rain when new input arrives; a channel alone is insufficient when its UI is idle.
Never run inference or wait for a Python callback on the Rust UI thread.

Python and Rust monotonic clocks cannot be assumed to share an epoch. Establish a clock
offset/session handshake before comparing observation timestamps across processes, or use an
explicit common-clock scheme with documented limitations. Measure receiver queue delay too.

The Rust adapter maps incoming gestures to Rain handlers. Initially keep these bindings in
code under developer control; user-editable bindings and dynamic reconfiguration can wait.
Pause safely on helper failure and leave normal mouse/keyboard review unaffected.

### Reuse existing action owners

These are inspected ownership paths, not an existing gesture API:

| Location | Existing responsibility | Integration guidance |
| --- | --- | --- |
| `crates/rain-core/src/command.rs` | Commands such as `ToggleViewed`, `ViewedAndNext`, `ToggleWholeFile`, `Comment`, `Escape` | Reuse semantics, not fake keypresses. |
| `crates/rain-app/src/app/mod.rs`, `App::run` | Command dispatch | Follow the existing action ownership. |
| Same file, `App::poll` | Background messages and session updates | Follow receive-and-wake patterns for the adapter inbox. |
| `crates/rain-app/src/app/nav.rs` | Viewed navigation and toggling | Approval sets viewed once; never blindly invoke `ToggleViewed`. |
| `crates/rain-app/src/app/session.rs` | Viewed persistence and folding | Preserve reload/persistence behavior. |
| `crates/rain-app/src/app/wholefile.rs` | Whole-file state and loading | Check state before calling a toggle; already whole must not fold back. |
| `crates/rain-app/src/app/folds.rs` | Bulk folding | Do not map a current-file gesture to an all-files action. |
| `crates/rain-app/src/view/diff.rs` | Diff controls and presentation | Follow existing single-file header actions and targeting. |
| `crates/rain-server/src/routes/lifecycle.rs` | Application lifecycle routes | `/api/close` closes Rain, not a floating panel. |

The local HTTP server is not a general gesture-input API. A process adapter avoids extending
it just for the experiment. Reconsider transport only if independent process attachment is
actually needed; the Python engine's callback interface should not change with that choice.

### Integrate actions before generic pointer clicks

First use the current keyboard-selected file. Wire swipe, thumbs-up, and safe panel dismissal;
leave pointer clicks disconnected until the bridge is proven. Then add the in-app pointer.
Rain determines which widget or diff line is targeted, including overlays and scroll offsets.

Latch stable target identity on interaction start. A path alone is insufficient across
repositories, and file-list indices can go stale after filtering or reload. Cancel if the
target, focus, or review content changes before completion. Holding thumbs-up must not approve
the next file after the first one's viewed action changes the screen.

Inspect Rune's pointer dispatch and Rain's hit-testing before promising generic clicks. A
painted dot does not activate actual controls. Reuse the same control action paths where
possible. This native targeting detail remains implementation research.

Dismiss only the top supported overlay. Do not blindly invoke Escape: it can also cancel
composers or clear other state. Initially refuse gesture dismissal when there is unsent text
and ask the user to use the keyboard or mouse for that draft.

Separate fold state from content extent. Pinch-out opens a folded file; another completed
pinch-out loads its whole content; already whole or loading means no-op. Pinch-in folds to the
header without changing viewed state or comments, preserving the previous extent for reopening.
Respect existing whole-file limitations for binary files, missing repositories, added/deleted
files, and failed loads. Never queue repeated reads while one is pending.

## 7. Validate callbacks, recognition, and application effects separately

Use timestamped landmark replays for deterministic recognition tests and short, explicitly
recorded webcam trials for perception. Keep recording opt-in with a visible indicator.
Landmark motion is still personal data; do not casually publish it or camera recordings.
Routine logs should contain measurements and cancellation reasons, not video.

| Risk | Discriminating test |
| --- | --- |
| Engine hard-codes Rain actions | The same replay invokes different host callbacks without recognition changes. |
| Duplicate completion | A noisy tap emits one completion; a held pose does not repeat. |
| Tracking loss looks like release | Contact followed by missing observations emits cancellation, never a click. |
| Return stroke reverses scrolling | One swipe and repositioning produce one directional event. |
| Gestures compete incorrectly | A two-finger swipe emits no click or pointer motion during its owned interaction. |
| Callback lifetime is wrong | Unregistration stops delivery; stop releases the camera; handler errors are surfaced without retries. |
| Mode switch completes old input | Partial pinch followed by pause/resume requires a fresh interaction. |
| Fist alone dismisses panels | A fist entering frame emits no completion; armed palm-to-fist emits one. |
| Hold approves several files | Completion affects the latched target once, even if Rain's visible file changes. |
| Sampling rate changes timing | Equivalent adequately sampled motions preserve outcomes across frame rates. |
| Sizing toggles whole content off | Repeated pinch-outs stop at whole-file and do not duplicate pending reads. |
| Old input survives reconnection | Restarted sessions reject stale events and require neutral reset. |
| UI only changes an indicator | Viewed state survives reload; scrolling exposes expected lines; the intended panel actually closes. |

Start with a proposed trial of 20 deliberate attempts per gesture plus five minutes of ordinary
non-command movement while enabled. Include reaching for a mug, scratching your face, typing,
partial hands, and leaving the frame. Record correct actions, misses, wrong actions, and repeats.

An initial development gate could be at least 18/20 recognized attempts per enabled gesture,
no duplicates, and no accidental approvals or draft loss in the non-command trial. These are
proposed gates, not measured results or a production reliability claim. Keep unreliable
gestures in the playground rather than loosening every threshold together.

Measure capture-to-inference, inference-to-event, callback/transport delay, and UI response
separately. Verify normal review with gesture mode off. For Rust integration, run targeted
checks and the relevant repository gates from [README.md](../README.md), including
`scripts/gates.ps1` when integration is ready.

## 8. Package the Python engine only after a second consumer works

The standalone boundary is useful immediately; publishing is not required to establish it.
Before extracting a package, make the playground and Rain adapter both consume the same engine
through its public callbacks. Neither should import private recognition state.

- [ ] Separate reusable code, examples/playground, and Rain-specific adapter code.
- [ ] Add package metadata in `pyproject.toml`, dependency constraints, and documented Python support.
- [ ] Document callback phases, threading, payload coordinates, errors, and lifecycle.
- [ ] Provide a counter-only callback example that does not mention Rain.
- [ ] Make camera index and model path explicit; provide useful errors for missing resources.
- [ ] Verify the engine works without a preview window and releases resources on failure.
- [ ] Check model and dependency redistribution terms and choose the package license deliberately.
- [ ] Test an installed wheel in a clean environment, not just imports from the source directory.

Do not make model downloads or telemetry implicit. Publishing a package or moving to a separate
repository is a later delivery decision. There is no requirement to rewrite working Python
in Rust; native model deployment would be a separate investigation if it becomes worthwhile.

## 9. Add the movie feel after behavior is dependable

- Show a pointer halo only while the mode is active and tracking is fresh.
- Show the current interaction mode and the actual target file/control.
- Fill an approval ring during the hold; cancellation must not look like success.
- Give swipes a short bounded glide without fighting the view's existing scroll physics.
- Animate file expansion, with a distinct busy state while whole content loads.
- Fade the pointer and cancel candidates on tracking loss.
- Keep camera preview optional, pause unmistakable, and sound independently disableable.

These are Rain/playground presentation choices. The engine supplies observations and lifecycle
events, not animation policy. Visual feedback should acknowledge actions the host actually
accepted rather than hiding uncertain recognition.

## 10. First work sessions and scope

**Session 1: camera only.** Create the environment, display actual dimensions/FPS, and exit
cleanly. Leave inference and callbacks out until capture works.

**Session 2: landmarks only.** Load the official model, convert BGR to RGB, draw the skeleton,
and settle mirroring. Do not control Rain.

**Session 3: fingertip contact.** Display normalized separation, add hysteresis, and count
completed taps through a supplied counter callback. Try long holds, tracking loss, and jitter.

**Next checkpoints:** replayable recognizer, interactive playground, limited Rain bridge,
native targeting, sizing arbitration, then polish and packaging. Work by exit criteria rather
than promising a timeline before camera setup and the first gesture are understood.

Out of scope for the foundation: training models, LLM recognition, cloud video processing,
OS-wide mouse control, gesture text entry, speech recognition, complex range selection,
multi-user tracking, and cross-platform distribution. Initially, adding comments means using
Rain's composer and keyboard after selecting a line; dictation is a separate optional project.

The main risks are occlusion/lighting, click-versus-pinch ambiguity, callback latency, stale
targeting, native hit testing, and packaging friction. Diagnose them at the earliest milestone
that exposes them rather than adding a new model or abstraction by default.

## Sources and verification boundaries

Repository ownership was checked against `README.md`, `docs/decisions.md`,
`crates/rain-core/src/command.rs`, `crates/rain-app/src/app/mod.rs`, `nav.rs`, `session.rs`,
`wholefile.rs`, `folds.rs`, and the server route/lifecycle code. Proposed callbacks, gesture
modules, adapters, and protocol messages do not already exist.

- [MediaPipe Hand Landmarker and model](https://ai.google.dev/edge/mediapipe/solutions/vision/hand_landmarker)
- [MediaPipe Python setup](https://ai.google.dev/edge/mediapipe/solutions/setup_python)
- [OpenCV Python video capture](https://docs.opencv.org/4.x/dd/d43/tutorial_py_video_display.html)
- [Python monotonic clock](https://docs.python.org/3/library/time.html#time.monotonic)
- [Python virtual environments](https://docs.python.org/3/library/venv.html)

This is a researched plan, not a tested camera prototype. Setup commands, timing hypotheses,
trial gates, physical-camera performance, native targeting, and distribution must be validated
during implementation. This branch currently adds documentation only.
