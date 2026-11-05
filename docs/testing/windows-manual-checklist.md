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
