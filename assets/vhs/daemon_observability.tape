Output assets/vhs/daemon_observability.gif
Set Theme "Catppuccin Macchiato"
Set Shell "bash"
Set FontSize 18
Set Width 1200
Set Height 450
Set Padding 40

Hide
Type "export PATH=$PWD/target/release:$PATH"
Enter
Ctrl+L
Show

# Daemon check
Type "bart daemon status"
Sleep 500ms
Enter
Sleep 2s

# Enable Auto-indexing
Type "bart index auto true"
Sleep 500ms
Enter
Sleep 2s

# Add to index manually
Type "bart index add ."
Sleep 500ms
Enter
Sleep 2s

# List tracked paths
Type "bart index list"
Sleep 500ms
Enter
Sleep 3s
