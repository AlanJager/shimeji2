# Windows preview acceptance

Status: cross-compiled development preview; **Windows desktop acceptance pending**.
Use Windows 10 1607+ / Windows 11 x64, a normal user account, and the complete
extracted ZIP. Do not mark a check passed based only on a build or headless test.

- [ ] Double-click from an extracted folder with Chinese characters and spaces;
      start from another working directory. The bundled shime loads without Rust,
      Java, a developer checkout or administrator rights.
- [ ] It stays alive past 20 seconds without `--seconds`; the tray menu exits and
      removes all pet windows and the tray icon.
- [ ] Only opaque pet pixels accept input. Transparent margins allow clicking
      Explorer / another process underneath. Pets do not steal keyboard focus.
- [ ] Drag, cross monitor boundaries and release quickly/slowly. Throw direction
      follows the cursor. Alt-tab / interrupted capture never leaves a pet stuck.
- [ ] Right-click the pet and tray; click outside to dismiss. Tray pause/resume,
      add/remove, hide/show and Ctrl+Shift+S work. Paused pets remain draggable.
- [ ] “找回桌宠” returns every pet to a visible screen, including while paused.
      Removing the last pet leaves the tray usable for adding another.
- [ ] Open a ZIP through the menu, drag a ZIP/folder onto the EXE, and drop a ZIP
      on a pet. Unicode paths work, character selection works with `--character`,
      and invalid input shows an error while the existing pet remains usable.
- [ ] Load an English/Japanese classic pack, a native pack and a Codex pack.
      Check explicit right-facing art, mirrored anchors, and varying sprite sizes.
- [ ] Desktop floor respects the taskbar. Foreground window borders update after
      move/resize/minimize/maximize; Explorer desktop/taskbar are not platforms.
- [ ] Test 100/150/200% DPI, a monitor left of/above the primary, mixed DPI,
      unplug/reconnect and primary-display changes. Use `--scale` if needed.
- [ ] Restart Explorer: tray icon returns. Sleep/wake and lock/unlock recover
      without replaying a backlog of animation ticks.
- [ ] Leave 1 and 20 pets running for 30 minutes; record CPU, memory and GDI
      object counts. Pause/hide/resume and repeated pack switching do not leak.
- [ ] Check downloaded first-open behavior and antivirus on a second computer.
      Preview is unsigned; do not require disabling system security.

Automated Windows checks: `cargo clippy -p shimeji-windows --all-targets -- -D warnings`,
`cargo test -p shimeji-windows`, release build, pack checksums, ZIP manifest, then
run the relocated EXE with `--check-pack` and require exit code 0. This smoke test
does not create windows and cannot replace the checklist above.


## Pomodoro companion acceptance

Pending: the implementation was developed on macOS and cross-compiled. The user
will perform Windows desktop acceptance. Start using an isolated profile, e.g.
`shimeji2.exe --profile "%TEMP%\shimeji2-pomodoro-test"`.

- [ ] Tray → 番茄钟设置: edit 25/5/15 minute defaults, save and restart. Confirm
      preferences persist and the previous running stage does not resume.
      Enter 0, 181, empty text and a decimal: show an error and keep old settings.
- [ ] Start a 1-minute focus stage. Tray tooltip, open tray-menu status row,
      settings status and optional badge show the same countdown. The tray menu
      may remain open across the deadline; completion must arrive once.
- [ ] Start with a climbing/falling pet and add more while focused. All appear
      in safe static poses on the desktop floor, with no running, throwing or
      breeding. Drag still works; release leaves the pet still. Remove every pet:
      the timer remains usable from the tray.
- [ ] Pause/resume and cancel. Paused focus keeps pets calm. Cancel does not add
      a completed round. Hide/show, the pet Pause command and Ctrl+Shift+S do not
      pause the timer. Ending focus respects the current user Pause/Hide state.
- [ ] Lock/unlock, sleep/wake and disconnect/reconnect the session. The current
      stage pauses and waits for manual Continue; overlapping lock + sleep must
      not enable Continue prematurely. Turning off only the display leaves the
      timer running.
- [ ] Complete four focus stages with manually started rests. Fourth completion
      offers long rest; no phase starts automatically and no callback duplicates
      the completed count or reminder. Duration changes affect the next stage.
- [ ] With system notifications enabled, a completion balloon appears once.
      Default sound is off; enable sound and confirm one sound. Disable system
      notification display / enable Focus Assist: completion remains visible in
      the tray and settings even if the OS suppresses the balloon. Do not treat
      Shell_NotifyIconW success as proof of visible delivery.
- [ ] Optional **头顶耐心条与倒计时** card is hidden by default, never steals focus,
      passes mouse clicks through the entire card, stays on screen
      with a left/above secondary monitor, hides with pets, and moves during drag.
      Check settings keyboard Tab/Shift+Tab/Enter and 100/150/200% DPI.
- [ ] Start a one-minute stage: `耐心：MM:SS` and a shrinking bar above the pet.
      At 45/30/15 seconds remaining, color becomes yellow/orange/red. Pause
      freezes both without refilling on resume. Completion keeps an empty red
      bar and 00:00; Cancel hides it; each new stage starts full. Editing minutes
      mid-stage must not change its ratio. Rest animations keep the card attached.
- [ ] Drag along top/left/right work-area edges and negative-coordinate monitors;
      the card stays visible, moves below the pet if needed, and scales with DPI.
      Track GDI handles through repeated updates to catch resource growth.
- [ ] Normal Quit removes tray, pet windows and badge and leaves no future
      reminder. Restart Explorer during a running stage: tray icon and current
      tooltip return. Compare CPU/GDI counts with focus on/off and user Pause.

The Windows CI headless job now includes `shimeji-app` timer/settings tests plus
`shimeji-windows` real imported-pack → quiet runtime → BGRA rendering tests.
Reference APIs: [session changes](https://learn.microsoft.com/en-us/windows/win32/termserv/wm-wtssession-change),
[power broadcasts](https://learn.microsoft.com/en-us/windows/win32/power/wm-powerbroadcast-messages),
and [notification data / NIIF_NOSOUND](https://learn.microsoft.com/en-us/windows/win32/api/shellapi/ns-shellapi-notifyicondataw).
