# Records docs/demo.gif: an "agent" works through a list while the TUI shows its
# changes live. Needs wedo on PATH and vhs (https://github.com/charmbracelet/vhs):
#
#   cargo install --path . && vhs docs/demo.tape

Output docs/demo.gif

Set Shell "bash"
Set FontSize 18
Set Width 1200
Set Height 560
Set Padding 20
Set Theme "Dracula"
Set TypingSpeed 50ms

# Set up a throwaway list, and an agent that will work through it in the background.
Hide
Type "export XDG_DATA_HOME=$(mktemp -d) WEDO_AUTHOR=claude-code" Enter
Type "wedo -l auth-refactor add '# backend' 'Add a sessions table' 'Hash passwords with argon2' 'Write the login handler' '# tests' 'Cover expired tokens' > /dev/null" Enter
Type "wedo -l groceries add 'oat milk' 'coffee beans' > /dev/null" Enter
Type "(sleep 4; wedo -l auth-refactor done 2; sleep 3; wedo -l auth-refactor done 3; sleep 3; wedo -l auth-refactor add --after 4 'Rate-limit failed logins') > /dev/null 2>&1 &" Enter
Type "clear" Enter
Show

Type "wedo -l auth-refactor"
Sleep 500ms
Enter

# Watch the agent check items off and add one; each change is highlighted.
Sleep 12s

# Then do some work ourselves.
Type "j"
Sleep 300ms
Type "j"
Sleep 300ms
Type "j"
Sleep 300ms
Type "j"
Sleep 500ms
Space
Sleep 1s
Type "h"
Sleep 2s
Type "h"
Sleep 1s
Tab
Sleep 2s
Type "q"
Sleep 500ms
