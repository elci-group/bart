Output assets/vhs/differential.gif
Set Theme "Catppuccin Macchiato"
Set Shell "bash"
Set FontSize 18
Set Width 1200
Set Height 400
Set Padding 40

Hide
Type "export PATH=$PWD/target/release:$PATH"
Enter
Ctrl+L
Show

# First scan to populate cache
Type "bart > /dev/null"
Enter
Sleep 1s

# Simulate a change
Type "touch NEW_BLOB_FILE && dd if=/dev/zero of=NEW_BLOB_FILE bs=1M count=50"
Enter
Sleep 1s
Ctrl+L

# Show the diff
Type "bart --diff"
Sleep 500ms
Enter
Sleep 5s

Hide
Type "rm NEW_BLOB_FILE"
Enter
Show
