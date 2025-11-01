# Setup Guide

## Configuration Required

1. **Configure the cfg file path**: Point to your game's cfg directory
   ```rust
   .cfg_file_path("/home/isaac/games/SteamLibrary/steamapps/common/Counter-Strike Global Offensive/game/csgo/cfg/scp.cfg")
   ```

2. **Set up your bind in-game**: In CS:GO/CS2 console, create a bind that executes the cfg:
   ```
   bind p "exec scp.cfg"
   ```

3. **Configure the bind key in code**: Tell the parser which key to press:
   ```rust
   .exec_bind_key(source_cmd_parser::enigo::Key::Layout('p'))
   ```

## How it works

1. Chat command is detected from log file
2. Response message is written to `scp.cfg` as `say {message}`
3. The configured bind key is automatically pressed
4. Game executes `exec scp.cfg`, which runs the `say` command
5. Message appears in chat

## Example cfg file content

After a command runs, your `scp.cfg` might contain:
```
say {Hello from the parser!}
```

The game will execute this and broadcast the message.