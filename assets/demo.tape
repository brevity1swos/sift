# VHS demo tape for sift — https://github.com/charmbracelet/vhs
# Regenerate: export PATH=$PWD/target/release:$PATH && vhs assets/demo.tape
#
# The Hidden block populates a throwaway project at /tmp/sift-demo with a
# real sift ledger (3 simulated agent turns) via assets/demo_setup.sh, then
# the visible part shows the storytelling commands against that ledger.

Output assets/demo.gif

Set FontSize 16
Set Width 1180
Set Height 720
Set Theme "Catppuccin Mocha"
Set TypingSpeed 35ms
Set Shell "bash"

Hide
Type "export PATH=$PWD/target/release:$PATH; bash assets/demo_setup.sh; cd /tmp/sift-demo; clear"
Enter
Sleep 2s
Show

# An AI agent just edited this project across three turns.
# sift recorded every write in the background — keyed to the turn that caused it.
Type "sift status"
Enter
Sleep 3.5s

# "What did the agent change in turn 2?"
Type "sift list --turn 2"
Enter
Sleep 3s

# Did it leave any junk behind?
Type "sift sweep"
Enter
Sleep 4s

# Yes — a sibling file instead of an edit, plus a scratch note. Clean them up.
Type "sift sweep --apply"
Enter
Sleep 3s

# Only the real work is left pending.
Type "sift status"
Enter
Sleep 3.5s

# Accept the rest — in normal use this happens automatically on `git commit`.
Type "sift accept all"
Enter
Sleep 3s

Sleep 1s
