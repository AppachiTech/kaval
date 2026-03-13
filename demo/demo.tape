# Kaval Demo — Guard your ports.
# Shows: list, sort, check, JSON, TUI navigation, detail pane, filter, sort cycle

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

# --- CLI: list all ports ---
Type "kav list"
Sleep 200ms
Enter
Sleep 3s

# --- CLI: sort by CPU ---
Type "kav list --sort cpu"
Sleep 200ms
Enter
Sleep 3s

# --- CLI: check specific ports ---
Type "kav check 443 5432 3000"
Sleep 200ms
Enter
Sleep 3s

# --- CLI: JSON output ---
Type "kav list --json | head -10"
Sleep 200ms
Enter
Sleep 3s

# --- Launch TUI ---
Type "kav"
Sleep 200ms
Enter
Sleep 2s

# --- Navigate down ---
Down 4
Sleep 500ms

# --- Open detail pane ---
Ctrl+D
Sleep 1.5s

# --- Browse with detail pane open ---
Down 3
Sleep 500ms

# --- Filter ---
Type "/"
Sleep 300ms
Type "postgres"
Sleep 1.5s

# --- Clear filter ---
Escape
Sleep 800ms

# --- Cycle sort (Port → Name → CPU) ---
Ctrl+S
Sleep 800ms
Ctrl+S
Sleep 800ms

# --- Close detail pane ---
Ctrl+D
Sleep 800ms

# --- Quit ---
Ctrl+Q
Sleep 500ms
