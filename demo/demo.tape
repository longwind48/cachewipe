# VHS tape for the README demo. Re-render with:  vhs demo/demo.tape
# Runs the real binary against a sandbox HOME seeded by setup-demo.sh, so the
# numbers are genuine but nothing on the real machine is touched.

Output demo/demo.gif

Set Shell "bash"
Set FontSize 16
Set Width 1180
Set Height 720
Set Theme "Catppuccin Mocha"
Set Padding 18
Set TypingSpeed 45ms

# --- hidden setup: sandbox HOME + a `cachewipe` shim on PATH ---
# Everything here is off-camera so the demo shows only the real commands.
Hide
Type "export PS1='$ '" Enter
Type "bash demo/prepare.sh" Enter
Sleep 4s
# Minimal PATH: the shim plus coreutils only. Docker is deliberately absent so
# the demo shows the clean "not installed" path instead of a daemon error.
Type "export HOME=/tmp/cwdemo/home PATH=/tmp/cwdemo-bin:/usr/bin:/bin" Enter
Type "cd $HOME && clear" Enter
Sleep 1s
Show

Sleep 800ms

# 1. Dry-run — reports what it would free, deletes nothing.
Type "cachewipe --root ~/projects" Sleep 500ms Enter
Sleep 5s

# 2. Apply — actually reclaims the space. Clear first so the payoff is readable.
Hide
Type "clear" Enter
Show
Sleep 400ms
Type "cachewipe --apply --root ~/projects" Sleep 500ms Enter
Sleep 5s
