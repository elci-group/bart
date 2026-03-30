Output assets/vhs/interactive_tui.gif
Set Theme "Catppuccin Macchiato"
Set Shell "bash"
Set FontSize 18
Set Width 1200
Set Height 500
Set Padding 40

Hide
Type "export PATH=$PWD/target/release:$PATH"
Enter
Ctrl+L
Show

Type "bart -f"
Sleep 500ms
Enter
Sleep 2s

# Cycle filters
Right
Sleep 1s
Right
Sleep 1s
Right
Sleep 1s

# Navigate
Down
Sleep 500ms
Down
Sleep 500ms
Up
Sleep 500ms

# Show action menu (Enter)
Enter
Sleep 3s

# Cancel (Esc/Invalid)
Type "4"
Enter
Sleep 1s
Escape
Sleep 1s
