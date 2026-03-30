Output demo.gif

Set Theme "Catppuccin Macchiato"
Set Shell "bash"
Set FontSize 18
Set Width 1200
Set Height 600
Set Padding 40

# Setup path for local binary
Type "export PATH=$PWD/target/release:$PATH"
Enter
Ctrl+L

# Initial Scan
Type "bart"
Sleep 500ms
Enter
Sleep 3s

# Show Daemon Status
Type "bart daemon status"
Sleep 500ms
Enter
Sleep 2s

# Enable Auto-indexing
Type "bart index auto true"
Sleep 500ms
Enter
Sleep 2s

# Filter Mode
Type "bart -f"
Sleep 500ms
Enter
Sleep 2s
Right
Sleep 1s
Right
Sleep 1s
Right
Sleep 1s
Down
Sleep 500ms
Down
Sleep 500ms
Escape
Sleep 2s
