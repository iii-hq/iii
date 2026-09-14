# Keep generated media outside the repository:
# vhs --output /tmp/compose-download-progress.gif .github/vhs/compose-download-progress.tape
# vhs publish /tmp/compose-download-progress.gif
# This records the real renderer with simulated events, not a live compose project.

Require cargo

Set Shell "bash"
Set Theme "Catppuccin Mocha"
Set FontSize 20
Set Width 1400
Set Height 1000
Set Padding 24
Set WindowBar "Rings"
Set TypingSpeed 15ms
Set Framerate 20

Type "cargo test -q -p iii-compose --lib download_panel_fixture -- --ignored --nocapture"
Sleep 400ms
Enter
Sleep 12s
