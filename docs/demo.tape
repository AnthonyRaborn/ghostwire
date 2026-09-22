# Records docs/demo.gif. Regenerate with: cargo build --release && PATH="$PWD/target/release:$PATH" vhs docs/demo.tape
Output docs/demo.gif

Set Shell "bash"
Set FontSize 14
Set Width 1600
Set Height 820
Set Padding 10
Set Theme { "background": "#07080f", "foreground": "#c8d3f5" }
Set TypingSpeed 40ms

# An empty config dir, so the recording never picks up a real config.
Hide
Type "export XDG_CONFIG_HOME=$(mktemp -d) PS1='$ '" Enter
Type "clear" Enter
Show

Type "ghostwire --demo" Sleep 400ms Enter
Sleep 6s
# Dive into a few nodes, then surface.
Type "2" Sleep 5s
Type "4" Sleep 5s
Type "5" Sleep 5s
Type " " Sleep 3s
Type "q"
Sleep 500ms
