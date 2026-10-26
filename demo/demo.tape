# Kaval Demo — Guard your ports.
# Shows: list, exposed listeners, check with a probe, JSON, pick; then the TUI:
# navigation, detail pane, exposure view, filter, sort, copy menu, help.
# Record on a machine with a few dev servers running: vhs demo/demo.tape

Output demo/demo.gif

Require kav

Set Shell "zsh"
Set FontSize 14
Set FontFamily "Menlo"
Set LetterSpacing 0
Set LineHeight 1.2
Set Width 1400
Set Height 800
Set Theme {"name":"Kaval","black":"#000000","red":"#ff5555","green":"#50fa7b","yellow":"#f1fa8c","blue":"#bd93f9","magenta":"#ff79c6","cyan":"#8be9fd","white":"#f8f8f2","brightBlack":"#6272a4","brightRed":"#ff6e6e","brightGreen":"#69ff94","brightYellow":"#ffffa5","brightBlue":"#d6acff","brightMagenta":"#ff92df","brightCyan":"#a4ffff","brightWhite":"#ffffff","foreground":"#f8f8f2","background":"#000000","selectionBackground":"#44475a","cursorColor":"#f8f8f2"}
Set Padding 10
Set Framerate 24
Set TypingSpeed 30ms
Set WindowBar Colorful
Set BorderRadius 8

# --- CLI: every listening port, with bind address and project ---
Type "kav list"
Sleep 200ms
Enter
Sleep 3s

# --- CLI: only what other machines can reach ---
Type "kav list --exposed"
Sleep 200ms
Enter
Sleep 3s

# --- CLI: what's on these ports, and does it answer? ---
Type "kav check 5432 3000 --probe"
Sleep 200ms
Enter
Sleep 3s

# --- CLI: JSON (schema 2) ---
Type "kav list --json | head -20"
Sleep 200ms
Enter
Sleep 3s

# --- CLI: a free port for a new server ---
Type "kav pick"
Sleep 200ms
Enter
Sleep 2s

# --- Launch the TUI (the detail pane is open on wide terminals) ---
Type "kav"
Sleep 200ms
Enter
Sleep 2.5s

# --- Navigate; the detail pane follows the selection ---
Down 3
Sleep 1s
Down 2
Sleep 1s

# --- Only listeners exposed to the network, then everything again ---
Type "e"
Sleep 1.5s
Type "e"
Sleep 800ms

# --- Filter ---
Type "/"
Sleep 300ms
Type "postgres"
Sleep 1.5s

# --- Esc clears the filter ---
Escape
Sleep 800ms

# --- Sort: Port → Name → CPU, then reverse ---
Type "s"
Sleep 800ms
Type "s"
Sleep 800ms
Type "S"
Sleep 800ms

# --- Copy menu (Esc closes it) ---
Type "y"
Sleep 1.2s
Escape
Sleep 500ms

# --- Help ---
Type "?"
Sleep 2s
Type "?"
Sleep 500ms

# --- Quit ---
Type "q"
Sleep 500ms
